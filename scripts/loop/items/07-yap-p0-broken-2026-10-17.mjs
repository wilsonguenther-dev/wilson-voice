// yap-p0-broken-2026-10-17 — "NONE OF THE HOTKEYS ARE WORKING, PERMISSIONING, THE BRIDGES, NOTHING'S
// WORKING." Written 2026-10-17 (07:46Z) by the yap-p0-items seat from Wilson's report on Yap 0.9.0
// (the Developer-ID-signed build installed 2026-10-16). His observation is ground truth. FIX, no panel.
// One file = one lane, run in order: hotkeys first (a take must start with zero grants), then the
// truthful probe + Repair (the hotkey item's degraded UI reads it), then insertion, then the release
// smoke that would have stopped 0.9.0 shipping. Launch this file with args {lanes: 1, only:
// ['yap-p0-']} so ONE builder walks the four items in order (template.mjs, args.lanes).
// Review: Opus only (review-opus + review-opus2). No Codex, no Gemini, no third-party reviewer.
// HAZARD (same as 07/08): no agent launches the GUI app, posts events into, signals or quits Wilson's
// running /Applications/Yap.app, or writes TCC on this machine (no tccutil run by a seat, no sqlite
// writes). The APP may run `tccutil reset <Service> com.wilsonguenther.wilson-voice` for ITSELF when
// the user clicks Repair; a seat only unit-tests the command it would build. Every automated proof is a
// cargo test, a vitest, or a script self-test. SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.
//
// NO GUESSING (binds every item below): before writing code, the builder PROVES the root cause with a
// probe it ran (a cargo test that fails on main, a script self-test, or a read-only log/codesign line)
// and pastes that output into the PR body under "Root cause, proven". A PR whose body has no probe
// output is incomplete and the review seat blocks it.
//
// DIAGNOSIS (main b853b83, read-only, 2026-10-17):
//   HOTKEYS. Dictation start/stop has exactly ONE keyboard path: a listen-only CGEventTap at
//     kCGHIDEventTap (ptt_macos.rs:822-848, CGEventTapCreate at :826). That path needs Input
//     Monitoring (ListenEvent) or Accessibility, and is blind under Secure Input (secure_input.rs:1-9).
//     The Carbon RegisterEventHotKey path (tauri-plugin-global-shortcut, shortcuts.rs:23, lib.rs:6933-6971)
//     needs NO TCC grant and is immune to Secure Input, but registers only ⌃⌘V paste-last, ⌃⌘M meeting,
//     ⌃⌘Z undo, and the ⌘⇧V dictation toggle, which is OFF by default (lib.rs:498 keep_cmd_shift_v:
//     false; gated at lib.rs:6965). So with a dead grant, or with Terminal's Secure Keyboard Entry on,
//     there is no keyboard way to start or stop a take. lib.rs:6800 only LOGS "Accessibility not
//     trusted" and starts the tap anyway; lib.rs:6888 sets hotkey_registered = true unconditionally,
//     before the tap thread has even tried (a failed install at ptt_macos.rs:859-863 sets RUNNING false
//     and never retries), so the status can claim a hotkey that does not exist.
//     yap.log 2026-10-17: 07:40:34Z "Accessibility not trusted"; fn double-tap did start takes at
//     07:40:37Z and 07:44:01Z; then 6x "tap disabled by kCGEventTapDisabledByUserInput" 07:44:31Z-07:45:48Z;
//     Wilson relaunched Yap 4 times 07:43:07Z-07:43:42Z; 07:47:47Z "Secure Input ENABLED by Terminal (pid
//     1407) — the fn PTT event tap is blind". The secure_input.rs:20-27 note says no Carbon fallback is
//     possible because Carbon cannot express fn. True for fn, but it is the reason a fixed non-fn chord
//     must exist, not a reason to have none.
//   PERMISSIONS. Grants are read live: AXIsProcessTrusted (permissions.rs:129-145), IOHIDCheckAccess
//     listen (permissions.rs:407-418). CGPreflightListenEventAccess / CGPreflightPostEventAccess are not
//     called anywhere in the app (only in scripts/qa/perm-helper.swift:16 and fn-post.swift:32). Stale
//     identity is detected ONLY from a ledger of the last signature a grant read TRUE under
//     (perm_truth.rs:128-149); no stored signature -> silent. The ledger shipped in #211 (d6cd36a),
//     AFTER release 0.9.0 (ecef342), so on Wilson's Mac it cannot fire for the dev->Developer ID switch.
//     There is no repair path: tccutil appears nowhere in the tree; the banner can only open the pane
//     (permissions.rs:855-891) and tell the user to remove and re-add by hand.
//   BRIDGES. The insertion path into the focused app: paste.rs:184-190 refuses to paste when
//     AXIsProcessTrusted is false (copy only, "Enable Accessibility for Yap to auto-paste"); otherwise
//     paste_tx.rs:252 publish_and_paste puts the text on the pasteboard and paste.rs:391-442 posts ⌘V
//     with CGEventPost at the HID tap; focus.rs:56/102 read the focused element through AXUIElement.
//     Every take in the 07:40Z-07:46Z window logged pasted=false. NOTE: CGEventPost of keystrokes is
//     itself gated by the PostEvent TCC service (listed under Accessibility in Settings): fn-post.swift:32
//     already preflights CGPreflightPostEventAccess for exactly that reason. So "⌘V without Accessibility"
//     is NOT assumed to work; the probe decides, and the zero-grant floor is the clipboard + a Copy card.
//     Separately, the frontend calls the Rust side the "backend bridge": appShell.ts:351-391 boots with ONE
//     Promise.all over ~12 invokes, and any single rejection replaces the status with "Bridge error: ..."
//     (App.tsx:36-38 "UI failed to load backend bridge"). Which of the two Wilson meant is unproven; item 3
//     must probe both.
//   QA HARNESS (#210): scripts/qa/perm-probe.sh (read-only identity/signature/duplicates, --compare
//     reports dr_change), scripts/qa/hotkey-walk.sh (posts real fn edges into the installed Yap, needs
//     YAP_QA_DRIVE_RUNNING=1; --self-test posts nothing), scripts/qa/perm-helper.swift, fn-post.swift.


ITEMS.push({
  id: 'yap-p0-hotkeys-no-permission', prompt: 'yap-p0-hotkeys-no-permission', branch: 'loop/yap-p0-hotkeys-carbon-primary-zero-tcc', gated: null,
  title: 'A take starts and stops with ZERO TCC grants: a fixed-chord dictation hotkey on Carbon RegisterEventHotKey is the primary path, the fn event tap is the extra, and a dead tap is shown as a degraded mode instead of a working hotkey',
  preflight: `
    test -f desktop/src-tauri/tests/hotkey_no_tcc.rs
    grep -q 'DICTATE_TOGGLE' desktop/src-tauri/src/shortcuts.rs
    grep -q 'hotkey-walk --chord' scripts/qa/hotkey-walk.sh
  `,
  spec: `
    Panel: not required — FIX (Wilson 2026-10-17: "none of the hotkeys are working")
    DEPENDS: a green main. Blocks: yap-p0-perm-repair (it reads the degraded-mode state this adds).

    REQUIRE (before any code — NO GUESSING)
      - Write tests/hotkey_no_tcc.rs FIRST and run it on unmodified main: with every grant denied, it must
        show that no registered chord starts a take (only the tap does) and that hotkey_registered reads
        true anyway. Paste that failing output into the PR body as "Root cause, proven".
      - Quote the yap.log lines from the diagnosis above (read-only grep, never edit the log).
      - Prove the chosen chord is not a macOS system hotkey on this machine: paste
        \`defaults read com.apple.symbolichotkeys AppleSymbolicHotKeys\` evidence for it. Do not guess.

    DO
      1. shortcuts.rs: add DICTATE_TOGGLE, scope AlwaysGlobal, registered UNCONDITIONALLY at launch through
         tauri-plugin-global-shortcut (Carbon RegisterEventHotKey: no Accessibility, no Input Monitoring,
         immune to Secure Input). Default chord ⌃⌥⌘Space unless the REQUIRE probe shows a conflict (then the
         nearest free ⌃⌥⌘<key>, named in the PR). Pressed = start (or stop if a take is live). Held past
         HOLD_ARM_MS and Released = stop, so hold-to-talk works on Carbon too (ShortcutState::Released is
         delivered by the plugin). It passes tests/tray_hotkey_no_collision.rs. ⌘⇧V stays opt-in.
         Settings shows it as the "Works everywhere" hotkey and lets the user rebind it (Code + modifiers
         only; the type system already forbids modifier-only Carbon chords).
      2. The fn / fn⌃ event tap (ptt_macos.rs) stays, for the chords Carbon cannot express. It is armed only
         when the live probe says ListenEvent or Accessibility is granted; it is re-attempted on every
         permission-watch edge (no relaunch needed) instead of dying once at ptt_macos.rs:859-863.
      3. Truthful status: hotkey_registered is replaced by hotkeys: {carbon: Registered|Failed(reason),
         fnTap: Delivering|ArmedNoDelivery|Blocked(reason)|SecureInput(holder)}. "Delivering" needs a
         delivered edge, never "the thread started". lib.rs:6888's unconditional true is deleted.
      4. Degraded mode UI, explicit: when fnTap is not Delivering, the pill/home line reads
         "fn is off — <reason>. Use <DICTATE_TOGGLE label> (works without permissions)." with a Repair
         button that calls the yap-p0-perm-repair entry point (until it lands, it opens the exact pane).
         Under Secure Input the same line names the holder ("Terminal has Secure Keyboard Entry on").
      5. A registrar seam: the code that registers chords and arms the tap takes a trait object for the OS
         (Carbon registrar + permission probe), so tests drive it with grants denied and no real OS hooks.
      6. tests/hotkey_no_tcc.rs (>= 6 tests): zero grants -> DICTATE_TOGGLE registered and Pressed starts a
         take, second Pressed stops it; hold + Released stops; tap NOT armed with zero grants and status is
         Blocked, never Delivering; status never reports a hotkey that failed to register; Secure Input ->
         Carbon still starts a take, fnTap is SecureInput; a grant arriving later arms the tap with no restart.
      7. scripts/qa/hotkey-walk.sh gains \`--chord\` ("hotkey-walk --chord" in the usage text): posts the
         DICTATE_TOGGLE chord (a keyDown/keyUp with its modifiers, via fn-post.swift extended with a
         \`chord:<keycode>:<flags>\` op) and asserts "recording started" then the take-ended line; plus a
         chord fixture log, exercised by --self-test (which posts nothing). Same exit codes as today.

    WILSON'S WALK (PR body): with Yap's Accessibility and Input Monitoring rows REMOVED in Settings, press
    the chord in TextEdit: a take starts and stops; the pill names the degraded fn mode.
    YAP_QA_DRIVE_RUNNING=1 bash scripts/qa/hotkey-walk.sh --chord when he is not dictating.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/hotkey_no_tcc.rs
    grep -q 'DICTATE_TOGGLE' desktop/src-tauri/src/shortcuts.rs
    grep -q 'hotkey-walk --chord' scripts/qa/hotkey-walk.sh
    ! grep -q 'hotkey_registered.lock() = true' desktop/src-tauri/src/lib.rs
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    (cd desktop/src-tauri && cargo test --features custom-protocol --test hotkey_no_tcc)
    test "$(cd desktop/src-tauri && cargo test --features custom-protocol --test hotkey_no_tcc -- --list 2>/dev/null | grep -c ': test$')" -ge 6
    (cd desktop/src-tauri && cargo test --features custom-protocol --test tray_hotkey_no_collision)
    (cd desktop/src-tauri && cargo test --features custom-protocol --test hotkey_stop)
    bash scripts/qa/hotkey-walk.sh --self-test
    shellcheck scripts/qa/hotkey-walk.sh
  `,
})

ITEMS.push({
  id: 'yap-p0-perm-repair', prompt: 'yap-p0-perm-repair', branch: 'loop/yap-p0-perm-repair-truthful-probe-one-click', gated: null,
  title: 'One truthful grant probe per service (live API, never the Settings list), a detected "stale identity" state, and a one-click Repair: tccutil reset for Yap\'s own bundle id, open the exact pane, re-probe until granted',
  preflight: `
    test -f desktop/src-tauri/tests/perm_repair.rs
    grep -q 'StaleIdentity' desktop/src-tauri/src/perm_truth.rs
    grep -q 'repair-plan' scripts/qa/perm-probe.sh
  `,
  spec: `
    Panel: not required — FIX (Wilson 2026-10-17: "permissioning ... nothing's working")
    DEPENDS: yap-p0-hotkeys-no-permission (its degraded-mode Repair button calls this).

    REQUIRE (before any code — NO GUESSING)
      - Run on unmodified main: bash scripts/qa/perm-probe.sh (read-only) and paste authority_leaf, cdhash,
        designated_requirement, helper_* lines. Paste a cargo test that FAILS on main showing that with no
        ledger entry (Wilson's real state: #211 shipped after 0.9.0) and a denied probe, stale_grants() is
        empty — the stale state is invisible today.
      - Confirm the tccutil service names from \`tccutil\` usage / man page on this macOS (Accessibility,
        ListenEvent, PostEvent, Microphone) and paste them. Never run tccutil yourself (HAZARD).

    DO
      1. One probe module, one function per service, each a LIVE API call: Accessibility =
         AXIsProcessTrusted; ListenEvent = CGPreflightListenEventAccess AND IOHIDCheckAccess (record both,
         disagreement is reported); PostEvent = CGPreflightPostEventAccess; Microphone = AVCaptureDevice
         authorizationStatus. Nothing reads TCC.db or the Settings list. The banner, onboarding, the pill's
         degraded line and the insertion path all call THIS probe; no second copy of a grant check
         survives (grep proves it).
      2. Grant state per service: Granted | Denied | NotDetermined | StaleIdentity | Unknown. StaleIdentity
         = probe denied AND (ledger says it was granted under a different signature OR the last-run
         signature persisted at EVERY launch differs from the running one in authority or team). Persist
         last_run_signature every launch (not only on a true read) so the next identity change is caught
         even when no grant ever read true. Pure decision function.
      3. Repair (one click, per service, from banner + onboarding + degraded pill): runs
         \`/usr/bin/tccutil reset <Service> com.wilsonguenther.wilson-voice\` for Yap's OWN bundle id
         (read from NSBundle.mainBundle at runtime, never a literal elsewhere; never \`tccutil reset All\`,
         never another bundle id), then for Accessibility calls AXIsProcessTrustedWithOptions(prompt) and
         for ListenEvent IOHIDRequestAccess, opens the exact pane
         (x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility /
         ?Privacy_ListenEvent / ?Privacy_Microphone, via the existing open_privacy_pane allow-list), and
         re-probes every 1 s for 120 s, showing "Waiting for you to switch Yap on…" then "Fixed" the moment
         the probe flips. If tccutil exits non-zero, the UI shows its stderr and the manual steps
         (select Yap, minus, plus, choose /Applications/Yap.app). The repair command is built by a pure
         function so tests assert the exact argv.
      4. scripts/qa/perm-probe.sh gains \`--repair-plan\`: prints repair_cmd_<service>= lines (the exact argv
         the app would run) and repair_pane_<service>= URLs for the probed app's bundle id, executes
         nothing; --self-test covers it (Calculator's id).
      5. tests/perm_repair.rs (>= 8 tests): the probe matrix (each service Granted/Denied x ledger
         none/same/different x last-run same/different) maps to the right state; the stale state with NO
         ledger and a changed last-run signature is StaleIdentity (the 0.9.0 case); repair argv is exactly
         [/usr/bin/tccutil, reset, <Service>, <own bundle id>] and never contains "All"; pane URL per service;
         re-probe loop stops on Granted and times out at 120 s; banner/onboarding share one probe.
         desktop/src/permission.repair.test.ts (>= 3 vitest cases): StaleIdentity copy, Repair button
         present for Denied and StaleIdentity, "Fixed" after the probe flips.

    WILSON'S WALK (PR body): open Yap; the banner says "macOS lists Yap as allowed, but that permission
    belongs to an older Yap" for Accessibility and Input Monitoring; click Repair, flip Yap on in the
    pane that opens; the banner turns to Fixed without a relaunch.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/perm_repair.rs
    test -f desktop/src/permission.repair.test.ts
    grep -q 'StaleIdentity' desktop/src-tauri/src/perm_truth.rs
    grep -q 'CGPreflightListenEventAccess' desktop/src-tauri/src/permissions.rs
    grep -q 'CGPreflightPostEventAccess' desktop/src-tauri/src/permissions.rs
    grep -q '/usr/bin/tccutil' desktop/src-tauri/src/permissions.rs
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    (cd desktop && npx vitest run src/permission.repair.test.ts)
    (cd desktop/src-tauri && cargo test --features custom-protocol --test perm_repair)
    test "$(cd desktop/src-tauri && cargo test --features custom-protocol --test perm_repair -- --list 2>/dev/null | grep -c ': test$')" -ge 8
    (cd desktop/src-tauri && cargo test --features custom-protocol --test perm_truth)
    bash scripts/qa/perm-probe.sh --self-test
    mkdir -p desktop/target && bash scripts/qa/perm-probe.sh --app /System/Applications/Calculator.app --repair-plan > desktop/target/qa-repair-plan.txt
    grep -q '^repair_cmd_accessibility=/usr/bin/tccutil reset Accessibility com.apple.calculator$' desktop/target/qa-repair-plan.txt
    grep -q '^repair_cmd_listenevent=/usr/bin/tccutil reset ListenEvent com.apple.calculator$' desktop/target/qa-repair-plan.txt
    grep -q '^repair_pane_accessibility=x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility$' desktop/target/qa-repair-plan.txt
    shellcheck scripts/qa/perm-probe.sh
  `,
})

ITEMS.push({
  id: 'yap-p0-insertion-bridge-fallback', prompt: 'yap-p0-insertion-bridge-fallback', branch: 'loop/yap-p0-insertion-fallback-copy-card', gated: null,
  title: 'Insertion never silently fails: AX insertion only when the probe says trusted, a clipboard + ⌘V bridge when PostEvent is granted, an instant Copy card when neither is, and one failing backend call can no longer blank the app as "Bridge error"',
  preflight: `
    test -f desktop/src-tauri/tests/insertion_matrix.rs
    grep -q 'InsertPath' desktop/src-tauri/src/paste.rs
    grep -q 'allSettled' desktop/src/appShell.ts
  `,
  spec: `
    Panel: not required — FIX (Wilson 2026-10-17: "the bridges ... nothing's working")
    DEPENDS: yap-p0-perm-repair (uses its single probe; never a second grant check).

    REQUIRE (before any code — NO GUESSING)
      - Establish which "bridge" is broken, with output, not opinion: (a) read-only grep of yap.log for
        pasted=false / "paste not confirmed" / "Enable Accessibility" since 2026-10-16 and paste the counts;
        (b) a vitest on unmodified main that makes ONE of the boot invokes in appShell.ts reject and shows
        the whole app replaced by "Bridge error". Both go into the PR body.
      - Prove with a probe whether CGEventPost(⌘V) is delivered without Accessibility on this macOS: build a
        tiny ad hoc Swift helper (scripts/qa/, like fn-post.swift) that prints post_event_access=<CGPreflightPostEventAccess()>
        and, only with --post, posts ⌘V into a TextEdit document IT created. Do not assume "keyDown needs no
        AX" — fn-post.swift:32 already shows PostEvent is gated. The design below follows what the probe shows.

    DO
      1. paste.rs: one InsertPath decision, pure, from the probe: Ax (Accessibility trusted: AX
         kAXSelectedText set on the focused element, falling back to the pasteboard bridge when the element
         refuses), PasteBridge (PostEvent granted: publish to the pasteboard + CGEventPost ⌘V, receipt-checked
         as today by paste_tx), CopyCard (neither: no keystroke is attempted). paste.rs:184's
         "is_accessibility_trusted() -> copy only" becomes this decision.
      2. CopyCard: the transcript appears in the pill within 100 ms of the transcript being ready, with a
         Copy button and the line "Copied — press ⌘V. Repair to auto-insert." (Repair = yap-p0-perm-repair).
         The text is already on the clipboard, so ⌘V by hand works; the card stays until dismissed or the
         next take. A PasteBridge with no receipt (paste_tx RECEIPT_WINDOW) also lands on the CopyCard,
         never on a silent pasted=false.
      3. The backend-bridge boot: appShell.ts loads with Promise.allSettled; each failed command shows its
         own inline error and retry in the view that needs it; the status line is never replaced by
         "Bridge error" while any command succeeded. App.tsx's full-screen "UI failed to load backend bridge"
         only when get_settings itself fails.
      4. tests/insertion_matrix.rs (>= 6 tests): AX trusted -> Ax; AX dead + PostEvent granted -> PasteBridge;
         none -> CopyCard and no CGEventPost is attempted (a counting fake poster); PasteBridge no receipt ->
         CopyCard; wrong-target guard (paste.rs is_same_paste_target) still copies only; stale take still
         copies only. desktop/src/appShell.boot.test.ts (>= 2 vitest cases): one rejected invoke leaves the
         rest of the app rendered; get_settings rejection shows the full-screen retry.

    WILSON'S WALK (PR body): with Accessibility removed, dictate into TextEdit: the Copy card shows the
    text at once and ⌘V pastes it; after Repair, the same take inserts by itself.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/insertion_matrix.rs
    test -f desktop/src/appShell.boot.test.ts
    grep -q 'InsertPath' desktop/src-tauri/src/paste.rs
    grep -q 'allSettled' desktop/src/appShell.ts
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    (cd desktop && npx vitest run src/appShell.boot.test.ts)
    (cd desktop/src-tauri && cargo test --features custom-protocol --test insertion_matrix)
    test "$(cd desktop/src-tauri && cargo test --features custom-protocol --test insertion_matrix -- --list 2>/dev/null | grep -c ': test$')" -ge 6
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib paste)
    bash scripts/qa/hotkey-walk.sh --self-test
  `,
})

ITEMS.push({
  id: 'yap-p0-release-smoke', prompt: 'yap-p0-release-smoke', branch: 'loop/yap-p0-release-smoke-signed-app-gate', gated: null,
  title: 'Every DMG build runs a signed-app smoke (launch, probe every service, register hotkeys, insert into TextEdit) and the release FAILS when any step fails',
  preflight: `
    test -x scripts/qa/release-smoke.sh
    grep -q 'release-smoke.sh' scripts/sign-local.sh
    grep -q 'release-smoke.sh' docs/RELEASING.md
  `,
  spec: `
    Panel: not required — FIX (0.9.0 shipped with every TCC grant dead and nothing noticed)
    DEPENDS: yap-p0-hotkeys-no-permission, yap-p0-perm-repair, yap-p0-insertion-bridge-fallback (the smoke
      asserts their log lines; if one is not on main when you start, rebase after it lands).

    REQUIRE (before any code — NO GUESSING)
      - Read scripts/sign-local.sh --tauri-build and docs/RELEASING.md and paste the exact step where the
        .app/.dmg is produced; the smoke hooks in right after it. Paste perm-probe.sh --compare output
        for the built app vs /Applications/Yap.app (dr_change=yes|no) — the 0.9.0 signal.

    DO
      1. scripts/qa/release-smoke.sh --app PATH [--force-fail STEP] [--self-test]. Steps, in order, each
         with its own exit code and a printed step=<name> ok|fail line: launch (open -n the built app with a
         temp data dir and a --release-smoke flag; refuse with exit 2 when another Yap with the same bundle id
         is running — never signal it), probe (the app logs one line per service from the yap-p0-perm-repair
         probe; all four present), hotkeys (the app logs DICTATE_TOGGLE registered from the Carbon path),
         insert (headless AppleScript: create a TextEdit document, the app inserts a fixed smoke string through
         its insertion path or shows the CopyCard, the script reads the document text back and accepts either
         the string in the document or the CopyCard log line), quit (the smoke instance exits cleanly within
         10 s). --force-fail STEP makes that step fail on purpose; the script must exit non-zero.
         --self-test runs the parser and every refusal and force-fail path against fixture logs and
         launches nothing.
      2. The --release-smoke flag in the app: registers hotkeys and allows ONE insertion into the document
         the smoke created (frontmost must be that TextEdit document, checked first), logs the probe lines,
         then quits. Distinct from --smoke (which refuses hotkeys and paste) and covered by a unit test that
         it never pastes when the frontmost app is not TextEdit.
      3. scripts/sign-local.sh --tauri-build runs release-smoke.sh on the built app and exits non-zero if it
         fails (bare exit propagation, no pipe in the exit path). docs/RELEASING.md: the smoke is mandatory,
         and dr_change=yes goes into the release notes with the Repair instructions.
      4. This seat never runs the full smoke on Wilson's machine (HAZARD); only --self-test and the
         force-fail checks below. The full run is the release host's step, listed in the PR body.
  `,
  acceptance: `
    test -x scripts/qa/release-smoke.sh
    grep -q 'release-smoke.sh' scripts/sign-local.sh
    grep -q 'release-smoke.sh' docs/RELEASING.md
    bash scripts/qa/release-smoke.sh --self-test
    ! bash scripts/qa/release-smoke.sh --self-test --force-fail probe
    ! bash scripts/qa/release-smoke.sh --self-test --force-fail hotkeys
    ! bash scripts/qa/release-smoke.sh --self-test --force-fail insert
    shellcheck scripts/qa/release-smoke.sh
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib smoke)
  `,
})
