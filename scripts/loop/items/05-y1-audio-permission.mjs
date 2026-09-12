// Y1 — AUDIO PERMISSION. Wilson, 2026-09-12, verbatim: "audio permission is not
// requested or handled at all."
//
// He is right, and the audit found WHY he experiences it that way even though an
// onboarding button exists. Yap has never asked macOS the actual question.
//
//   git grep -n "AVCaptureDevice\|authorizationStatus\|requestAccess" origin/main -- desktop
//     -> ONE hit, and it is a comment in syscapture.rs:2117 about the CoreAudio
//        process tap ("There is no `requestAccess`, no `authorizationStatus`").
//        For the MICROPHONE there is no call anywhere in the tree.
//
// What Yap does instead, everywhere it claims to know:
//   src/mic_auth.rs:12   microphone_ready()  = default_input_device().is_some()
//                                              && default_input_config().is_ok()
//   src/permissions.rs:66 microphone_probe() = the same two calls, plus prose
//   src/permissions.rs:118 feeds that into PermissionReport.microphone
//   src/Onboarding.tsx:294-303 renders it as "Granted ✓"
//
// Neither call consults TCC. A device exists and a config resolves whether or
// not this bundle is authorized — so the checklist shows a green dot, the button
// says "Granted ✓", the take records, and the transcript is empty. From the
// user's chair that is indistinguishable from "it never asked."
//
// Three further facts that make it worse, all measured:
//   * src/lib.rs:1135, on the ONE dictation entry point, verbatim: "Do NOT call
//     mic_auth::request_microphone_access here — that is Permissions-only." So
//     the hotkey never re-checks. A permission revoked after onboarding is
//     invisible forever.
//   * src/Onboarding.tsx:320 offers "Continue anyway" with no grant.
//   * desktop/src-tauri/tauri.conf.json bundle.macOS.signingIdentity = "-"
//     (ad-hoc). Every local build gets a NEW code signature, and macOS keys TCC
//     grants to the signature — so on Wilson's own machine the grant really does
//     evaporate on every rebuild. ROADMAP.md already names this: "ad-hoc re-sign
//     invalidates trust". It is a permission bug wearing a build-config costume.
//
// The pill has no permission state at all:
//   git grep -n "permission\|denied" origin/main -- desktop/src/pill  -> 0
//
// SHARED PREAMBLE + STANDARD GATE: see 00-y0-harness-and-gates.mjs.
// Depends on Y0-A (a real clippy gate) for everything after PERM-A.

ITEMS.push({
  id: 'PERM-A', prompt: 'Y1', branch: 'loop/perm-a-real-tcc-authorization-status', gated: null,
  title: 'Ask macOS the actual question: AVCaptureDevice authorizationStatus + requestAccess, replacing the device probe that cannot see a denial',
  preflight: `
    grep -q 'AVCaptureDevice' desktop/src-tauri/src/mic_auth.rs
    grep -q 'enum MicAuth' desktop/src-tauri/src/mic_auth.rs
    test 0 -eq "$(grep -rn 'default_input_config().is_ok()' desktop/src-tauri/src/mic_auth.rs desktop/src-tauri/src/permissions.rs | wc -l)"
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test mic_auth
  `,
  spec: `
    Add the real TCC read, and make it the ONLY source of truth for microphone
    authorization in the app.

    In \`desktop/src-tauri/src/mic_auth.rs\`:
      * \`#[repr(i32)] pub enum MicAuth { NotDetermined = 0, Restricted = 1,
        Denied = 2, Authorized = 3 }\` — the exact AVAuthorizationStatus values,
        so the FFI is a transmute-free \`match\`.
      * \`pub fn authorization_status() -> MicAuth\` — objc msgSend to
        \`+[AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio]\`.
        Link AVFoundation: \`#[link(name = "AVFoundation", kind = "framework")]\`.
        Use the \`objc2\`/\`objc\` crate already in the graph if one is (check
        Cargo.lock before adding a dependency); otherwise add \`objc2\` and
        \`objc2-av-foundation\` with exact pinned versions and say so in the PR.
      * \`pub fn request_access(timeout: Duration) -> MicAuth\` — msgSend to
        \`+[AVCaptureDevice requestAccessForMediaType:completionHandler:]\`, which
        is ASYNCHRONOUS: block on a channel the completion block signals, with a
        timeout, and return the status re-read afterwards. macOS shows the system
        dialog only when the status is NotDetermined; when it is Denied the call
        returns immediately with false and the correct next step is the Settings
        deep link (PERM-B).
      * \`#[cfg(not(target_os = "macos"))]\` stub returning \`Authorized\`, so the
        crate still compiles off-Mac like \`permissions.rs:57-62\` already does.

    Then REWIRE, deleting the lie rather than layering on it:
      * \`microphone_ready()\` (mic_auth.rs:12) becomes
        \`authorization_status() == MicAuth::Authorized\` AND a device exists.
        A device probe is a HARDWARE question and must be reported separately —
        keep it as \`pub fn input_device_present() -> bool\`.
      * \`permissions::microphone_probe()\` (permissions.rs:66) returns the TCC
        status, and its prose per status. Its current message — "If Yap is
        missing from System Settings → Microphone, click Dictate once to trigger
        the prompt" — is advice built on the old wrong model; replace it.
      * \`PermissionReport\` (permissions.rs:14-26) gains
        \`pub microphone_status: String\` ("not_determined"|"denied"|"restricted"|
        "authorized") alongside the bool, and \`all_critical_ok\` requires
        Authorized. The bool stays for the existing call sites; the string is
        what the UI branches on.
      * \`#[tauri::command] fn request_microphone()\` (lib.rs:2416-2418) returns
        the status string, not a bool. Add
        \`#[tauri::command] fn microphone_status() -> String\`.

    Tests (\`desktop/src-tauri/tests/mic_auth_status.rs\`), all runnable on a CI
    runner with no mic and no grant:
      * \`mic_auth_discriminants_match_avfoundation\` — the four enum values are
        0/1/2/3. A silent renumber is a permanently wrong permission screen.
      * \`authorization_status_is_the_only_microphone_authority\` — a call-site
        sweep: \`default_input_config\` appears ZERO times in any function whose
        name or doc mentions permission/authorization/granted. Assert with
        pattern AND scope over src/mic_auth.rs and src/permissions.rs.
      * \`report_requires_authorized_for_all_critical_ok\` — construct a
        PermissionReport per status; only Authorized sets all_critical_ok.

    What NOT to do:
      - Do NOT keep the device probe as a FALLBACK when the objc call fails.
        "Couldn't ask macOS, so assume yes" reproduces the exact bug.
      - Do NOT call \`request_access\` from \`authorization_status\`. Reading must
        never prompt; the UI reads constantly.
      - Do NOT use \`AVAudioSession\` — that is iOS. macOS is AVCaptureDevice.
    ── PANEL 2026-09-12, BINDING, three corrections ────────────────────────
    (1) NEVER BLOCK. The first draft said request_access should "block on a
        channel the completion block signals, with a timeout". PERM-C then calls
        it from start_recording, which is reached ONLY on the AppKit main thread
        (lib.rs:4728-4759 — the PTT tap callback wraps everything in
        \`h.run_on_main_thread(...)\`; the tray handler and the sync Tauri command
        path are main-thread too). Blocking there stops the AppKit run loop for
        the length of a human decision on the TCC dialog: no redraws, the pill's
        new \`blocked\` phase and the \`mic_permission_required\` event cannot
        paint (webview IPC is main-thread), and macOS marks the process
        unresponsive. AVCaptureDevice's completion handler is documented as
        arriving on an arbitrary dispatch queue, so if it lands on the main
        queue the wait DEADLOCKS until the timeout and the first press always
        fails. (One panel seat placed this block on the CGEvent tap thread
        instead; that is wrong on the mechanism — the tap hops to main and
        returns — and right on the remedy.)
        So: \`authorization_status()\` is a pure, non-blocking TCC read and is
        the only thing start_recording may call. On NotDetermined, fire
        \`requestAccessForMediaType:completionHandler:\` and return IMMEDIATELY;
        the completion block emits the status event that clears the pill's
        waiting state. Make \`request_microphone\` a
        \`#[tauri::command(async)]\` so the UI path is off the main thread as well.
    (2) \`#[repr(isize)]\`, not \`#[repr(i32)]\`. AVAuthorizationStatus is
        NS_ENUM(NSInteger) — 64-bit on both arm64 and x86_64. The i32 form
        happens to survive on arm64 because 0..3 fits the low word, which makes
        it a latent bug rather than a caught one, and objc2's \`msg_send!\` does
        not verify return encodings (verification lives in \`define_class\` and
        the opt-in \`AnyClass::verify_sel\`). Keep the 0/1/2/3 discriminant test.
    (3) A BUNDLE IS REQUIRED. requestAccess reads
        NSMicrophoneUsageDescription from the MAIN BUNDLE's Info.plist and TCC
        KILLS the process when it is absent. Info.plist is merged only into the
        .app by the bundler, so \`npm run tauri dev\`, \`cargo test\` and Y7-A's
        smoke all run a bare Mach-O with no bundle. Before calling requestAccess,
        read NSBundle.mainBundle's infoDictionary for the key and, when it is
        missing, log ONCE and return NotDetermined instead of calling into TCC.
        Add \`desktop/src-tauri/Info.dev.plist\` with the three usage strings so
        \`tauri dev\` behaves like the shipped app. Any test that touches TCC is
        \`#[ignore]\`d with the reason in its name.

  `,
  acceptance: `
    grep -q 'AVCaptureDevice' desktop/src-tauri/src/mic_auth.rs
    grep -q 'authorizationStatusForMediaType' desktop/src-tauri/src/mic_auth.rs
    grep -q 'requestAccessForMediaType' desktop/src-tauri/src/mic_auth.rs
    grep -qE 'NotDetermined *= *0' desktop/src-tauri/src/mic_auth.rs
    grep -q 'input_device_present' desktop/src-tauri/src/mic_auth.rs
    grep -q 'microphone_status' desktop/src-tauri/src/permissions.rs
    grep -q 'fn microphone_status' desktop/src-tauri/src/lib.rs
    # the old lie is gone from both permission modules   (was 2 sites)
    test 0 -eq "$(grep -c 'default_input_config().is_ok()' desktop/src-tauri/src/mic_auth.rs)"
    test -f desktop/src-tauri/tests/mic_auth_status.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test mic_auth_status ; test $? -eq 0    grep -q 'repr(isize)' desktop/src-tauri/src/mic_auth.rs
    test -f desktop/src-tauri/Info.dev.plist
    grep -q 'NSMicrophoneUsageDescription' desktop/src-tauri/Info.dev.plist
    # the blocking wait must not exist anywhere on the start_recording call graph
    test 0 -eq "$(grep -cE 'thread::sleep|recv_timeout' desktop/src-tauri/src/mic_auth.rs)"

  `,
})

ITEMS.push({
  id: 'PERM-B', prompt: 'Y1', branch: 'loop/perm-b-denied-state-ui-and-settings-deeplink', gated: null,
  title: 'A denied microphone gets its own screen with a working System Settings deep link, not a green check',
  preflight: `
    grep -q 'micStatus\\|microphoneStatus' desktop/src/Onboarding.tsx
    test 0 -eq "$(grep -c 'Continue anyway' desktop/src/Onboarding.tsx)"
    grep -rq 'Privacy_Microphone' desktop/src
    cd ${APP} && npm ci && npm test -- permission
  `,
  spec: `
    Four distinct states, four distinct screens. Today there is one boolean and
    one button label ("Request Microphone" / "Granted ✓", Onboarding.tsx:300-302).

      not_determined -> primary button "Allow microphone access", which calls
                        \`request_microphone\` and shows the macOS dialog.
      authorized     -> a settled row. No button. No spinner.
      denied         -> THE SCREEN THAT DOES NOT EXIST. Say plainly that macOS is
                        blocking Yap, that the dialog will not come back, and
                        that the only way through is System Settings. One primary
                        button: "Open System Settings", then a live re-check when
                        the window regains focus.
      restricted     -> managed by an MDM profile; the user cannot fix it. Say so
                        and do not offer a button that will not work.

    The deep link already exists and is correct — \`permissions.rs:185-206\`
    exposes a settings-pane opener whose Microphone arm is
    \`x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone\`
    with the Ventura+ \`com.apple.settings.PrivacySecurity.extension\` fallback.
    Wire the UI to it; do not write a second one. Confirm the Tauri command name
    with \`git grep -n "open_privacy_pane\\|open_settings_pane" desktop/src-tauri/src/lib.rs\`
    and expose it if it is not yet a command.

    Re-check on focus, not on a timer: \`Onboarding.tsx:98-122\` polls TCC every
    tick while on the permissions step. Keep that poll (it is already bounded to
    one step), and ADD a \`window.addEventListener("focus", ...)\` re-read so
    returning from System Settings updates the row instantly instead of after
    the next interval.

    Delete "Continue anyway" (Onboarding.tsx:320). A person who continues past a
    denied microphone lands in an app whose only feature cannot run — and then
    reports that dictation is broken. Replace it with "Continue without
    dictation", which is honest, and which routes to the done step with a
    persistent banner (PERM-C) rather than to the calibration step, because
    calibration cannot succeed.

    Copy rules: sentence case, no exclamation marks, name the app as Yap, never
    "the app". State what happens next, not what went wrong
    (feedback_think_ux_first).

    Frontend tests, pure, in \`desktop/src/permission.test.ts\` over a new pure
    module \`desktop/src/permission.ts\` holding \`permissionCopy(status)\` and
    \`permissionAction(status)\`: four statuses in, four distinct copy+action
    pairs out, and a case that asserts \`restricted\` yields NO settings action.

    What NOT to do:
      - Do NOT show the denied screen for \`not_determined\`. A first-run user who
        has not been asked yet is not a user who said no.
      - Do NOT put the deep-link URL in the frontend. It must stay a
        compile-time constant in Rust (the same discipline license.rs applies to
        PAYMENT_LINK_URL and for the same reason).
  `,
  acceptance: `
    test -f desktop/src/permission.ts
    test -f desktop/src/permission.test.ts
    grep -qE '"restricted"' desktop/src/permission.ts
    test 0 -eq "$(grep -c 'Continue anyway' desktop/src/Onboarding.tsx)"      # 0  (was 1)
    grep -q 'Continue without dictation' desktop/src/Onboarding.tsx
    grep -q 'addEventListener("focus"' desktop/src/Onboarding.tsx
    # the URL never crosses into the frontend
    test 0 -eq "$(git grep -c 'x-apple.systempreferences' -- desktop/src | wc -l)"
    cd ${APP} && npm ci
    npm test -- permission   ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build            ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'PERM-C', prompt: 'Y1', branch: 'loop/perm-c-recheck-on-every-hotkey-and-pill-denied-state', gated: null,
  title: 'Every hotkey press re-checks the grant, and the pill shows a denied state instead of recording silence',
  preflight: `
    grep -q 'authorization_status' desktop/src-tauri/src/lib.rs
    test 0 -eq "$(grep -c 'Do NOT call mic_auth::request_microphone_access here' desktop/src-tauri/src/lib.rs)"
    grep -q 'permission' desktop/src/pill/live.ts
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test mic_gate
  `,
  spec: `
    \`start_recording\` (src/lib.rs:1128) is documented at lib.rs:1100-1106 as
    "the ONE gate. Every way to begin a new dictation (hotkey, hands-free, tray,
    pill, the Home button, onboarding calibration) funnels into
    \`start_recording\`". That is exactly the right shape and exactly where the
    mic check belongs. It is not there: lib.rs:1135-1136 says
    "Do NOT call mic_auth::request_microphone_access here — that is
    Permissions-only. Opening the real capture stream is enough for TCC."
    Opening the stream is NOT enough — a denied stream delivers silence.

    Add a SECOND gate beside \`license_allows_new_dictation\` (lib.rs:1111),
    modelled on it exactly, because that function is already the proven pattern
    for "refuse a new take, say why once, never touch existing data":

      fn microphone_allows_new_dictation(app, state) -> bool
        * \`mic_auth::authorization_status()\` — a cheap TCC read, not a prompt.
        * Authorized -> true.
        * NotDetermined -> emit \`mic_permission_required\` with status, call
          \`request_access\` ONCE (so the very first hotkey press does the natural
          thing and shows the system dialog), and proceed if it comes back
          Authorized.
        * Denied / Restricted -> emit \`mic_permission_required\`, throttle the
          notification exactly the way \`should_announce_gate\` does
          (license.rs) so leaning on the hotkey is not a notification storm,
          and return false.
        * Order matters: microphone BEFORE license. "Yap cannot hear you" is the
          truer message than "buy a license", and a user with no mic grant must
          never be shown a purchase prompt.

    Add \`tests/mic_gate.rs\`, mirroring \`tests/license_gate.rs\` (which reads
    lib.rs and fails if the license check appears anywhere else):
      * \`microphone_check_lives_only_in_start_recording\` — one call site.
      * \`microphone_is_checked_before_license\` — assert source order in
        start_recording, so a refactor cannot invert them.
      * \`denied_microphone_never_starts_a_recorder\` — with a stubbed status,
        \`record::start_recording\` is not reached.

    THE PILL. \`desktop/src/pill/live.ts:252\` is
    \`LivePhase = "idle" | "listening" | "thinking" | "done" | "sleepy"\` — no
    error, no permission. \`ClassicPill.tsx\` (the DEFAULT pill: lib.rs:407
    \`pill_style: "classic"\`) tracks only \`{recording, busy, message}\` and a
    \`done\` flag. Add \`"blocked"\` to LivePhase and render it in BOTH
    ClassicPill.tsx and YappyPill.tsx: the capsule goes to a muted treatment
    with a struck-through mic glyph, and clicking it opens the permission screen
    in the main window. Listen for \`mic_permission_required\` in
    \`float-main.tsx\` and route it to the phase.

    Extend \`desktop/src/pill/live.test.ts\` (already 271 lines of pure state
    machine tests — the file the CI comment at ci.yml calls out as the reason
    vitest is a gate) with: blocked outranks listening; blocked survives a
    \`recording:false\` event; blocked clears on an \`authorized\` status event.

    What NOT to do:
      - Do NOT call \`request_access\` on every press. Once per NotDetermined
        session. macOS will not re-prompt after a denial and a loop of
        no-op requests is how the hotkey feels dead.
      - Do NOT silently drop the take. A refused press must produce a visible
        pill state — an invisible refusal is the bug being fixed.
    PANEL 2026-09-12 — two binding changes.
    (a) On NotDetermined this item does NOT wait. It calls PERM-A's
        non-blocking request, moves the pill to a \`waiting for permission\`
        phase and RETURNS; the completion event either clears it (and the user
        presses again, or the take auto-arms off the event) or paints
        \`blocked\`. Blocking start_recording blocks the AppKit main thread —
        see PERM-A (1).
    (b) THIS ITEM OWNS THE WHOLE \`LivePhase\` UNION. Six items across BOTH
        lanes mutate desktop/src/pill/live.ts (PERM-C, Y2-C, Y3-C, Y5-C, Y7-D,
        Y8-D), each adding a variant from its own branch off main, and the lanes
        have no barrier — so a later builder that finds its predecessor's PR
        unlanded invents the variant itself and guarantees a conflict on the one
        surface carrying Wilson's defects (a) and (c). PERM-C is the earliest of
        the six, so it lands the COMPLETE union in one commit — every phase
        this loop will need (blocked, waiting, gated, transcribing, polishing,
        pasting, error, model-loading, empty) — with each not-yet-rendered
        variant rendering as a named placeholder and a \`// OWNED BY <item>\`
        comment. Later items only add rendering and copy; none of them edits the
        union again.

  `,
  acceptance: `
    grep -q 'fn microphone_allows_new_dictation' desktop/src-tauri/src/lib.rs
    grep -q 'mic_permission_required' desktop/src-tauri/src/lib.rs
    test 0 -eq "$(grep -c 'Do NOT call mic_auth::request_microphone_access here' desktop/src-tauri/src/lib.rs)"
    grep -q '"blocked"' desktop/src/pill/live.ts
    grep -q 'mic_permission_required' desktop/src/float-main.tsx
    grep -q 'blocked' desktop/src/pill/ClassicPill.tsx
    grep -q 'blocked' desktop/src/pill/YappyPill.tsx
    test -f desktop/src-tauri/tests/mic_gate.rs
    grep -q 'microphone_is_checked_before_license' desktop/src-tauri/tests/mic_gate.rs
    cd ${APP} && npm ci
    npm test                                              ; test $? -eq 0
    cd src-tauri && cargo test --features custom-protocol --test mic_gate ; test $? -eq 0    grep -q 'OWNED BY' desktop/src/pill/live.ts
    test 8 -le "$(grep -ro '\\bcase \"' desktop/src/pill/live.ts | wc -l)"

  `,
})

ITEMS.push({
  id: 'Y1-A', prompt: 'Y1', branch: 'loop/y1-a-cgevent-tap-disabled-by-timeout-is-re-armed', gated: null,
  title: 'A tap macOS disabled is re-armed and reported — the other half of "the hotkey is dead"',
  preflight: `
    grep -q 'kCGEventTapDisabledByTimeout\\|0xFFFFFFFE' desktop/src-tauri/src/ptt_macos.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test tap_health
  `,
  spec: `
    PANEL 2026-09-12, two seats independently. MEASURED at 4e8c9adf:
      ptt_macos.rs:299      mask = flagsChanged | keyDown only
      ptt_macos.rs:303-317  CGEventTapCreate, listen-only, CGEventTapEnable
                            called EXACTLY ONCE at startup
      ptt_macos.rs:332-376  tap_callback branches only on FLAGS_CHANGED (12)
                            and KEY_DOWN (10); everything else returns the event
      ptt_macos.rs:174      the CGEventTapEnable extern is already declared
      grep -rn 'TapDisabled|CGEventTapEnable' over all item files -> ZERO hits
    macOS disables a tap whose callback is slow and notifies the callback with
    \`kCGEventTapDisabledByTimeout\` (0xFFFFFFFE) or \`ByUserInput\`
    (0xFFFFFFFF) — delivered regardless of the event mask — and the tap stays
    dead until CGEventTapEnable is called again. This tree never calls it again.
    So one slow callback, one long decode, or one sleep/wake cycle can silently
    kill the push-to-talk hotkey for the rest of the process's life, which is
    INDISTINGUISHABLE from the permission bug the rest of this file is chasing.
    Fixing TCC without fixing this leaves half of Wilson's observation (b) open.

    Do:
      * Handle both disable types in tap_callback: classify which, log it once
        at warn with the type name, call \`CGEventTapEnable(tap, true)\` (retain
        the CFMachPort so the callback can), and increment a counter.
      * Raise a health state the pill (PERM-C's surface) and PERM-E's report can
        show: "the hotkey stopped listening — re-armed". Re-arm is silent the
        first time and visible if it happens twice in one session.
      * Keep the classification in a PURE function over the event type so it is
        unit-testable with no window server.

    What NOT to do:
      - Do NOT respawn the tap thread as the fix. Re-enable the existing tap.
      - Do NOT swallow ByUserInput as normal: it means a user-input flood took
        the tap out, and it still needs re-arming.
  `,
  acceptance: `
    grep -qE '0xFFFFFFFE|kCGEventTapDisabledByTimeout' desktop/src-tauri/src/ptt_macos.rs
    grep -qE '0xFFFFFFFF|kCGEventTapDisabledByUserInput' desktop/src-tauri/src/ptt_macos.rs
    test 2 -le "$(grep -c 'CGEventTapEnable' desktop/src-tauri/src/ptt_macos.rs)"
    test -f desktop/src-tauri/tests/tap_health.rs
    grep -q 'a_timeout_disable_is_classified_and_re_armed' desktop/src-tauri/tests/tap_health.rs
    grep -q 'a_user_input_disable_is_classified_and_re_armed' desktop/src-tauri/tests/tap_health.rs
    grep -q 're_arm_attempts_are_counted_not_just_logged' desktop/src-tauri/tests/tap_health.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test tap_health ; test $? -eq 0
    sed -i.bak 's/0xFFFFFFFE/0x0EADBEEF/' src/ptt_macos.rs
    cargo test --features custom-protocol --test tap_health ; test $? -ne 0
    mv src/ptt_macos.rs.bak src/ptt_macos.rs
    git diff --exit-code src/ptt_macos.rs ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'PERM-D', prompt: 'Y1', branch: 'loop/perm-d-first-run-preflight-before-any-take', gated: null,
  title: 'First run refuses to reach calibration without a real grant, and a silent take is diagnosed instead of pasted as nothing',
  preflight: `
    grep -q 'silent_take\\|all_zero_samples' desktop/src-tauri/src/lib.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test silent_capture
  `,
  spec: `
    Two halves of the same failure: a take that recorded nothing.

    (1) FIRST-RUN ORDER. \`Onboarding.tsx:41\` is
    \`STEP_ORDER = ["welcome","permissions","calibration","done"]\` and
    calibration records a real take. With no grant that take can only produce
    silence, and the app's own response to silence is the YV16 no-speech gate,
    which correctly refuses to paste — so the user's first experience of Yap is
    a recording that does nothing and says nothing about permissions. Gate the
    calibration step's record button on \`microphone_status === "authorized"\`,
    wearing the same waiting-ribbon pattern the step already uses for a
    downloading model (Onboarding.tsx:~340, YV54), with the reason named.

    (2) DIAGNOSE SILENCE. \`record.rs\` already computes level and voiced
    seconds (\`record.rs:881\` builds an RMS series; \`microphone_ready\` prose
    mentions \`voiced_seconds\`). Add, at the end of a take, before the
    hallucination gate at lib.rs:1546:
      * if the take's peak amplitude is EXACTLY zero across the whole buffer,
        that is not quiet speech — it is a muted or unauthorized input. Emit
        \`take_failed\` with code \`silent_capture\`, re-read
        \`mic_auth::authorization_status()\`, and surface the permission screen if
        it is not Authorized and a "check your input device / it may be muted"
        toast if it is.
      * never paste, never store a transcript row for such a take, and
        preserve the clip under the existing recovery-dir lifecycle
        (\`recovery_dir()\`, lib.rs:1145, YV63) rather than unlinking it.

    Add \`tests/silent_capture.rs\`:
      * \`all_zero_buffer_is_classified_silent_not_quiet\` — a zero buffer and a
        -60 dBFS buffer classify differently. The second must NOT be silent: a
        whisper is a real take.
      * \`silent_capture_never_reaches_the_paste_path\`.
      * \`silent_capture_rechecks_authorization\`.

    Also: \`desktop/src-tauri/Info.plist\` already carries
    NSMicrophoneUsageDescription, NSAudioCaptureUsageDescription and
    NSAppleEventsUsageDescription — verified present at 4e8c9adf, do not touch
    them. Add a test asserting all three survive, because a missing usage string
    is an instant TCC failure with no dialog at all and no other test sees it.

    What NOT to do:
      - Do NOT treat "very quiet" as silent. That would suppress real takes in
        a quiet room and is a worse bug than the one being fixed.
      - Do NOT delete the clip on silent_capture.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/silent_capture.rs
    grep -q 'all_zero_buffer_is_classified_silent_not_quiet' desktop/src-tauri/tests/silent_capture.rs
    grep -q 'silent_capture' desktop/src-tauri/src/lib.rs
    grep -qE 'authorized' desktop/src/Onboarding.tsx
    grep -q 'NSMicrophoneUsageDescription' desktop/src-tauri/tests/silent_capture.rs
    cd ${APP} && npm ci && npm run build ; test $? -eq 0
    cd src-tauri
    cargo test --features custom-protocol --test silent_capture ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'SEC-A', prompt: 'Y1', branch: 'loop/sec-a-stable-signing-identity-so-grants-survive-rebuilds', gated: null,
  title: 'Stop ad-hoc signing local builds — the reason permissions "reset" on Wilson\'s own machine',
  preflight: `
    test 0 -eq "$(grep -c '"signingIdentity": "-"' desktop/src-tauri/tauri.conf.json)"
    test -x scripts/sign-local.sh
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test entitlements
  `,
  spec: `
    \`desktop/src-tauri/tauri.conf.json\` bundle.macOS.signingIdentity is \`"-"\`
    — ad-hoc. macOS keys TCC grants (Microphone, Accessibility, Input
    Monitoring) to the code signature, so every \`npm run tauri build\` produces
    an app macOS treats as a stranger and every grant has to be re-given.
    ROADMAP.md, "Permissions (hardest part)", already states it: "ad-hoc re-sign
    invalidates trust → user toggles off/on after each rebuild until Developer ID
    signing." That is a large part of what "permission is not handled at all"
    feels like from the inside.

    The certificates exist. project_yap_build_state records the documented
    manual dance: \`xattr -cr\`, then
    \`codesign --force --deep --entitlements src-tauri/Entitlements.plist
     --sign "Apple Development: Wilson Guenther (U8BP8Z86T2)"\`, because "the
    tauri codesign flakes on resource-fork detritus". The release path signs with
    \`Developer ID Application: Wilson Guenther (VHYV8C2JNU)\` via
    APPLE_SIGNING_IDENTITY in .github/workflows/release.yml.

    Do:
      * \`signingIdentity\` becomes an env-driven value, read from
        \`APPLE_SIGNING_IDENTITY\` (Tauri supports the env var; confirm against
        the Tauri 2 config docs before choosing between removing the key and
        setting it to null). Ad-hoc must not be the committed default.
      * Create \`scripts/sign-local.sh\`: xattr -cr the bundle, codesign with the
        FIRST available identity in this order — \`Developer ID Application\`,
        then \`Apple Development\` — using Entitlements.plist, then verify with
        \`codesign -dv --verbose=4\` and \`codesign -d --entitlements :-\`. It
        FAILS if no identity is present, printing the exact
        \`security find-identity -v -p codesigning\` command; it never falls back
        to ad-hoc. Replace the hand-typed dance in scripts/rebuild_app.sh with a
        call to it.
      * \`tests/entitlements.rs\`: parse Entitlements.plist and assert
        \`com.apple.security.app-sandbox\` is present AND its value is FALSE —
        the VALUE, not the key (feedback_never_sandbox_utility_apps says exactly
        this), that \`com.apple.security.device.audio-input\` is true, and that
        the two dylib-injection entitlements the plist's own comment forbids
        (\`cs.disable-library-validation\`,
        \`cs.allow-dyld-environment-variables\`) are ABSENT.
      * Document in docs/RELEASE.md: which identity signs what, and that a
        grant lost after a rebuild means the signature changed.

    What NOT to do:
      - Do NOT enable the sandbox. Ever. It silently kills the CGEvent tap, the
        synthesized ⌘V and AXIsProcessTrusted, and the failure looks like a
        permissions bug even when the grant is present.
      - Do NOT hardcode a certificate SHA or a team id into a committed file.
        Read the identity at sign time.
      - Do NOT print a certificate name or serial into any log this repo commits.
    PANEL 2026-09-12 — three seats converged that this item, whose whole point
    is "grants survive a rebuild", cannot currently tell a stably-signed build
    from the ad-hoc one it replaces: every check it lands is a grep of JSON or
    shell text, with no build, no codesign and no verification.
      * DEFINE THE UNSET CASE. No APPLE_SIGNING_IDENTITY -> the build FAILS
        with the \`security find-identity -v -p codesigning\` hint. It NEVER
        falls through to ad-hoc and never to unsigned — unsigned is worse than
        ad-hoc for TCC persistence and would pass a "no ad-hoc" grep. An
        env-driven identity plus a keychain certificate is state "found on the
        machine", which Y0-B's runtime-dependency rule forbids, so the named
        failure is what makes it legitimate.
      * PROVE IT. scripts/sign-local.sh runs inside the acceptance against a
        built .app and the acceptance greps its own \`codesign -dv --verbose=4\`
        output for an Authority line and for the ABSENCE of "Signature=adhoc",
        plus \`codesign -d --entitlements :-\` showing app-sandbox false.
      * TWO PROFILES, and Y7-E must agree with them: a RELEASE profile
        (Developer ID + notarized + stapled — the only identity available on
        this machine is a Developer ID) and a LOCAL profile (Apple Development,
        unnotarized, stable designated requirement, documented as such in
        docs/RELEASE.md). \`spctl --assess\` reporting "Notarized Developer ID"
        is a RELEASE assertion only; a locally signed DMG cannot satisfy it, and
        Y7-E asserting it unconditionally makes the two items disagree about
        what a good build is.

  `,
  acceptance: `
    test 0 -eq "$(grep -c '"signingIdentity": "-"' desktop/src-tauri/tauri.conf.json)"   # 0 (was 1)
    grep -q 'APPLE_SIGNING_IDENTITY' desktop/src-tauri/tauri.conf.json docs/RELEASE.md
    test -x scripts/sign-local.sh
    grep -q 'find-identity' scripts/sign-local.sh
    test 0 -eq "$(grep -c 'sign "-"' scripts/sign-local.sh)"
    grep -q 'sign-local.sh' scripts/rebuild_app.sh
    test -f desktop/src-tauri/tests/entitlements.rs
    grep -q 'app-sandbox' desktop/src-tauri/tests/entitlements.rs
    grep -q 'disable-library-validation' desktop/src-tauri/tests/entitlements.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test entitlements ; test $? -eq 0    grep -q 'find-identity' scripts/sign-local.sh
    grep -q 'codesign -dv' scripts/sign-local.sh
    grep -q 'adhoc' scripts/sign-local.sh
    grep -q 'Apple Development' docs/RELEASE.md

  `,
})

ITEMS.push({
  id: 'Y1-B', prompt: 'Y1', branch: 'loop/y1-b-register-the-sleep-wake-observer-that-does-not-exist', gated: null,
  title: 'Register the sleep/wake observer two later items already claim exists',
  preflight: `
    grep -qE 'NSWorkspaceWillSleepNotification|IORegisterForSystemPower' desktop/src-tauri/src/power.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test matrix_row16_sleep_wake
  `,
  spec: `
    PANEL 2026-09-12, two seats independently, and it deletes a FALSE CAPABILITY
    CLAIM from two other items. PERM-E's spec says "power.rs already observes
    sleep/wake — reuse it, do not add a second observer" and Y6-D repeats it.
    MEASURED at 4e8c9adf: power.rs registers NOTHING. It is
    IOPMAssertionCreateWithName / IOPMAssertionRelease only (power.rs:63-135).
    The repo has already written this down itself —
    meeting_matrix.rs:398-408 carries
    \`Coverage::PolicyOnly { absent_call_site: "NSWorkspaceWillSleepNotification" }\`
    with the comment "power.rs mentions NSWorkspaceWillSleepNotification in a
    comment and registers nothing", and
    \`git grep -n "WillSleep|DidWake|IORegisterForSystemPower|addObserver" -- desktop\`
    returns comments only, zero registrations.

    Following PERM-E as first written therefore produces a watcher that never
    re-reads after wake — the single most common way a TCC grant or an audio
    device changes under a laptop user — plus a test that asserts a subscription
    to a publisher with no input.

    Do:
      * Register the call site in power.rs and NOWHERE else: a block-based
        observer on \`NSWorkspace.sharedWorkspace.notificationCenter\` for
        \`NSWorkspaceWillSleepNotification\` and
        \`NSWorkspaceDidWakeNotification\` (objc2-app-kit 0.3.2 and block2 0.6.2
        are already in desktop/Cargo.lock — no new dependency), or
        IORegisterForSystemPower if the block API fights the Tauri run loop.
      * Fan ONE event out to three consumers: the dictation path (Y6-D's
        mid-take behaviour), meeting_matrix's SleepEvent, and PERM-E's
        revocation re-check. One publisher, three subscribers.
      * Flip meeting_matrix row 16 off \`Coverage::PolicyOnly\` and assert the row
        and the code agree, so they can never drift apart again.
      * Unregister on shutdown. An observer that outlives the app is a crash.
  `,
  acceptance: `
    grep -qE 'NSWorkspaceWillSleepNotification|IORegisterForSystemPower' desktop/src-tauri/src/power.rs
    grep -q 'NSWorkspaceDidWakeNotification' desktop/src-tauri/src/power.rs
    test 0 -eq "$(grep -c 'absent_call_site: "NSWorkspaceWillSleepNotification"' desktop/src-tauri/src/meeting_matrix.rs)"
    test -f desktop/src-tauri/tests/matrix_row16_sleep_wake.rs
    grep -q 'one_observer_fans_out_to_dictation_meetings_and_permissions' desktop/src-tauri/tests/matrix_row16_sleep_wake.rs
    grep -q 'row16_is_no_longer_policy_only' desktop/src-tauri/tests/matrix_row16_sleep_wake.rs
    grep -q 'the_observer_is_unregistered_on_shutdown' desktop/src-tauri/tests/matrix_row16_sleep_wake.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test matrix_row16_sleep_wake ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'PERM-E', prompt: 'Y1', branch: 'loop/perm-e-permission-health-row-and-revocation-watch', gated: null,
  title: 'One permission health surface, watched for revocation, covering mic + Accessibility + Input Monitoring + audio capture',
  preflight: `
    grep -q 'permission_changed' desktop/src-tauri/src/permissions.rs
    grep -rq 'PermissionHealth' desktop/src
    cd ${APP} && npm ci && npm test -- permission
  `,
  spec: `
    Yap needs four grants and reports them in three unrelated places today: the
    onboarding checklist (Onboarding.tsx:286-325), \`PermissionReport.summary\`
    (permissions.rs:23), and the meeting setup flow (the audio-capture deep link
    at permissions.rs:202, YV102). A user whose Accessibility grant was revoked
    by an OS update finds out when paste silently fails.

    Build ONE surface and one watcher:
      * \`permissions::report()\` gains Input Monitoring
        (\`Privacy_ListenEvent\`, already in the deep-link table at
        permissions.rs:196 — needed for the modifier-only fn / fn⌃ PTT hold that
        \`ptt_macos.rs\` implements) and system audio capture
        (\`Privacy_AudioCapture\`, for the notetaker tap), each with its own
        status string and its own deep link.
      * A single watcher thread re-reads all four on: app launch, window focus,
        wake from sleep (\`power.rs\` already observes sleep/wake — reuse it, do
        not add a second observer), and every hotkey refusal. It emits
        \`permission_changed\` ONLY on a transition, never on every read.
      * A \`PermissionHealth\` row in the main window that is INVISIBLE when all
        four are fine and a single calm line when one is not, with the one
        button that fixes it. Not a dashboard, not four persistent cards
        (feedback_no_generic_ui).
      * The pill's \`blocked\` phase (PERM-C) covers mic. Accessibility failure has
        its own shape: the take succeeds and the PASTE fails. Route that to the
        existing paste error path (\`paste_tx.rs\`) so the message names
        Accessibility instead of reading as a transcription failure.

    Minimum polling discipline: no interval under 30 s anywhere in the watcher,
    and it must park entirely while all four are Authorized. Yap's energy pass
    (YV81) removed busy timers on purpose; do not reintroduce one here.

    Tests: pure \`permissionHealth(report)\` in desktop/src/permission.ts ->
    \`{ visible, line, action } \`, with the all-authorized case asserting
    \`visible === false\`. Rust side: \`tests/permission_transitions.rs\` asserts
    \`permission_changed\` fires once per transition and zero times on repeat
    reads of an unchanged report.

    What NOT to do:
      - Do NOT add a second sleep/wake observer. power.rs owns that.
      - Do NOT show a permission banner while all four are granted. A
        permanent nag is how a good app becomes nagware.
    ── PANEL 2026-09-12, BINDING ───────────────────────────────────────────
    (1) DELETE the claim "power.rs already observes sleep/wake — reuse it, do
        not add a second observer". MEASURED: power.rs registers NOTHING
        (power.rs:63-135 is IOPMAssertionCreateWithName/Release only) and the
        repo says so itself at meeting_matrix.rs:398-408,
        \`Coverage::PolicyOnly { absent_call_site: "NSWorkspaceWillSleepNotification" }\`.
        Y1-B now writes that call site and runs BEFORE this item. Consume Y1-B's
        publisher; do not register a second observer.
    (2) FOUR GRANTS, TRI-STATE, and two of them cannot be read the way the
        first draft assumed. Each grant reports Authorized / Denied / UNKNOWN,
        and Unknown is a first-class, NON-NAGGING state:
          * Microphone — AVCaptureDevice.authorizationStatusForMediaType (PERM-A).
          * Accessibility — AXIsProcessTrustedWithOptions (permissions.rs:50-54).
          * Input Monitoring — \`IOHIDCheckAccess(kIOHIDRequestTypeListenEvent)\`
            from IOKit, named here because it appears NOWHERE in the tree today
            (\`git grep IOHIDCheckAccess -- desktop\` -> 0). permissions.rs:195-204
            holds deep LINKS only, which is what the first draft mistook for a
            status. A tap that is already working under an Accessibility grant
            must never be reported blocked.
          * System audio capture — UNKNOWN BY CONSTRUCTION. There is no public
            API, and syscapture.rs:2112-2119 already quotes the reason: "There's
            no public API to request audio recording permission or to check if
            the app has that permission ... no requestAccess, no
            authorizationStatus", and TCC does not re-ask after a denial. Infer
            it only from the YV102 pre-warm's observed outcome. NEVER probe and
            NEVER report a probe result as a grant — that is the same lie PERM-A
            exists to delete.
    (3) DELETE \`ffmpeg_ok\`. permissions.rs:119 hardcodes it true with the
        comment "no longer required — cpal in-process", and Onboarding.tsx:22-30
        still models it. A vestigial always-true row in the surface that is
        supposed to be the single source of permission truth is worse than no
        row. Removing it also means \`all_critical_ok\` stops being computed from
        a fiction.
    (4) The four rows here ARE the onboarding checklist's rows — one component,
        one source — so PERM-B and Y6-A render this, not their own list.

  `,
  acceptance: `
    grep -q 'Privacy_ListenEvent' desktop/src-tauri/src/permissions.rs
    grep -q 'permission_changed' desktop/src-tauri/src/permissions.rs
    grep -q 'permissionHealth' desktop/src/permission.ts
    grep -rq 'PermissionHealth' desktop/src
    test -f desktop/src-tauri/tests/permission_transitions.rs
    # no sub-30s polling introduced anywhere in the watcher
    test 0 -eq "$(grep -rnE 'from_secs\\((?:[0-9]|1[0-9]|2[0-9])\\)' desktop/src-tauri/src/permissions.rs | wc -l)"
    cd ${APP} && npm ci
    npm test -- permission ; test $? -eq 0
    cd src-tauri && cargo test --features custom-protocol --test permission_transitions ; test $? -eq 0    grep -q 'IOHIDCheckAccess' desktop/src-tauri/src/permissions.rs
    test 0 -eq "$(grep -c 'ffmpeg_ok' desktop/src-tauri/src/permissions.rs)"
    test 0 -eq "$(grep -rc 'ffmpeg_ok' desktop/src/Onboarding.tsx)"
    grep -q 'Unknown' desktop/src-tauri/src/permissions.rs
    grep -q 'an_unknown_grant_is_invisible_and_never_nags' desktop/src-tauri/tests/permission_health.rs
    grep -q 'audio_capture_is_unknown_by_construction_never_probed' desktop/src-tauri/tests/permission_health.rs

  `,
})
