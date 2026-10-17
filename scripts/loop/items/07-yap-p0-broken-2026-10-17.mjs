// yap-p0-broken-2026-10-17 — "NONE OF THE HOTKEYS ARE WORKING, PERMISSIONING, THE BRIDGES, NOTHING'S
// WORKING." Written 2026-10-17 (07:46Z) by the yap-p0-items seat from Wilson's report on Yap 0.9.0
// (the Developer-ID-signed build installed 2026-10-16). His observation is ground truth. FIX, no panel.
// One file = one lane, run in order: hotkeys first (a take must start with zero grants), then the
// truthful probe + Repair (the hotkey item's degraded UI calls it), then insertion, then the release
// smoke that would have stopped 0.9.0 shipping.
//
// LAUNCH SEQUENCE (fix round 2026-10-17, second reviewer B1). Each item builds on the previous item's
// code, every builder starts from origin/main, and build mode never lands an unreviewed PR. So items
// 2-4 carry a DEPENDENCY preflight: when the previous item's symbols are not on main, the preflight
// prints DEPENDENCY-NOT-ON-MAIN and exits 0, and the harness records "skipped: dependency not on main"
// (template.mjs runBuild) with no builder and no PR. The operator alternates, one item per round:
//   build {mode:'build', lanes:1, only:['yap-p0-']} -> review {mode:'review'} (lands item 1)
//   -> build (item 1 already-done, item 2 builds) -> review -> build (item 3) -> review -> build (item 4)
//   -> review. Stop when all four report already-done. Never build two of these in one pass.
//
// Review: Opus only (review-opus + review-opus2). No Codex, no Gemini, no third-party reviewer.
// HAZARD (same as 07/08): no agent launches the GUI app, posts events into, signals or quits Wilson's
// running /Applications/Yap.app, or writes TCC on this machine (no tccutil run by a seat, no sqlite
// writes). The APP may run `tccutil reset <Service> com.wilsonguenther.wilson-voice` for ITSELF when
// the user clicks Repair; a seat only unit-tests the argv it would build. Every automated proof is a
// cargo test, a vitest, or a script self-test. SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.
//
// NO GUESSING (binds every item below): before writing code, the builder PROVES the root cause with a
// probe it ran (a cargo test that fails on main, a script self-test, or a read-only log/codesign line)
// and pastes that output into the PR body under "Root cause, proven". A PR whose body has no probe
// output is incomplete and the review seat blocks it.
//
// EVIDENCE FILE (first reviewer advisory): the "Root cause, proven" probe output ALSO goes into
// docs/loop/evidence/<item id>.md, first line exactly "ROOT-CAUSE-PROBE <item id>", then the raw output.
// Each acceptance checks the file is non-empty and carries that item's probe marker lines.
//
// SYMBOLS ARE CONTRACT. The function, module, event and test names in each spec are what the later
// items' dependency preflights and this item's acceptance grep for. Do not rename them.
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
//     1407) — the fn PTT event tap is blind". secure_input.rs:20-27 says no Carbon fallback is possible
//     because Carbon cannot express fn. True for fn, but it is the reason a fixed non-fn chord must
//     exist, not a reason to have none.
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
    grep -q 'DICTATE_TOGGLE.shortcut()' desktop/src-tauri/src/lib.rs
    grep -q 'HotkeySource::Carbon' desktop/src-tauri/src/lib.rs
    grep -q 'set_fn_tap_status(' desktop/src-tauri/src/ptt_macos.rs
    grep -q 'hotkey-walk --chord' scripts/qa/hotkey-walk.sh
  `,
  spec: `
    Panel: not required — FIX (Wilson 2026-10-17: "none of the hotkeys are working")
    DEPENDS: a green main. Blocks: yap-p0-perm-repair (its dependency preflight greps this item's
      DICTATE_TOGGLE.shortcut() registration and HotkeySource::Carbon dispatch in lib.rs).

    REQUIRE (before any code — NO GUESSING)
      - Write tests/hotkey_no_tcc.rs FIRST and run it on unmodified main: with every grant denied, it must
        show that no registered chord starts a take (only the tap does) and that hotkey_registered reads
        true anyway. Paste that failing output into the PR body as "Root cause, proven" AND into
        docs/loop/evidence/yap-p0-hotkeys-no-permission.md (see EVIDENCE FILE above).
      - Quote the yap.log lines from the diagnosis above (read-only grep, never edit the log).
      - Prove the chosen chord is not a macOS system hotkey on this machine: paste
        \`defaults read com.apple.symbolichotkeys AppleSymbolicHotKeys\` evidence for it. Do not guess.

    DO (names are contract)
      1. shortcuts.rs: add DICTATE_TOGGLE, scope AlwaysGlobal, registered UNCONDITIONALLY at launch in
         lib.rs with h.global_shortcut().register(shortcuts::DICTATE_TOGGLE.shortcut()) (Carbon
         RegisterEventHotKey: no Accessibility, no Input Monitoring, immune to Secure Input). Default chord
         ⌃⌥⌘Space unless the REQUIRE probe shows a conflict (then the nearest free ⌃⌥⌘<key>, named in the
         PR). It passes tests/tray_hotkey_no_collision.rs. ⌘⇧V stays opt-in. Settings shows it as the
         "Works everywhere" hotkey and lets the user rebind it (Code + modifiers only).
      2. ONE dispatcher for every hotkey edge: fn take_hotkey_edge(app, state, edge: HotkeyEdge,
         source: HotkeySource) in lib.rs, with HotkeySource::{Carbon, FnTap} and HotkeyEdge::{Press,
         Release}. The global-shortcut handler's DICTATE_TOGGLE branch calls
         take_hotkey_edge(.., HotkeySource::Carbon); the ptt_macos PttEvent::Start / PttEvent::Stop arms
         at lib.rs ~6827/6856 call take_hotkey_edge(.., HotkeySource::FnTap). Press = start (or stop if a
         take is live); a press held past ptt_macos::HOLD_ARM_MS then Release = stop, so hold-to-talk works
         on Carbon too (ShortcutState::Released). No second start/stop code path survives.
      3. The fn / fn⌃ event tap stays, for chords Carbon cannot express, and is ALWAYS ATTEMPTED: at
         launch and again on every permission-watch edge, never gated on the probe (yap.log 07:40:34Z-
         07:40:39Z: fn started a take while AXIsProcessTrusted said "not trusted"). The probe drives only
         status and UI. A tap that failed to install is retried on the next edge instead of dying once at
         ptt_macos.rs:859-863. With valid grants, fn start/stop behaves exactly as on main.
         Carbon chord + fn must never double-start: take_hotkey_edge ignores a Press from one source while
         a take started by the other is live, except as a stop.
      4. Truthful status: hotkey_registered and its unconditional true at lib.rs:6888 are deleted. AppStatus
         carries hotkeys: {carbon: Registered|Failed(reason), fnTap: Delivering|ArmedNoDelivery|
         Blocked(reason)|SecureInput(holder)}. Blocked means "attempted, nothing delivered", never "not tried". set_carbon_status(..) is called in lib.rs with the Ok/Err of
         each DICTATE_TOGGLE register call; set_fn_tap_status(..) is called in ptt_macos.rs from run_tap's
         install_tap result, from recreate_tap, and from the first delivered edge. "Delivering" needs a
         delivered edge, never "the thread started".
      5. Degraded mode UI, explicit: when fnTap is not Delivering, the pill/home line reads
         "fn is off — <reason>. Use <DICTATE_TOGGLE label> (works without permissions)." with a Repair
         button (until yap-p0-perm-repair lands it opens the exact pane). Under Secure Input the line
         names the holder ("Terminal has Secure Keyboard Entry on").
      6. A registrar seam: the registration + arming code takes trait objects for the OS (Carbon registrar,
         permission probe, event source), so tests drive it with grants denied and no real OS hooks.
      7. tests/hotkey_no_tcc.rs, these exact test names, each with real assertions (>= 12 assert! /
         assert_eq! in the file in total): zero_grants_dictate_toggle_starts_and_stops,
         zero_grants_hold_release_stops_take, zero_grants_fn_tap_attempted_status_blocked,
         failed_registration_never_reported_registered, secure_input_carbon_still_starts_take,
         grant_arriving_later_arms_tap_without_restart, valid_grants_fn_start_stop_unchanged,
         probe_denied_but_tap_delivering_stays_armed, carbon_and_fn_never_double_start (>= 18 asserts in
         total with these). zero_grants_dictate_toggle_starts_and_stops
         prints exactly one line "hotkey_no_tcc: grants=none source=carbon started=true stopped=true"
         (println!, after its asserts) — acceptance greps it under --nocapture.
      8. scripts/qa/hotkey-walk.sh gains \`--chord\` ("hotkey-walk --chord" in the usage text): posts the
         DICTATE_TOGGLE chord (fn-post.swift gains a \`chord:<keycode>:<flags>\` op) and asserts
         "recording started" then the take-ended line; plus scripts/qa/fixtures/yap-walk-chord-pass.log,
         exercised by --self-test (which posts nothing). Same exit codes as today.

    WILSON'S WALK (PR body): with Yap's Accessibility and Input Monitoring rows REMOVED in Settings, press
    the chord in TextEdit: a take starts and stops; the pill names the degraded fn mode.
    YAP_QA_DRIVE_RUNNING=1 bash scripts/qa/hotkey-walk.sh --chord when he is not dictating.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/hotkey_no_tcc.rs
    test -f scripts/qa/fixtures/yap-walk-chord-pass.log
    grep -q 'pub const DICTATE_TOGGLE' desktop/src-tauri/src/shortcuts.rs
    grep -q 'DICTATE_TOGGLE.shortcut()' desktop/src-tauri/src/lib.rs
    grep -q 'HotkeySource::Carbon' desktop/src-tauri/src/lib.rs
    grep -q 'HotkeySource::FnTap' desktop/src-tauri/src/lib.rs
    test "$(grep -c 'take_hotkey_edge(' desktop/src-tauri/src/lib.rs)" -ge 4
    ! grep -q 'hotkey_registered' desktop/src-tauri/src/lib.rs
    grep -q 'set_carbon_status(' desktop/src-tauri/src/lib.rs
    test "$(grep -c 'set_fn_tap_status(' desktop/src-tauri/src/ptt_macos.rs)" -ge 3
    grep -q 'hotkey-walk --chord' scripts/qa/hotkey-walk.sh
    test "$(grep -cE 'assert(_eq|_ne)?!' desktop/src-tauri/tests/hotkey_no_tcc.rs)" -ge 18
    test -s docs/loop/evidence/yap-p0-hotkeys-no-permission.md
    grep -q '^ROOT-CAUSE-PROBE yap-p0-hotkeys-no-permission$' docs/loop/evidence/yap-p0-hotkeys-no-permission.md
    grep -q 'kCGEventTapDisabledByUserInput' docs/loop/evidence/yap-p0-hotkeys-no-permission.md
    grep -q 'AppleSymbolicHotKeys' docs/loop/evidence/yap-p0-hotkeys-no-permission.md
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    mkdir -p desktop/target
    (cd desktop/src-tauri && cargo test --features custom-protocol --test hotkey_no_tcc -- --list) > desktop/target/p0-hk-list.txt 2>/dev/null
    grep -q '^zero_grants_dictate_toggle_starts_and_stops: test$' desktop/target/p0-hk-list.txt
    grep -q '^zero_grants_hold_release_stops_take: test$' desktop/target/p0-hk-list.txt
    grep -q '^zero_grants_fn_tap_attempted_status_blocked: test$' desktop/target/p0-hk-list.txt
    grep -q '^failed_registration_never_reported_registered: test$' desktop/target/p0-hk-list.txt
    grep -q '^secure_input_carbon_still_starts_take: test$' desktop/target/p0-hk-list.txt
    grep -q '^grant_arriving_later_arms_tap_without_restart: test$' desktop/target/p0-hk-list.txt
    grep -q '^valid_grants_fn_start_stop_unchanged: test$' desktop/target/p0-hk-list.txt
    grep -q '^probe_denied_but_tap_delivering_stays_armed: test$' desktop/target/p0-hk-list.txt
    grep -q '^carbon_and_fn_never_double_start: test$' desktop/target/p0-hk-list.txt
    (cd desktop/src-tauri && cargo test --features custom-protocol --test hotkey_no_tcc -- --exact zero_grants_dictate_toggle_starts_and_stops --nocapture) > desktop/target/p0-hk-crux.txt 2>&1
    grep -q '^hotkey_no_tcc: grants=none source=carbon started=true stopped=true$' desktop/target/p0-hk-crux.txt
    grep -q 'test result: ok. 1 passed' desktop/target/p0-hk-crux.txt
    (cd desktop/src-tauri && cargo test --features custom-protocol --test hotkey_no_tcc)
    (cd desktop/src-tauri && cargo test --features custom-protocol --test tray_hotkey_no_collision)
    (cd desktop/src-tauri && cargo test --features custom-protocol --test hotkey_stop)
    (cd desktop/src-tauri && cargo test --features custom-protocol --test matrix_row1_tap_permission_denied)
    bash scripts/qa/hotkey-walk.sh --self-test
    shellcheck scripts/qa/hotkey-walk.sh
  `,
})

ITEMS.push({
  id: 'yap-p0-perm-repair', prompt: 'yap-p0-perm-repair', branch: 'loop/yap-p0-perm-repair-truthful-probe-one-click', gated: null,
  title: 'One truthful grant probe per service (live API, never the Settings list) in perm_probe.rs, a detected "stale identity" state, and a one-click Repair: tccutil reset for Yap\'s own bundle id, open the exact pane, re-probe until granted',
  preflight: `
    ( M=0; test -f desktop/src-tauri/tests/hotkey_no_tcc.rs || M=1; grep -q 'DICTATE_TOGGLE.shortcut()' desktop/src-tauri/src/lib.rs || M=1; grep -q 'HotkeySource::Carbon' desktop/src-tauri/src/lib.rs || M=1; if [ "$M" = 1 ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-p0-hotkeys-no-permission has not landed (no DICTATE_TOGGLE registration / HotkeySource::Carbon in lib.rs) — run the review pass to land it, then the build pass again"; exit 0; fi; test -f desktop/src-tauri/src/perm_probe.rs && grep -q 'StaleIdentity' desktop/src-tauri/src/perm_probe.rs && grep -q 'pub fn repair_argv' desktop/src-tauri/src/perm_probe.rs && grep -q 'repair-plan' scripts/qa/perm-probe.sh )
  `,
  spec: `
    Panel: not required — FIX (Wilson 2026-10-17: "permissioning ... nothing's working")
    DEPENDS: yap-p0-hotkeys-no-permission ON MAIN (the preflight skips this item with "dependency not on
      main" until it is). Blocks: yap-p0-insertion-bridge-fallback (its preflight greps perm_probe.rs
      pub fn probe_all and the StaleIdentity state).

    REQUIRE (before any code — NO GUESSING)
      - Run on unmodified main: bash scripts/qa/perm-probe.sh (read-only) and paste authority_leaf, cdhash,
        designated_requirement, helper_* lines. Paste a cargo test that FAILS on main showing that with no
        ledger entry (Wilson's real state: #211 shipped after 0.9.0) and a denied probe, stale_grants() is
        empty — the stale state is invisible today.
      - Paste all of it into docs/loop/evidence/yap-p0-perm-repair.md too (EVIDENCE FILE above).
      - Confirm the tccutil service names from \`tccutil\` usage / man page on this macOS (Accessibility,
        ListenEvent, PostEvent, Microphone) and paste them. Never run tccutil yourself (HAZARD).

    DO (names are contract)
      1. NEW module desktop/src-tauri/src/perm_probe.rs (declared in lib.rs). One function per service,
         each a LIVE API call: Accessibility = AXIsProcessTrusted; ListenEvent = CGPreflightListenEventAccess
         AND IOHIDCheckAccess (both recorded, disagreement reported); PostEvent = CGPreflightPostEventAccess;
         Microphone = AVCaptureDevice authorizationStatus. pub fn probe_all() -> ProbeReport. Nothing reads
         TCC.db or the Settings list. The ONE extern declaration of AXIsProcessTrusted lives here;
         permissions.rs report(), the banner, onboarding, the hotkey item's degraded line and (item 3)
         paste.rs all read perm_probe — permissions.rs keeps no grant FFI of its own.
      2. ProbeState per service: Granted | Denied | NotDetermined | StaleIdentity | Unknown. StaleIdentity
         = probe denied AND (perm_truth ledger says it was granted under a different signature OR the
         last-run signature, persisted at EVERY launch, differs from the running one in authority or team).
         pub fn classify(..) is pure. Persist last_run_signature every launch so the next identity change is
         caught even when no grant ever read true.
      3. Repair: a Tauri command repair_permission(service) (in the invoke handler list) reachable from the
         banner, onboarding and the degraded pill. pub fn repair_argv(service, bundle_id) -> Vec<String>
         is pure and returns exactly ["/usr/bin/tccutil", "reset", <Service>, <bundle id>], the bundle id
         read from NSBundle.mainBundle at runtime; never "All", never another bundle id. Repairing
         Accessibility ALSO resets PostEvent (the ⌘V insertion bridge depends on it). Then
         AXIsProcessTrustedWithOptions(prompt) for Accessibility / IOHIDRequestAccess for ListenEvent, open
         the exact pane through the existing open_privacy_pane allow-list
         (x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility / ?Privacy_ListenEvent
         / ?Privacy_Microphone), and re-probe every 1 s for 120 s: "Waiting for you to switch Yap on…", then
         "Fixed" the moment probe_all flips — for Accessibility, PostEvent and Microphone. Input Monitoring
         usually only takes effect after a quit and reopen, so for ListenEvent the flow says "Granted —
         quit and reopen Yap to finish" with a Relaunch button (app.restart()); never claim it works
         without a relaunch. tccutil non-zero -> show its stderr + the manual steps.
      4. scripts/qa/perm-probe.sh gains \`--repair-plan\`: prints repair_cmd_<service>= (the exact argv the
         app would run, space-joined) and repair_pane_<service>= lines for the probed app's bundle id,
         executes nothing; --self-test covers it (Calculator's id).
      5. tests/perm_repair.rs, these exact names, real assertions (>= 16 assert! / assert_eq! in total):
         probe_matrix_maps_every_combination (each service Granted/Denied x ledger none/same/different x
         last-run same/different), stale_identity_without_ledger_after_signature_change (the 0.9.0 case;
         prints "perm_repair: ledger=none last_run=changed probe=denied state=StaleIdentity"),
         stale_identity_from_ledger_different_signature, repair_argv_is_own_bundle_id_never_all,
         repair_pane_url_per_service, reprobe_stops_on_granted, reprobe_times_out_at_120s,
         banner_and_onboarding_share_one_probe, accessibility_repair_also_resets_post_event,
         listen_event_repair_says_relaunch (>= 20 asserts in total with these).
         desktop/src/permission.repair.test.ts (>= 3 vitest cases):
         StaleIdentity copy, Repair button for Denied and StaleIdentity calls invoke("repair_permission"),
         "Fixed" after the probe flips.

    WILSON'S WALK (PR body): open Yap; the banner says "macOS lists Yap as allowed, but that permission
    belongs to an older Yap" for Accessibility and Input Monitoring; click Repair, flip Yap on in the
    pane that opens; Accessibility turns to Fixed; Input Monitoring asks for one Relaunch, after which
    fn works.
  `,
  acceptance: `
    test -f desktop/src-tauri/src/perm_probe.rs
    test -f desktop/src-tauri/tests/perm_repair.rs
    test -f desktop/src/permission.repair.test.ts
    grep -q 'mod perm_probe' desktop/src-tauri/src/lib.rs
    grep -q 'StaleIdentity' desktop/src-tauri/src/perm_probe.rs
    grep -q 'pub fn probe_all' desktop/src-tauri/src/perm_probe.rs
    grep -q 'pub fn repair_argv' desktop/src-tauri/src/perm_probe.rs
    grep -rq 'CGPreflightListenEventAccess' desktop/src-tauri/src
    grep -rq 'CGPreflightPostEventAccess' desktop/src-tauri/src
    grep -rq '/usr/bin/tccutil' desktop/src-tauri/src
    test "$(grep -rl 'fn AXIsProcessTrusted()' desktop/src-tauri/src | wc -l | tr -d ' ')" -eq 1
    grep -q 'fn AXIsProcessTrusted()' desktop/src-tauri/src/perm_probe.rs
    grep -q 'perm_probe::probe_all' desktop/src-tauri/src/permissions.rs
    grep -q 'repair_permission' desktop/src-tauri/src/lib.rs
    grep -rq 'invoke("repair_permission"' desktop/src
    test "$(grep -cE 'assert(_eq|_ne)?!' desktop/src-tauri/tests/perm_repair.rs)" -ge 20
    grep -rq 'quit and reopen Yap to finish' desktop/src
    test -s docs/loop/evidence/yap-p0-perm-repair.md
    grep -q '^ROOT-CAUSE-PROBE yap-p0-perm-repair$' docs/loop/evidence/yap-p0-perm-repair.md
    grep -q 'authority_leaf=' docs/loop/evidence/yap-p0-perm-repair.md
    grep -q 'designated_requirement=' docs/loop/evidence/yap-p0-perm-repair.md
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    mkdir -p desktop/target
    (cd desktop && npx vitest run src/permission.repair.test.ts)
    (cd desktop/src-tauri && cargo test --features custom-protocol --test perm_repair -- --list) > desktop/target/p0-pr-list.txt 2>/dev/null
    grep -q '^probe_matrix_maps_every_combination: test$' desktop/target/p0-pr-list.txt
    grep -q '^stale_identity_without_ledger_after_signature_change: test$' desktop/target/p0-pr-list.txt
    grep -q '^stale_identity_from_ledger_different_signature: test$' desktop/target/p0-pr-list.txt
    grep -q '^repair_argv_is_own_bundle_id_never_all: test$' desktop/target/p0-pr-list.txt
    grep -q '^repair_pane_url_per_service: test$' desktop/target/p0-pr-list.txt
    grep -q '^reprobe_stops_on_granted: test$' desktop/target/p0-pr-list.txt
    grep -q '^reprobe_times_out_at_120s: test$' desktop/target/p0-pr-list.txt
    grep -q '^banner_and_onboarding_share_one_probe: test$' desktop/target/p0-pr-list.txt
    grep -q '^accessibility_repair_also_resets_post_event: test$' desktop/target/p0-pr-list.txt
    grep -q '^listen_event_repair_says_relaunch: test$' desktop/target/p0-pr-list.txt
    (cd desktop/src-tauri && cargo test --features custom-protocol --test perm_repair -- --exact stale_identity_without_ledger_after_signature_change --nocapture) > desktop/target/p0-pr-crux.txt 2>&1
    grep -q '^perm_repair: ledger=none last_run=changed probe=denied state=StaleIdentity$' desktop/target/p0-pr-crux.txt
    grep -q 'test result: ok. 1 passed' desktop/target/p0-pr-crux.txt
    (cd desktop/src-tauri && cargo test --features custom-protocol --test perm_repair)
    (cd desktop/src-tauri && cargo test --features custom-protocol --test perm_truth)
    bash scripts/qa/perm-probe.sh --self-test
    bash scripts/qa/perm-probe.sh --app /System/Applications/Calculator.app --repair-plan > desktop/target/qa-repair-plan.txt
    grep -q '^repair_cmd_accessibility=/usr/bin/tccutil reset Accessibility com.apple.calculator$' desktop/target/qa-repair-plan.txt
    grep -q '^repair_cmd_listenevent=/usr/bin/tccutil reset ListenEvent com.apple.calculator$' desktop/target/qa-repair-plan.txt
    grep -q '^repair_cmd_postevent=/usr/bin/tccutil reset PostEvent com.apple.calculator$' desktop/target/qa-repair-plan.txt
    grep -q '^repair_pane_accessibility=x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility$' desktop/target/qa-repair-plan.txt
    ! grep -q ' All ' desktop/target/qa-repair-plan.txt
    shellcheck scripts/qa/perm-probe.sh
  `,
})

ITEMS.push({
  id: 'yap-p0-insertion-bridge-fallback', prompt: 'yap-p0-insertion-bridge-fallback', branch: 'loop/yap-p0-insertion-fallback-copy-card', gated: null,
  title: 'Insertion never silently fails: AX insertion only when the probe says trusted, a clipboard + ⌘V bridge when PostEvent is granted, an instant Copy card when neither is, and one failing backend call can no longer blank the app as "Bridge error"',
  preflight: `
    ( M=0; test -f desktop/src-tauri/src/perm_probe.rs || M=1; grep -q 'pub fn probe_all' desktop/src-tauri/src/perm_probe.rs || M=1; grep -q 'StaleIdentity' desktop/src-tauri/src/perm_probe.rs || M=1; if [ "$M" = 1 ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-p0-perm-repair has not landed (no perm_probe.rs probe_all / StaleIdentity) — run the review pass to land it, then the build pass again"; exit 0; fi; test -f desktop/src-tauri/tests/insertion_matrix.rs && grep -q 'pub fn choose_insert_path' desktop/src-tauri/src/paste.rs && grep -q 'allSettled' desktop/src/appShell.ts )
  `,
  spec: `
    Panel: not required — FIX (Wilson 2026-10-17: "the bridges ... nothing's working")
    DEPENDS: yap-p0-perm-repair ON MAIN (preflight skips until perm_probe.rs is there; uses its single
      probe, never a second grant check). Blocks: yap-p0-release-smoke.

    REQUIRE (before any code — NO GUESSING)
      - Establish which "bridge" is broken, with output, not opinion: (a) read-only grep of yap.log for
        pasted=false / "paste not confirmed" / "Enable Accessibility" since 2026-10-16 and paste the counts;
        (b) a vitest on unmodified main that makes ONE of the boot invokes in appShell.ts reject and shows
        the whole app replaced by "Bridge error". Both go into the PR body and into
        docs/loop/evidence/yap-p0-insertion-bridge-fallback.md (EVIDENCE FILE above), with the
        post_event_access= line from the helper below.
      - Prove with a probe whether CGEventPost(⌘V) is delivered without Accessibility on this macOS: a tiny
        ad hoc Swift helper (scripts/qa/post-probe.swift, built like fn-post.swift) prints
        post_event_access=<CGPreflightPostEventAccess()> and, only with --post, posts ⌘V into a TextEdit
        document IT created. Do not assume "keyDown needs no AX" — fn-post.swift:32 shows PostEvent is gated.

    DO (names are contract)
      1. paste.rs: pub enum InsertPath { Ax, PasteBridge, CopyCard } and pub fn choose_insert_path(probe:
         &perm_probe::ProbeReport) -> InsertPath, pure. Ax when Accessibility is Granted (AX kAXSelectedText
         on the focused element, falling back to PasteBridge when the element refuses); PasteBridge when
         PostEvent is Granted (pasteboard + CGEventPost ⌘V, receipt-checked by paste_tx as today); CopyCard
         otherwise (no keystroke attempted). copy_and_maybe_paste AND delete_selection call
         choose_insert_path(&perm_probe::probe_all()); every permissions::is_accessibility_trusted() call
         in paste.rs is removed (paste.rs:184, :246, :306 today).
      2. CopyCard: paste.rs emits the Tauri event "insert-copy-card" with the transcript; the pill listens
         for it and shows the text within 100 ms with a Copy button and "Copied — press ⌘V. Repair to
         auto-insert." (Repair = invoke("repair_permission")). A PasteBridge with no receipt
         (paste_tx RECEIPT_WINDOW) also emits insert-copy-card — never a silent pasted=false.
      3. Backend-bridge boot: appShell.ts loads with Promise.allSettled; each failed command shows its own
         inline error + retry in the view that needs it; the status line is never replaced by "Bridge
         error" while any command succeeded. App.tsx's full-screen "UI failed to load backend bridge" only
         when get_settings itself fails.
      4. tests/insertion_matrix.rs, exact names, real assertions (>= 12 in total): ax_trusted_uses_ax,
         ax_dead_post_event_granted_uses_paste_bridge, none_granted_shows_copy_card_and_posts_nothing (a
         counting fake poster proves zero CGEventPost; prints "insertion_matrix: ax=denied post=denied
         path=CopyCard posts=0"), paste_bridge_no_receipt_falls_to_copy_card, wrong_target_copies_only,
         stale_take_copies_only. desktop/src/appShell.boot.test.ts (>= 2 vitest cases): one rejected invoke
         leaves the rest rendered and no "Bridge error" status; get_settings rejection shows the full-screen
         retry.

    WILSON'S WALK (PR body): with Accessibility removed, dictate into TextEdit: the Copy card shows the
    text at once and ⌘V pastes it; after Repair, the same take inserts by itself.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/insertion_matrix.rs
    test -f desktop/src/appShell.boot.test.ts
    test -f scripts/qa/post-probe.swift
    grep -q 'pub enum InsertPath' desktop/src-tauri/src/paste.rs
    test "$(grep -c 'choose_insert_path(' desktop/src-tauri/src/paste.rs)" -ge 3
    grep -q 'perm_probe::probe_all()' desktop/src-tauri/src/paste.rs
    ! grep -q 'is_accessibility_trusted()' desktop/src-tauri/src/paste.rs
    grep -q 'insert-copy-card' desktop/src-tauri/src/paste.rs
    grep -rq 'insert-copy-card' desktop/src
    grep -q 'allSettled' desktop/src/appShell.ts
    test "$(grep -cE 'assert(_eq|_ne)?!' desktop/src-tauri/tests/insertion_matrix.rs)" -ge 12
    test -s docs/loop/evidence/yap-p0-insertion-bridge-fallback.md
    grep -q '^ROOT-CAUSE-PROBE yap-p0-insertion-bridge-fallback$' docs/loop/evidence/yap-p0-insertion-bridge-fallback.md
    grep -q 'post_event_access=' docs/loop/evidence/yap-p0-insertion-bridge-fallback.md
    grep -q 'Bridge error' docs/loop/evidence/yap-p0-insertion-bridge-fallback.md
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    mkdir -p desktop/target
    (cd desktop && npx vitest run src/appShell.boot.test.ts)
    (cd desktop/src-tauri && cargo test --features custom-protocol --test insertion_matrix -- --list) > desktop/target/p0-ins-list.txt 2>/dev/null
    grep -q '^ax_trusted_uses_ax: test$' desktop/target/p0-ins-list.txt
    grep -q '^ax_dead_post_event_granted_uses_paste_bridge: test$' desktop/target/p0-ins-list.txt
    grep -q '^none_granted_shows_copy_card_and_posts_nothing: test$' desktop/target/p0-ins-list.txt
    grep -q '^paste_bridge_no_receipt_falls_to_copy_card: test$' desktop/target/p0-ins-list.txt
    grep -q '^wrong_target_copies_only: test$' desktop/target/p0-ins-list.txt
    grep -q '^stale_take_copies_only: test$' desktop/target/p0-ins-list.txt
    (cd desktop/src-tauri && cargo test --features custom-protocol --test insertion_matrix -- --exact none_granted_shows_copy_card_and_posts_nothing --nocapture) > desktop/target/p0-ins-crux.txt 2>&1
    grep -q '^insertion_matrix: ax=denied post=denied path=CopyCard posts=0$' desktop/target/p0-ins-crux.txt
    grep -q 'test result: ok. 1 passed' desktop/target/p0-ins-crux.txt
    (cd desktop/src-tauri && cargo test --features custom-protocol --test insertion_matrix)
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib paste)
    bash scripts/qa/hotkey-walk.sh --self-test
  `,
})

ITEMS.push({
  id: 'yap-p0-release-smoke', prompt: 'yap-p0-release-smoke', branch: 'loop/yap-p0-release-smoke-signed-app-gate', gated: null,
  title: 'Every DMG build runs a signed-app smoke after signing (codesign --strict on app + DMG with the pinned Developer ID team, a designated-requirement change gate, launch, probe every service, register hotkeys, insert into TextEdit) and the release FAILS when any step fails, including a stale-identity grant',
  preflight: `
    ( M=0; grep -q 'DICTATE_TOGGLE.shortcut()' desktop/src-tauri/src/lib.rs || M=1; grep -q 'HotkeySource::Carbon' desktop/src-tauri/src/lib.rs || M=1; grep -q 'pub fn probe_all' desktop/src-tauri/src/perm_probe.rs || M=1; grep -q 'StaleIdentity' desktop/src-tauri/src/perm_probe.rs || M=1; grep -q 'pub fn choose_insert_path' desktop/src-tauri/src/paste.rs || M=1; grep -q 'insert-copy-card' desktop/src-tauri/src/paste.rs || M=1; if [ "$M" = 1 ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-p0-hotkeys-no-permission, yap-p0-perm-repair and yap-p0-insertion-bridge-fallback must all be on main (DICTATE_TOGGLE / perm_probe::probe_all / choose_insert_path) — run the review pass, then the build pass again"; exit 0; fi; test -x scripts/qa/release-smoke.sh && grep -q 'release-smoke.sh' scripts/sign-local.sh && test -f scripts/qa/fixtures/release-smoke-negative-2026-10-17.log )
  `,
  spec: `
    Panel: not required — FIX (0.9.0 shipped with every TCC grant dead and nothing noticed)
    DEPENDS: yap-p0-hotkeys-no-permission, yap-p0-perm-repair, yap-p0-insertion-bridge-fallback, ALL ON
      MAIN (the preflight skips this item with "dependency not on main" until they are).

    REQUIRE (before any code — NO GUESSING)
      - Read scripts/sign-local.sh --tauri-build and docs/RELEASING.md and paste the exact step where the
        .app/.dmg is produced; the smoke hooks in right after it. Paste perm-probe.sh --compare output for
        the built app vs /Applications/Yap.app (dr_change=yes|no) — the 0.9.0 signal. Both go into
        docs/loop/evidence/yap-p0-release-smoke.md (EVIDENCE FILE above).

    DO (names are contract)
      1. The app's --release-smoke flag (distinct from --smoke, which refuses hotkeys and paste) logs ONE
         line per step through pure formatter functions in desktop/src-tauri/src/smoke.rs, exact formats:
           release-smoke step=probe service=<accessibility|listenevent|postevent|microphone> state=<ProbeState> ledger=<granted|none>
           release-smoke step=hotkeys carbon=<registered|failed> fn_tap=<status>
           release-smoke step=insert path=<Ax|PasteBridge|CopyCard> ok=<true|false>
           release-smoke step=quit ok=true
         It registers hotkeys, allows ONE insertion into the TextEdit document the smoke created (frontmost
         checked first; a unit test proves it never inserts when frontmost is not TextEdit), then quits.
      2. SIGNATURE + IDENTITY STEPS (second-pass reviewer B2), run FIRST and only after signing, launching
         nothing: step=signature runs codesign --verify --deep --strict on the .app AND on the .app inside
         the DMG (hdiutil attach -readonly -nobrowse to a temp mountpoint, detach after), and requires
         Authority "Developer ID Application: ..." with TeamIdentifier=VHYV8C2JNU (pinned) on both.
         step=dr_change runs perm-probe.sh --compare <installed or last-release app> <new app>; dr_change=yes
         FAILS unless BOTH --allow-dr-change and --release-note "<line>" are passed, and the note is printed
         as release_note=<line> for the release notes. --signature-only / --dr-only run just these steps
         (safe on any machine; read-only).
      3. scripts/qa/release-smoke.sh --app PATH | --parse LOG | --self-test [--force-fail STEP]. --app
         launches (open -n, temp data dir, --release-smoke; exit 2 if another Yap with the same bundle id
         is running, never signal it), drives a headless-AppleScript TextEdit document, reads the app's
         yap.log and hands it to the SAME parser --parse uses. It prints step=<name> ok|fail per step.
         THE MATRIX the parser enforces (per service, then per run):
           probe Granted                                  -> ok
           probe Denied/StaleIdentity AND ledger=granted  -> FAIL step=probe (stale identity: a grant the
                                                             ledger saw working reads denied; 0.9.0)
           probe StaleIdentity, any ledger                -> FAIL step=probe
           probe Denied/NotDetermined AND ledger=none     -> ok (genuinely absent: a fresh host)
           EVERY grant dead AND ledger=granted for any    -> FAIL step=probe (the 0.9.0 shape exactly)
           carbon=failed                                  -> FAIL step=hotkeys, always
           accessibility or postevent Granted AND path=CopyCard -> FAIL step=insert (a grant is live, so
                                                             the Copy card is a regression, not a fallback)
           both accessibility and postevent absent (ledger=none) AND path=CopyCard ok=true -> ok
           path=Ax|PasteBridge AND ok=false, or the document text != the smoke string -> FAIL step=insert
           no step=quit ok=true line within 10 s          -> FAIL step=quit
         Exit 0 all ok; 1 any FAIL (names the step); 2 usage / another Yap running. --force-fail STEP makes
         that step fail and must exit 1.
      4. FIXTURES. scripts/qa/fixtures/release-smoke-negative-2026-10-17.log: a REAL captured log — the
         verbatim yap.log excerpt 2026-10-17T07:40:34Z..07:45:48Z from Wilson's machine (read-only copy,
         transcript text redacted, header "# captured from yap.log 2026-10-17 Yap 0.9.0"), with the
         release-smoke lines the 0.9.0 state implies appended under "# expected-under-new-build" (ledger=
         granted, state=StaleIdentity). --parse on it must exit 1 at step=probe. Matrix fixtures in
         scripts/qa/fixtures/release-smoke-matrix/<row>.log are GOLDEN files written by
         tests/release_smoke_lines.rs from the real smoke.rs formatters (golden_fixtures_match_formatter
         regenerates and compares, so the parser is proven against lines the app actually emits), one
         per matrix row, named: granted-all-ax.log (exit 0), absent-grants-copy-card.log (exit 0),
         all-dead-ledger-granted.log (exit 1),
         stale-ledger-granted-probe-denied.log (exit 1), granted-but-copy-card.log (exit 1),
         carbon-failed.log (exit 1), no-quit.log (exit 1). --self-test runs all of them plus every refusal
         and force-fail path and launches nothing.
      5. scripts/sign-local.sh --tauri-build runs release-smoke.sh (signature, dr_change, then --app) AFTER
         its last codesign/notarize step, on the built app and DMG, and exits non-zero
         if it fails (bare exit propagation, no pipe in the exit path). docs/RELEASING.md: the smoke is
         mandatory; dr_change=yes goes into the release notes with the Repair instructions.
      6. This seat never runs --app on Wilson's machine (HAZARD); only --parse / --self-test /
         --signature-only / --dr-only. The full run
         is the release host's step, listed in the PR body.
  `,
  acceptance: `
    test -x scripts/qa/release-smoke.sh
    grep -q 'release-smoke.sh' scripts/sign-local.sh
    grep -q 'release-smoke.sh' docs/RELEASING.md
    grep -q 'release-smoke step=probe' desktop/src-tauri/src/smoke.rs
    grep -q 'release-smoke step=hotkeys' desktop/src-tauri/src/smoke.rs
    grep -q 'release-smoke step=insert' desktop/src-tauri/src/smoke.rs
    test -f scripts/qa/fixtures/release-smoke-negative-2026-10-17.log
    grep -q '^# captured from yap.log 2026-10-17' scripts/qa/fixtures/release-smoke-negative-2026-10-17.log
    grep -q 'wilson_voice_lib\\] Accessibility not trusted' scripts/qa/fixtures/release-smoke-negative-2026-10-17.log
    grep -q 'pasted=false' scripts/qa/fixtures/release-smoke-negative-2026-10-17.log
    test -f desktop/src-tauri/tests/release_smoke_lines.rs
    bash scripts/qa/release-smoke.sh --self-test
    bash scripts/qa/release-smoke.sh --parse scripts/qa/fixtures/release-smoke-matrix/granted-all-ax.log
    bash scripts/qa/release-smoke.sh --parse scripts/qa/fixtures/release-smoke-matrix/absent-grants-copy-card.log
    ! bash scripts/qa/release-smoke.sh --parse scripts/qa/fixtures/release-smoke-negative-2026-10-17.log
    ! bash scripts/qa/release-smoke.sh --parse scripts/qa/fixtures/release-smoke-matrix/stale-ledger-granted-probe-denied.log
    ! bash scripts/qa/release-smoke.sh --parse scripts/qa/fixtures/release-smoke-matrix/granted-but-copy-card.log
    ! bash scripts/qa/release-smoke.sh --parse scripts/qa/fixtures/release-smoke-matrix/carbon-failed.log
    ! bash scripts/qa/release-smoke.sh --parse scripts/qa/fixtures/release-smoke-matrix/no-quit.log
    ! bash scripts/qa/release-smoke.sh --parse scripts/qa/fixtures/release-smoke-matrix/all-dead-ledger-granted.log
    grep -q 'codesign --verify --deep --strict' scripts/qa/release-smoke.sh
    grep -q 'VHYV8C2JNU' scripts/qa/release-smoke.sh
    grep -q 'hdiutil attach' scripts/qa/release-smoke.sh
    awk '/codesign/ {c=NR} /release-smoke.sh/ {r=NR} END {exit !(c > 0 && r > c)}' scripts/sign-local.sh
    test -s docs/loop/evidence/yap-p0-release-smoke.md
    grep -q '^ROOT-CAUSE-PROBE yap-p0-release-smoke$' docs/loop/evidence/yap-p0-release-smoke.md
    grep -q 'dr_change=' docs/loop/evidence/yap-p0-release-smoke.md
    mkdir -p desktop/target
    bash scripts/qa/release-smoke.sh --parse scripts/qa/fixtures/release-smoke-negative-2026-10-17.log > desktop/target/p0-smoke-neg.txt 2>&1; test $? -eq 1
    grep -q '^step=probe fail' desktop/target/p0-smoke-neg.txt
    bash scripts/qa/release-smoke.sh --self-test --force-fail probe; test $? -eq 1
    bash scripts/qa/release-smoke.sh --self-test --force-fail hotkeys; test $? -eq 1
    bash scripts/qa/release-smoke.sh --self-test --force-fail insert; test $? -eq 1
    bash scripts/qa/release-smoke.sh --self-test --force-fail signature; test $? -eq 1
    bash scripts/qa/release-smoke.sh --self-test --force-fail dr_change; test $? -eq 1
    bash scripts/qa/release-smoke.sh --signature-only --app /System/Applications/Calculator.app > desktop/target/p0-smoke-sig.txt 2>&1; test $? -eq 1
    grep -q '^step=signature fail' desktop/target/p0-smoke-sig.txt
    bash scripts/qa/release-smoke.sh --dr-only --app /System/Applications/Calculator.app --compare /System/Applications/TextEdit.app; test $? -eq 1
    bash scripts/qa/release-smoke.sh --dr-only --app /System/Applications/Calculator.app --compare /System/Applications/TextEdit.app --allow-dr-change; test $? -eq 1
    bash scripts/qa/release-smoke.sh --dr-only --app /System/Applications/Calculator.app --compare /System/Applications/TextEdit.app --allow-dr-change --release-note 'loop acceptance override check'
    shellcheck scripts/qa/release-smoke.sh
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    (cd desktop/src-tauri && cargo test --features custom-protocol --test release_smoke_lines)
    git diff --exit-code -- scripts/qa/fixtures/release-smoke-matrix
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib smoke)
  `,
})
