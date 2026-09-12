// Y6 — END TO END. Wilson: "we got to really think about this thing end to end."
// The seams between the features, which is where a product that works in pieces
// still feels broken.
//
// AUDIT at 4e8c9adf. SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.
// Gate, from docs/loop/HARNESS.md "The gate", run from the worktree's app dir:
//   npx tsc --noEmit · npm test · npm run build ·
//   cargo build -p yap-polish --release + stage src-tauri/binaries/yap-polish-<triple> ·
//   cargo test -p yap-polish --release ·
//   cargo clippy --all-targets --features custom-protocol (in src-tauri) ·
//   cargo test --features custom-protocol (in src-tauri).
// cargo fmt is INFORMATIONAL and exits 1 on unmodified main — never reformat to silence it.
//
// NEVER touch the bundle identifier (com.wilsonguenther.wilson-voice) or the data
// directory (WilsonVoice). Renaming the id resets every macOS TCC grant; renaming
// the data dir orphans the SQLite history.

ITEMS.push({
  id: 'UPD-A', prompt: 'Y6', branch: 'loop/upd-a-updater-endpoint-that-can-actually-serve', gated: null,
  title: 'The updater points at an endpoint that can serve a private repo — today it points at a dead URL',
  preflight: `
    test 0 -eq "$(grep -c 'releases/latest/download/latest.json' desktop/src-tauri/tauri.conf.json)"
    test -f desktop/src-tauri/tests/updater_endpoint.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test updater_endpoint
  `,
  spec: `
    MEASURED, desktop/src-tauri/tauri.conf.json plugins.updater.endpoints:
      "https://github.com/wilsonguenther-dev/wilson-voice/releases/latest/download/latest.json"
    and the repo's own history records why that cannot work: commit 2eabf33
    "site: serve the DMG from Forge — repo went private, GitHub release assets
    are no longer publicly downloadable", then 734aa8c "YV83 site + DMG served
    from Vercel, past Forge's 25MB edge cap". The DMG moved twice and the
    updater manifest URL did not move with it. docs/loop/HARNESS.md records the
    repo as PUBLIC again as of 2026-09-12 — which means the URL may resolve
    today and will silently die the next time the repo is flipped private
    (project_github_actions_public_window is an explicit, recurring procedure).
    An updater whose correctness depends on repo visibility is not an updater.

    Do:
      * Point \`plugins.updater.endpoints\` at the same host that serves the DMG,
        so the manifest and the asset can never disagree about where the build
        is. Read docs/DEPLOY-SITE.md and site/ for the current host before
        choosing, and state in the PR body which host you chose and why.
      * Keep the GitHub URL as a SECOND endpoint, after the primary. Tauri tries
        endpoints in order, so a private-repo window degrades to the primary
        instead of failing.
      * The signing pubkey stays exactly as it is
        (plugins.updater.pubkey, verified present at 4e8c9adf — a base64
        minisign key, not a placeholder). Never regenerate it in this item: a
        new keypair makes every installed copy unable to verify an update, and
        project_yap_build_state records one keypair regeneration already (YV82).
      * \`src/updater.ts\` is already correct in shape — check-only, no auto
        install, DEBUG not ERROR when there is no manifest (its own doc says
        so). Do not change its contract. Add ONE thing: when every endpoint
        fails, the manual "Check for updates" button must say which endpoint was
        tried, because a silent "you're up to date" on a dead endpoint is the
        failure this item exists to prevent.
      * \`tests/updater_endpoint.rs\`: the config parses; there are >= 2
        endpoints; the primary is not a github.com release-asset URL; the pubkey
        is non-empty and is not the string "PLACEHOLDER" or a bare newline; and
        \`createUpdaterArtifacts\` is still true.

    What NOT to do:
      - Do NOT regenerate the updater keypair.
      - Do NOT make the updater install on startup. User-triggered only
        (src/updater.ts's own contract: "USER-TRIGGERED ONLY").
      - Do NOT print any key material into a log, a test name or the PR body.
  `,
  acceptance: `
    # PANEL: the old line anchored the URL to end-of-line, so it passed only
    # while that endpoint happened to be last with no trailing comma. Parse it.
    node -e "const e=require('./desktop/src-tauri/tauri.conf.json').plugins.updater.endpoints; process.exit(e.length>=2 && !/github\\.com\\/.*\\/releases\\//.test(e[0]) ? 0 : 1)"
    node -e "const u=require('./desktop/src-tauri/tauri.conf.json').plugins.updater; process.exit(u.pubkey && u.pubkey.length>40 ? 0 : 1)"
    test -f desktop/src-tauri/tests/updater_endpoint.rs
    grep -q 'at_least_two_endpoints' desktop/src-tauri/tests/updater_endpoint.rs
    grep -q 'primary_endpoint_is_not_a_github_release_asset' desktop/src-tauri/tests/updater_endpoint.rs
    grep -q 'pubkey_is_present_and_not_a_placeholder' desktop/src-tauri/tests/updater_endpoint.rs
    grep -q 'createUpdaterArtifacts' desktop/src-tauri/tests/updater_endpoint.rs
    cd ${APP} && npm ci
    npx tsc --noEmit ; test $? -eq 0
    npm run build    ; test $? -eq 0
    cd src-tauri && cargo test --features custom-protocol --test updater_endpoint ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'UPD-B', prompt: 'Y6', branch: 'loop/upd-b-publish-the-updater-triple-and-keep-a-rollback', gated: null,
  title: 'Publish latest.json + .app.tar.gz + .sig to the host the updater now points at, and keep the previous build',
  preflight: `
    test -x scripts/release-local.sh
    grep -q 'latest.json' docs/DEPLOY-SITE.md
    grep -q 'app.tar.gz' docs/DEPLOY-SITE.md
  `,
  spec: `
    PANEL 2026-09-12, two seats independently. UPD-A repoints
    plugins.updater.endpoints at "the same host that serves the DMG" — and
    nothing publishes a manifest there. MEASURED at 4e8c9adf:
      docs/DEPLOY-SITE.md:40-46  the staging block copies *.html *.css *.woff2 +
        vercel.json and one \`gh release download --pattern '*.dmg'\`. No
        latest.json, no .app.tar.gz, no .sig.
      .github/workflows/release.yml:91-120  the ONLY producer of the updater
        manifest and the minisign .sig — and Actions is disabled account-wide
        (docs/loop/HARNESS.md, CI mode: the expected answer is \`local\`).
      tauri.conf.json:42 createUpdaterArtifacts true; :66-69 the endpoint and
        the pubkey.
    macOS's updater consumes the .app.tar.gz plus its .sig, not the DMG, so
    serving the DMG at that host is necessary and insufficient. Left as is,
    every installed copy reports "up to date" forever — src/updater.ts logs
    DEBUG, not ERROR, when there is no manifest — and a security fix reaches
    nobody. There is also no rollback: no known-good DMG retained on the host.

    Do:
      * \`scripts/release-local.sh\`: build, sign with SEC-A's identity,
        notarize, staple, emit Yap.app.tar.gz + .sig + latest.json with
        TAURI_SIGNING_PRIVATE_KEY, and stage all four next to the DMG.
      * Amend docs/DEPLOY-SITE.md's staging block to copy all four.
      * Verify the LIVE endpoint at the end of a release: fetch latest.json over
        the network and verify the .sig against the pubkey already in
        tauri.conf.json. Offline, skip with a NAMED reason — never pass quietly.
      * Keep the previous DMG + manifest on the host as the documented rollback,
        and write the downgrade steps into docs/RELEASE.md.
      * Until a manifest is actually published, the updater check ships DISABLED
        rather than pointed at a 404. Say which state shipped in the PR body.

    Runs immediately after UPD-A, whose config change is inert without it.
  `,
  acceptance: `
    test -x scripts/release-local.sh
    grep -q 'set -euo pipefail' scripts/release-local.sh
    grep -q 'TAURI_SIGNING_PRIVATE_KEY' scripts/release-local.sh
    grep -q 'app.tar.gz' scripts/release-local.sh
    grep -q 'latest.json' docs/DEPLOY-SITE.md
    grep -q 'app.tar.gz' docs/DEPLOY-SITE.md
    grep -q 'rollback' docs/RELEASE.md
    test 0 -eq "$(grep -c 'TAURI_SIGNING_PRIVATE_KEY=' scripts/release-local.sh)"
    bash -n scripts/release-local.sh ; test $? -eq 0
    ./scripts/release-local.sh --verify-endpoint-or-name-the-reason ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y6-A', prompt: 'Y6', branch: 'loop/y6-a-onboarding-that-ends-in-a-working-dictation', gated: null,
  title: 'Onboarding ends with one successful pasted dictation, or it tells you exactly what is missing',
  preflight: `
    grep -q 'first_paste\\|firstPaste' desktop/src/Onboarding.tsx
    cd ${APP} && npm ci && npm test
  `,
  spec: `
    MEASURED: \`src/Onboarding.tsx:41\`
    \`STEP_ORDER = ["welcome","permissions","calibration","done"]\`. Calibration
    records a sample; nothing ever pastes. So a user finishes onboarding without
    having seen Yap's one and only trick work, and the four things that can
    break it — microphone TCC, Accessibility TCC, a missing ASR model, a missing
    paste target — each fail later, separately, with no context.

    Add a final step: TRY IT. The user clicks into a real text field inside the
    Yap window, holds the hotkey, speaks, and watches the text arrive. Then:
      * SUCCESS -> the done step, and mark \`onboarded: true\` (lib.rs:420) only
        here. Today \`onboarded\` is set without any proof the app works.
      * FAILURE -> name the stage that failed, using PERM-E's permission health
        and Y5-F's error catalogue. Four distinct dead ends, four distinct
        screens, each with the one button that fixes it:
          no mic grant -> PERM-B's denied screen
          no Accessibility grant -> the Accessibility deep link
          no model -> the model download (ModelSetup.tsx)
          paste refused -> explain the paste target and offer copy-to-clipboard
            instead (the clipboard path exists; \`auto_paste\` is a setting,
            lib.rs:405)
      * A skip is allowed and must be honest: "Skip for now" leaves
        \`onboarded\` true but raises the permission health row until it works.

    Wispr ships 92 i18n keys for this one flow ("Try it yourself", \`tiy_*\`,
    reference_wispr_parity_research §2.8 [BUNDLE], scored 🟡 for Yap as
    "calibration step"). Yap does not need 92 strings; it needs the moment.

    Also fix the ordering hazard already in the file: the model may still be
    downloading when the user reaches calibration, and the step handles it by
    WAITING with a ribbon (Onboarding.tsx:~340, YV54). Reuse that exact pattern
    for the try-it step rather than inventing a second waiting affordance.

    Tests: extract the step machine to \`desktop/src/onboarding.ts\`
    (\`nextStep(step, outcome)\`) with \`onboarding.test.ts\` covering: the happy
    path; each of the four failures routing to its own screen; skip; and
    \`onboarded_is_only_set_after_a_success_or_an_explicit_skip\`.

    Depends on PERM-B, PERM-E, Y5-F.

    What NOT to do:
      - Do NOT paste into another application during onboarding. The target is a
        field inside Yap's own window; pasting into whatever was focused before
        onboarding is a surprise and a paste-target violation (YV21).
      - Do NOT set \`onboarded: true\` on mount.
  `,
  acceptance: `
    test -f desktop/src/onboarding.ts
    test -f desktop/src/onboarding.test.ts
    grep -q 'onboarded_is_only_set_after_a_success_or_an_explicit_skip' desktop/src/onboarding.test.ts
    grep -qE '"try-it"' desktop/src/onboarding.ts
    test 4 -le "$(grep -c 'case ' desktop/src/onboarding.ts)"
    cd ${APP} && npm ci
    npm test         ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build    ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'PRIV-A', prompt: 'Y6', branch: 'loop/priv-a-crash-reporting-stays-local-and-says-so', gated: null,
  title: 'Crash reporting is local, complete and provably offline — no Sentry, no PostHog, ever',
  preflight: `
    test -f desktop/src-tauri/tests/no_outbound_on_the_dictation_path.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test no_outbound
  `,
  spec: `
    \`src-tauri/src/crash.rs\` (777 lines, YV64) is local crash capture with a
    committed fixture (tests/fixtures/crash/wilson-voice-crash.ips), and the
    support bundle has redaction tests
    (tests/support_bundle_redaction.rs, tests/support_bundle_contents.rs).
    \`git grep -i "sentry|posthog" -- desktop\` returns nothing. This item keeps
    it that way and turns the claim into a test, because it is a shipped
    marketing claim and the sharpest one Yap has.

    reference_wispr_parity_research §5.2 records the claim and the missing
    proof, verbatim: "Audio never leaves the machine ... Assert it: a test that
    fails if the dictation path opens any outbound connection (already queued as
    P3.13)". And: "No telemetry — local-only crash capture (YV64). Wispr ships
    PostHog + Sentry + Segment. Name it."

    Do:
      * \`tests/no_outbound_on_the_dictation_path.rs\`:
        - a SOURCE sweep asserting no module on the take path (record, vad,
          transcription, asr_engine, dictation, polish, polish_protocol, paste,
          paste_tx, focus, snippets, db) references an HTTP client, a socket or
          a URL literal. Pattern AND scope, both named in the test.
        - a DEPENDENCY sweep: the modules above must not import the HTTP client
          crate at all. Model-download code may; the take path may not. If the
          crate graph makes that unprovable by grep, state so and assert the
          narrower thing you CAN prove, naming the gap.
        - an assertion that the ONLY network call sites in the whole crate are
          the model download, the revocation list refresh and the updater
          check — an explicit allowlist by file and function that a new call
          site forces you to update.
      * \`crash.rs\` completeness: a crash report must never contain transcript
        text, a file path inside the user's home beyond the app's data dir, or a
        license key. Extend the redaction tests with a report synthesized to
        contain all three and assert all three are gone.
      * Nothing in crash.rs may upload. The support bundle is produced for the
        USER to send; assert there is no send path.
      * Update PRIVACY.md to state the claim in the exact words the test proves,
        and add the test's name to the doc so the claim is traceable. Do not
        soften the claim and do not overstate it: the model download, the
        revocation refresh and the updater DO talk to the network, and PRIVACY.md
        must say which three and that none of them carry audio or text.

    What NOT to do:
      - Do NOT add Sentry, PostHog, Segment or any analytics SDK.
      - Do NOT add an opt-in telemetry toggle. There is no telemetry to toggle.
      - Do NOT claim in PRIVACY.md that Yap makes no network calls at all. Three
        calls exist and naming them is what makes the rest credible.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/no_outbound_on_the_dictation_path.rs
    grep -q 'only_three_network_call_sites_exist' desktop/src-tauri/tests/no_outbound_on_the_dictation_path.rs
    grep -q 'the_take_path_has_no_url_literal' desktop/src-tauri/tests/no_outbound_on_the_dictation_path.rs
    grep -q 'no_send_path_in_crash_or_support' desktop/src-tauri/tests/no_outbound_on_the_dictation_path.rs
    grep -q 'no_outbound_on_the_dictation_path' PRIVACY.md
    test 0 -eq "$(git grep -ci 'sentry' -- desktop | wc -l)"
    test 0 -eq "$(git grep -ci 'posthog' -- desktop | wc -l)"
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test no_outbound_on_the_dictation_path ; test $? -eq 0
    cargo test --features custom-protocol --test support_bundle_redaction          ; test $? -eq 0
    cargo test --features custom-protocol --test support_bundle_contents           ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y6-B', prompt: 'Y6', branch: 'loop/y6-b-menu-bar-is-a-real-surface', gated: null,
  title: 'The menu bar becomes a usable surface: state, the last transcript, hide-for-an-hour, quit',
  preflight: `
    grep -q 'hide_for_an_hour\\|hideForAnHour' desktop/src-tauri/src/lib.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test tray_menu
  `,
  spec: `
    Yap has a tray (YV26) and \`sync_tray\` (lib.rs:4468). Wispr's status menu,
    from reference_wispr_parity_research §2.2 [BUNDLE] \`hub_status_menu_*\`:
    "Formatting options · Languages · Microphone · Paste last transcript ·
    Transcript history · Settings · Hide for 1 hour · Show all", against which
    Yap scores 🟡 "tray menu (YV26)"; and §2.2 scores "Hide the bar" 🟡.

    Ship the subset that is real today and skip the rest:
      * State header (SEC-B / Y2-E already add the license line here — build on
        it, do not duplicate).
      * Paste last transcript. The command EXISTS — \`paste_last_transcript\`
        (lib.rs:2714) with a global binding (shortcuts.rs:86) — and is not in
        the menu. One line to expose, and it is the highest-value item on the
        list.
      * Copy last transcript. Missing entirely
        (reference_wispr_parity_research §2.1 scores it ❌). Add the command and
        the menu item; it is the clipboard sibling of the paste path and must
        use the same receipt-sequenced discipline (YV39) so it cannot race
        a paste.
      * Transcript history -> focus the main window on the History view.
      * Settings -> focus the main window on Settings.
      * HIDE THE PILL FOR ONE HOUR, and "show it now". \`show_floating_pill\` is
        a persistent boolean (lib.rs:406); a temporary hide is a different thing
        and needs a deadline that survives nothing (not the setting, not a
        restart — a restart shows the pill again, which is the correct and
        forgiving behaviour). Say that in the doc comment.
      * Quit, which must run the existing exit drain (there is one — the meeting
        path has \`ABANDONED_FOR_EXIT\`, transcription.rs:84) rather than killing
        the process mid-take.

    \`sync_tray\` stays the ONLY place the tray is rebuilt. Keep
    \`tests/tray_hotkey_no_collision.rs\` green.

    Tests \`tests/tray_menu.rs\`: every menu item maps to a registered command;
    no item is unreachable; hide-for-an-hour expires; a restart during the hide
    window shows the pill; quit drains.

    Depends on Y2-E.

    What NOT to do:
      - Do NOT add Languages or Formatting submenus. Multi-language is Y10 and
        an empty submenu is worse than no submenu.
      - Do NOT make hide-for-an-hour persist across a restart.
  `,
  acceptance: `
    grep -q 'fn copy_last_transcript' desktop/src-tauri/src/lib.rs
    grep -qE 'hide_for_an_hour' desktop/src-tauri/src/lib.rs
    test -f desktop/src-tauri/tests/tray_menu.rs
    grep -q 'every_menu_item_maps_to_a_registered_command' desktop/src-tauri/tests/tray_menu.rs
    grep -q 'a_restart_during_the_hide_window_shows_the_pill' desktop/src-tauri/tests/tray_menu.rs
    grep -q 'quit_runs_the_exit_drain' desktop/src-tauri/tests/tray_menu.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test tray_menu                ; test $? -eq 0
    cargo test --features custom-protocol --test tray_hotkey_no_collision ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y6-C', prompt: 'Y6', branch: 'loop/y6-c-paste-target-and-secure-input-end-to-end', gated: null,
  title: 'The paste lands in the app you dictated into, or it does not paste — including secure-input fields',
  preflight: `
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test paste_target_e2e
  `,
  spec: `
    This is the property Yap can claim and Wispr's docs never do
    (reference_wispr_parity_research §5.2: "Paste goes only to the app you
    dictated into ... This is a security property Wispr's docs never claim").
    The machinery exists — \`focus.rs\` (342), \`paste.rs\` (557),
    \`paste_tx.rs\` (641, receipt-sequenced, YV39),
    \`secure_input.rs\` (395) — and yap8's M1 was an auto-paste-target fix. What
    is missing is one end-to-end test that the whole chain holds under the cases
    that actually happen.

    Cover, each as a named test in \`tests/paste_target_e2e.rs\`:
      * The focused app changes between the hold and the paste (the user
        cmd-tabs while Yap is transcribing). The paste must NOT go to the new
        app. It must be held, offered, or dropped with a message — pick one,
        say which in the doc comment, and make the pill say it (Y5-C's \`pasting\`
        phase and Y5-F's catalogue).
      * The focused app QUITS between the hold and the paste.
      * The target is a SECURE INPUT field (a password box).
        \`secure_input.rs\` exists for this; assert Yap refuses to paste, says
        why, and does not leave the text on the clipboard either — a password
        field's dictation sitting in the clipboard is a worse outcome than a
        refused paste.
      * Accessibility is granted but the target refuses synthesized ⌘V (some
        Electron and Java apps do). Fall back to the clipboard with an explicit
        message, never silently.
      * \`auto_paste: false\` (a real setting, lib.rs:405): the text goes to the
        clipboard and the pill says so. Assert no keystroke is synthesized.
      * A long take (Y3) whose paste arrives minutes after the hold: the target
        check must be re-run at PASTE time, not cached from hold time.
      * Two takes in quick succession cannot interleave their pastes. YV39's
        receipts exist for exactly this; assert ordering.

    Where a case cannot be driven headlessly, drive the decision function and
    say in the PR body which cases were proven by test and which by a named
    manual check with a screenshot. Do not claim a test that does not exist —
    a false capability claim is a blocking finding.

    Depends on Y5-C, Y5-F, Y3-B.

    What NOT to do:
      - Do NOT paste into a target that was not the hold target.
      - Do NOT leave dictated text on the clipboard after a refused secure-input
        paste.
      - Do NOT cache the target from hold time for a long take.
    PANEL 2026-09-12 — DECIDED, do not leave this to a doc comment. The first
    draft said the orphaned text "must be held, offered, or dropped with a
    message — pick one, say which in the doc comment". Those are three different
    products and the most consequential of the three was not in the plan's open
    decisions. It matters more after Y3-B, because the normal case becomes: the
    user stops talking, switches app while the decode runs, and the take has
    nowhere to go (paste.rs:99 \`is_same_paste_target\`, :186-192 samples the
    CURRENT frontmost app immediately before the synthesized paste, :232 "paste
    not confirmed").
    THE ANSWER IS HELD AND OFFERED. The text parks in the pill with ONE key that
    inserts it wherever the user is now, plus a History row. It is never
    silently clipboard-only behind a two-second toast, and it is never dropped —
    twelve minutes of talking is not a thing this app throws away. Name the
    test \`a_long_take_whose_target_moved_is_held_and_insertable\`, and state the
    same terminal state in Y3-B's and Y3-C's pill copy so the three agree.
    Wilson can override the choice; a builder cannot.

  `,
  acceptance: `
    test -f desktop/src-tauri/tests/paste_target_e2e.rs
    grep -q 'focus_changed_between_hold_and_paste_does_not_paste_to_the_new_app' desktop/src-tauri/tests/paste_target_e2e.rs
    grep -q 'secure_input_refuses_and_leaves_no_clipboard_residue' desktop/src-tauri/tests/paste_target_e2e.rs
    grep -q 'target_is_rechecked_at_paste_time_for_a_long_take' desktop/src-tauri/tests/paste_target_e2e.rs
    grep -q 'two_takes_cannot_interleave_their_pastes' desktop/src-tauri/tests/paste_target_e2e.rs
    grep -q 'auto_paste_false_synthesizes_no_keystroke' desktop/src-tauri/tests/paste_target_e2e.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test paste_target_e2e ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'DB-C', prompt: 'Y6', branch: 'loop/db-c-history-search-and-export-hold-up', gated: null,
  title: 'History, FTS search and export hold up at real volume, and Clear History still destroys the words',
  preflight: `
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test history_at_volume
  `,
  spec: `
    \`db.rs\` is 3,531 lines: SQLite WAL + FTS5, a dictionary, snippets, a
    scratchpad table (db.rs:648), insights rollups, and YV78's secure delete
    ("Clear history actually destroys the words — secure_delete, FTS rebuild,
    VACUUM", commit 16e2f71). History is also the surface that must keep working
    forever past the trial — \`KEEP_FOREVER_LINE\` in src/license/status.ts is a
    promise the app makes in writing, and lib.rs:1104-1106 lists history,
    search and export as things the license gate never touches.

    Prove it at volume, in \`tests/history_at_volume.rs\`:
      * Seed 10,000 takes with realistic text lengths (including one 4,000-word
        long-form take from Y3). Assert: the History view's first page query is
        bounded (a LIMIT, not a full scan); FTS search of a common word returns
        in a bounded time; and the day-series insights query does not scan the
        whole table.
      * Pagination correctness: no duplicated and no skipped row across pages
        with takes sharing a timestamp. An ORDER BY on a non-unique column is
        the classic bug here; assert a tiebreaker exists.
      * Export: full export of 10,000 rows streams rather than building one
        string in memory, and the export contains no license key and no absolute
        home path.
      * YV78 regression: after Clear History, the FTS index holds no residue for
        a word that was present, and the DB file has been VACUUMed. Assert the
        WORD is gone from the index, not merely that the row count is zero.
      * The scratchpad table (db.rs:648, :2667-2721) is covered too: it is
        user-typed text (db.rs:1191 classifies it as such) and Clear History must
        make a deliberate, documented choice about it. Say which and test it.
      * Migration idempotence is already covered
        (tests/db_migration_idempotent.rs) — keep it green and extend it to any
        migration this item adds.

    Depends on nothing in this file; can run early. Y3's long takes make the
    4,000-word case real rather than synthetic, so order it after Y3-B if the
    lane allows.

    What NOT to do:
      - Do NOT add an index without measuring. Say what each new index costs on
        insert; the take path is latency-critical (latency.rs instruments it).
      - Do NOT gate history, search or export behind the license under any
        circumstance.
      - Do NOT write transcript text into any log while testing.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/history_at_volume.rs
    grep -q 'first_page_query_is_bounded' desktop/src-tauri/tests/history_at_volume.rs
    grep -q 'pagination_has_a_tiebreaker_and_never_duplicates_a_row' desktop/src-tauri/tests/history_at_volume.rs
    grep -q 'clear_history_leaves_no_fts_residue_for_a_known_word' desktop/src-tauri/tests/history_at_volume.rs
    grep -q 'export_streams_and_leaks_no_key_or_home_path' desktop/src-tauri/tests/history_at_volume.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test history_at_volume      ; test $? -eq 0
    cargo test --features custom-protocol --test db_migration_idempotent ; test $? -eq 0
    cargo test --features custom-protocol --test license_gate            ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'PRIV-B', prompt: 'Y6', branch: 'loop/priv-b-clear-history-erases-the-audio-too', gated: null,
  title: 'Clear History erases the audio and the partial words, not only the SQLite rows',
  preflight: `
    grep -q 'recovery_dir' desktop/src-tauri/src/lib.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test erase_everything
  `,
  spec: `
    PANEL 2026-09-12 — this is a privacy regression this LOOP creates, so it
    ships inside the loop. MEASURED at 4e8c9adf:
      lib.rs:2386-2388  clear_history() is one line: state.db.clear_transcripts()
      lib.rs:565-570    recovery_dir() is "deliberately NOT the recordings dir:
                        record::sweep_stale_wavs empties that at every startup",
                        and is purged only after FAILED_TAKE_RETENTION_DAYS
      db.rs:4005        clear_history_leaves_no_plaintext_on_disk scans only
                        wilson_voice.db / -wal / -shm
    Today a dictation clip is transient. Three items in this loop convert that
    into days of retained raw audio and partial transcript text — Y3-A spills
    the take to disk, Y3-D specifies that "a cancel NEVER deletes the clip, it
    parks it in the recovery dir with the same 7-day purge lifecycle", DB-B
    persists per-take chunk TEXT — while DB-C's erase work covers the .db only.
    So after this loop a user who dictates something regrettable, cancels, and
    clicks Clear History has their rows VACUUMed and the full audio plus the
    partial words still on disk for a week, with nothing saying so.

    Do:
      * Erasure becomes one operation over the whole product: Clear History also
        deletes recovery/ WAVs, spilled take WAVs, take_chunks rows and
        orphaned meetings/ audio. Plus a separate, explicit
        "Delete all recordings" control.
      * Make the retention VISIBLE: Y3-D's and DB-B's parked clips render in
        History as "N clips kept for 7 days — review or delete". Invisible
        retained audio is the scare; visible retained audio is a feature.
      * Test on the FILESYSTEM, not the query layer: plant a sentinel WAV in
        recovery/ and a take_chunks row, run the command, assert both are gone
        from disk. Under YAP_DATA_DIR (Y0-D), never the real data dir.
      * Say in ARCHITECTURE.md what is kept, where, and for how long.

    Runs after DB-B and DB-C, whose retention this item is the counterweight to.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/erase_everything.rs
    grep -q 'a_sentinel_wav_in_recovery_is_gone_from_the_filesystem' desktop/src-tauri/tests/erase_everything.rs
    grep -q 'take_chunks_rows_are_gone_not_just_unqueryable' desktop/src-tauri/tests/erase_everything.rs
    grep -q 'orphaned_meeting_audio_is_swept' desktop/src-tauri/tests/erase_everything.rs
    grep -q 'YAP_DATA_DIR' desktop/src-tauri/tests/erase_everything.rs
    grep -rq 'kept for' desktop/src
    grep -q 'retention' ARCHITECTURE.md
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test erase_everything ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y6-D', prompt: 'Y6', branch: 'loop/y6-d-launch-sleep-wake-and-single-instance', gated: null,
  title: 'Cold launch, sleep/wake, display change and a second copy of Yap all behave',
  preflight: `
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test lifecycle_e2e
  `,
  spec: `
    The matrix tests cover these for MEETINGS and not for the app as a whole:
    \`tests/matrix_row15_single_instance.rs\`, \`matrix_row16_sleep_wake.rs\`,
    \`matrix_row14_output_device_change.rs\`, \`matrix_phase_offline.rs\`,
    \`matrix_row12_macos_144_gate.rs\`. Every one of them is an event that also
    breaks dictation, the pill and the hotkey.

    Cover, in \`tests/lifecycle_e2e.rs\`:
      * COLD LAUNCH with no model, no grants, no settings file: the app opens,
        lands on onboarding, and the pill either does not appear or appears in a
        state that explains itself (Y5-C's \`model_loading\` / PERM-C's
        \`blocked\`). It must never appear as a normal ready pill it cannot honour.
      * SLEEP/WAKE mid-take: the take is either completed or parked in recovery,
        never half-written. \`power.rs\` observes this already — assert the
        dictation path subscribes, not only the meeting path.
      * DISPLAY CHANGE / a monitor unplugged while the pill is docked to it: the
        pill must land on a visible screen, not at a negative coordinate
        off-screen. This is the classic floating-HUD bug and there is no test
        for it.
      * FULLSCREEN: ROADMAP.md records that "a normal NSWindow cannot float above
        FULLSCREEN apps" and names \`tauri-nspanel\` as the real fix. Yap sets
        \`macOSPrivateApi: true\`. Measure the current behaviour over a fullscreen
        app and write the ANSWER into the test as an assertion or into the doc
        as a named limitation with the evidence. Do not claim it works without
        measuring; do not silently leave it unknown.
      * OUTPUT DEVICE CHANGE while muted-for-dictation: YV28 snapshots and
        restores the exact prior mute state; assert a device swap mid-take does
        not leave the Mac permanently muted. That is the worst-feeling bug in
        this list.
      * SECOND INSTANCE: launching Yap twice focuses the first and exits, and
        does not open a second SQLite handle on the same WAL.
      * The four TCC grants surviving a relaunch (PERM-E's watcher) with no
        prompt storm on launch.

    Depends on PERM-C, PERM-E, Y5-C, DB-B.

    What NOT to do:
      - Do NOT claim fullscreen works without a measurement.
      - Do NOT leave the system output muted on any exit path.
      - Do NOT add tauri-nspanel in this item. Measure first; the port is its
        own item if the measurement says it is needed.
    PANEL 2026-09-12 — two corrections; without them this item lands green
    evidence for untested lifecycle behaviour, which is worse than an open gap.
    (a) DELETE "power.rs observes this already — assert the dictation path
        subscribes". It does not (power.rs:63-135 is IOPMAssertion only;
        meeting_matrix.rs:398-408 records the absent call site). Y1-B writes the
        observer and runs first; subscribe to IT.
    (b) A \`cargo test\` process has no window-server session: it cannot sleep
        the machine, unplug a monitor, launch a second copy of Yap or change a
        TCC grant. So SPLIT the seven promises by what can actually be proven:
          * State-machine level (cargo test, keep here): the sleep/wake handler's
            decision table, the display-change placement function, the
            device-swap unmute rule, the single-instance guard's logic.
          * Observed level (Y0-E's windowed smoke, under YAP_DATA_DIR): cold
            launch with nothing installed, the pill landing on a visible screen,
            fullscreen float.
          * MANUAL, and written down as a checklist in docs/RELEASE.md with a
            date and a machine: actual sleep/wake mid-take, an actual monitor
            unplug, TCC surviving a relaunch. A named manual row is honest; a
            green unit test standing in for it is not.
        Do not name a test after a behaviour the test cannot reach.

  `,
  acceptance: `
    test -f desktop/src-tauri/tests/lifecycle_e2e.rs
    grep -q 'cold_launch_with_nothing_installed_never_shows_a_ready_pill' desktop/src-tauri/tests/lifecycle_e2e.rs
    grep -q 'display_change_lands_the_pill_on_a_visible_screen' desktop/src-tauri/tests/lifecycle_e2e.rs
    grep -q 'device_swap_mid_take_never_leaves_the_mac_muted' desktop/src-tauri/tests/lifecycle_e2e.rs
    grep -q 'second_instance_focuses_the_first_and_exits' desktop/src-tauri/tests/lifecycle_e2e.rs
    grep -qE 'fullscreen' desktop/src-tauri/tests/lifecycle_e2e.rs ARCHITECTURE.md
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test lifecycle_e2e                ; test $? -eq 0
    cargo test --features custom-protocol --test matrix_row15_single_instance ; test $? -eq 0
    cargo test --features custom-protocol --test matrix_row16_sleep_wake      ; test $? -eq 0    grep -q 'LIFECYCLE MANUAL CHECKLIST' docs/RELEASE.md
    grep -q 'subscribes to power::' desktop/src-tauri/tests/lifecycle_e2e.rs

  `,
})

ITEMS.push({
  id: 'Y6-E', prompt: 'Y6', branch: 'loop/y6-e-docs-match-the-app', gated: null,
  title: 'README, ARCHITECTURE, ROADMAP and PRODUCT stop describing an app that no longer exists',
  preflight: `
    test 0 -eq "$(grep -c 'MLX Whisper' ROADMAP.md)"
    test 0 -eq "$(grep -c 'Wilson Voice' README.md)"
    grep -q 'Runtime Dependencies' ARCHITECTURE.md
  `,
  spec: `
    MEASURED. \`ROADMAP.md\` opens "Progress as of 2026-07-17" and its "What
    works today (v0.4.1)" table says: Hotkey = "Carbon ⌘⇧V hold", ASR =
    "MLX Whisper large-v3-turbo", Mic = "In-process cpal (TCC identity = Wilson
    Voice)". All three are wrong at 4e8c9adf: YV34 deleted the Python/MLX
    sidecar and made the embedded GGUF engine "the app's ONLY transcriber"
    (lib.rs:539, :1298), the default binding is \`fn⌃\` (lib.rs:404), the product
    is named Yap, and \`tauri.conf.json\` says version 0.8.0. ROADMAP's "Next
    build slices" lists as pending several things that shipped (warm daemon,
    Developer ID notarization).

    A stale ROADMAP is not cosmetic: every agent in this loop reads the repo
    docs as a spec source (docs/loop/HARNESS.md names PRODUCT.md, ROADMAP.md,
    ARCHITECTURE.md and docs/ as the spec sources), so a wrong table is a wrong
    instruction that propagates.

    Do:
      * ROADMAP.md: replace the "what works today" table with the measured truth
        at this commit, and move everything shipped into a "shipped" section
        with its YV number. Keep the research notes — the permissions and
        fullscreen notes are still accurate and load-bearing.
      * README.md: the product is Yap. Keep the bundle identifier
        \`com.wilsonguenther.wilson-voice\` and the data dir \`WilsonVoice\`
        documented as DELIBERATELY unchanged, with the reason (TCC grants and
        the SQLite history). That is the single most important sentence in the
        file for anyone who might "tidy" them.
      * ARCHITECTURE.md gains a RUNTIME DEPENDENCIES table: for each of the ASR
        model, the polish model, the yap-polish sidecar, the yap-diarize sidecar
        and the sherpa-onnx prebuilt archive — is it SHIPPED in the bundle, or
        MANAGED (downloaded+verified by the app), and where does it land on
        disk. Nothing may be listed as "assumed present on the machine". Note
        the sherpa fetch-at-build-time behaviour that .github/workflows/ci.yml
        documents at length, and the two escape hatches
        (\`SHERPA_ONNX_ARCHIVE_DIR\`, \`SHERPA_ONNX_LIB_DIR\`).
      * PRODUCT.md: one honest feature list at 0.8.0 including the notetaker and
        the license model, and the three network calls PRIV-A names.
      * A test that keeps them honest:
        \`tests/docs_match_the_app.rs\` asserting the version in ARCHITECTURE.md
        matches tauri.conf.json, the default binding named in README matches
        \`AppSettings::default().ptt_binding\`, and no doc mentions a deleted
        subsystem (MLX, the Python sidecar, \`⌘⇧V\` as the default).

    What NOT to do:
      - Do NOT rename the bundle identifier or the data directory. Document them.
      - Do NOT delete ROADMAP's research notes.
      - Do NOT write aspirational features into PRODUCT.md as shipped.
    PANEL 2026-09-12 — the docs must also answer the question the plan never
    asks: WHICH MAC IS THE WEAKEST ONE THIS MUST WORK ON. Declare it in
    ARCHITECTURE.md (chip, macOS version, RAM, free disk) and make the config
    honest about it: tauri.conf.json pins \`minimumSystemVersion: "12.0"\`,
    which invites 8 GB Intel Macs, while
    \`git grep -E 'x86_64-apple|universal-apple' -- .github desktop/package.json
    desktop/src-tauri/tauri.conf.json\` returns NOTHING and the only staged
    sidecar is aarch64-apple-darwin — so the shipped DMG cannot run on the
    machines the Info.plist invites. Building universal is out of scope for this
    loop; raising minimumSystemVersion to the arm64 reality is a one-line
    change and is Wilson's call (docs/loop/PLAN.md §4). Whichever he picks,
    ARCHITECTURE.md states the floor and Y3-G's budgets are asserted against it.

  `,
  acceptance: `
    test 0 -eq "$(grep -c 'MLX Whisper' ROADMAP.md)"
    test 0 -eq "$(grep -c 'v0.4.1' ROADMAP.md)"
    grep -q 'Runtime Dependencies' ARCHITECTURE.md
    grep -q 'SHERPA_ONNX_ARCHIVE_DIR' ARCHITECTURE.md
    grep -q 'com.wilsonguenther.wilson-voice' README.md
    grep -q 'WilsonVoice' README.md
    test -f desktop/src-tauri/tests/docs_match_the_app.rs
    grep -q 'version_in_docs_matches_tauri_conf' desktop/src-tauri/tests/docs_match_the_app.rs
    grep -q 'no_doc_mentions_a_deleted_subsystem' desktop/src-tauri/tests/docs_match_the_app.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test docs_match_the_app ; test $? -eq 0
  `,
})
