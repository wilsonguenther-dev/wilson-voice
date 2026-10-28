// yap24-OS — FASTER HOTKEY, TRUER PERMISSIONS, BETTER OS CITIZEN. Written 2026-09-26 by the
// architecture audit (docs/ARCHITECTURE-AUDIT-2026-09-26.md §4 and §6). Wilson, 2026-09-26: "the
// app should work better with the operating system, the kernel, the permissioning system; the
// hotkeys faster and smoother; way better than Wispr Flow."
//
// Rules that bind every item here: app-sandbox stays false (memory
// feedback_never_sandbox_utility_apps); the bundle id and the data dir are never renamed (TCC and
// history); no new vendor; headless tests only.
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap24-OS1', prompt: 'yap24-OS', branch: 'loop/yap24-os1-arm-capture-on-key-down-zero-lost-words', gated: 'panel',
  title: 'The first word is never lost and the start is 280 ms sooner: capture arms on key-down and a tap throws the pre-roll away',
  preflight: `
    grep -q "speculative" desktop/src-tauri/src/ptt_macos.rs
    test -f desktop/src-tauri/tests/ptt_speculative_arm.rs
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26, DEFERRED to the next release (not in panelApproved).
    DEPENDS: none

    Panel revisions 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, HIGH, GROUNDED, 2+ seats):
      - Arm speculatively ONLY when the persistent stream is already warm (inside IDLE_CLOSE).
        On a COLD stream, key-down handling is fn/Globe first (flags-changed), and whether this
        is a tap, hold or chord (fn+arrow, fn+Delete, Globe for emoji/input-switch) is only known
        once the gesture resolves — arming on every cold key-down would open the mic, light the
        orange indicator and force a Bluetooth headset into its HFP call profile on plain text
        editing and input-source switching, dozens of times an hour. On a cold stream: begin the
        device open at key-down but do not start delivering audio until the gesture resolves;
        add an explicit Interrupted -> discard branch (today only "released before TAP_MAX_MS"
        discards) and tests "fn+arrow opens no stream" / "a tap on a cold stream leaves it closed".
      - Measure BOTH the warm-stream and cold-stream press-to-first-sample paths separately in
        docs/BUDGETS.md — they are not the same number.

    EVIDENCE
      - ptt_macos.rs:32 HOLD_ARM_MS = 280; :488-509 a thread sleeps 280 ms after key-down and only
        then fires Start. The comment (YV38) is right that the gesture needs the window — but
        capture does not have to wait for the decision.
      - record.rs:390 arms the persistent capture worker only on Start; there is no pre-roll, so
        speech in the first ~280 ms after the press is not in the take.
      - record.rs:1316 IDLE_CLOSE = 60 s: after a minute idle the next take pays a cold stream
        open (bounded by ARM_TIMEOUT 3 s, record.rs:1312), and with a Bluetooth headset a profile
        switch.

    DO
      1. On fn / fn⌃ key-down: arm capture immediately in SPECULATIVE mode (buffering, pill still
         idle). At HOLD_ARM_MS: still held -> promote to a real take, keeping the buffered audio
         from key-down (the take starts at the press, not 280 ms later). Released before
         TAP_MAX_MS -> discard the buffer, then run the existing tap / double-tap logic.
      2. Measure (latency.rs) press->first-sample for warm and cold streams and record both in
         docs/BUDGETS.md; the pill's "listening" flips at promotion, unchanged.
      3. Do not keep the mic open longer than today: speculative arming reuses the same stream and
         the same IDLE_CLOSE; the orange mic indicator behaviour is unchanged.

    Tests (tests/ptt_speculative_arm.rs, pure state machine): hold -> promote with pre-roll
    retained; tap -> discard, no take; double-tap -> hands-free with no leftover buffer; a
    promote after a cold open still starts at the press timestamp.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - DEFER: next release. It touches ptt_macos.rs right after #249 stabilised the hotkeys, and it needs a measured number, not a guess.
        Pre-mortem guard recorded for when it is built: a speculative arm that opens the mic on every fn press lights the orange dot dozens of
        times an hour and flips AirPods to HFP, so warm-stream only plus a soak on Wilson's Mac before landing.
      - STALE REFERENCES FIXED: after #249, HOLD_ARM_MS is at ptt_macos.rs:142 and TAP_MAX_MS at :146 (the evidence cites :32); a start goes through
        HotkeyArbiter (hotkeys.rs). The speculative arm applies to the fn SOURCE only: the Carbon chord needs no pre-roll speculation.
  `,
  acceptance: `
    cd desktop && npm ci && cd src-tauri
    cargo test --features custom-protocol --test ptt_speculative_arm
    cargo test --features custom-protocol --lib ptt
    cargo test --features custom-protocol --lib latency
    cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-OS2', prompt: 'yap24-OS', branch: 'loop/yap24-os2-launch-at-login-via-smappservice', gated: 'panel',
  title: 'Launch at login through SMAppService.mainApp, the way macOS 13+ expects, instead of a LaunchAgent plist',
  preflight: `
    grep -q "SMAppService" desktop/src-tauri/src/lib.rs desktop/src-tauri/src/*.rs
  `,
  spec: `
    Panel: pending
    DEPENDS: none

    EVIDENCE
      - Cargo.toml:32-34 tauri-plugin-autostart "macOS LaunchAgent"; lib.rs:382-387 the autostart
        setting drives it (default OFF, correct).
      - Apple: SMAppService is macOS 13.0+ ("An object the framework uses to control helper
        executables that live inside an app's main bundle",
        https://developer.apple.com/documentation/servicemanagement/smappservice), and
        SMAppService.mainApp is "the main application as a login item", macOS 13.0+
        (https://developer.apple.com/documentation/servicemanagement/smappservice/mainapp).
      - Yap's floor is macOS 12.0 (tauri.conf.json bundle.macOS.minimumSystemVersion).

    DO: on macOS 13+ register/unregister SMAppService.mainApp via objc2 (the crate family the app
    already uses) and read its status back into Settings (enabled / requires approval / not
    registered — the "requires approval" case gets a deep link to Login Items). On 12.x keep the
    plugin path. Migrating users: if a LaunchAgent from the plugin exists and the setting is on,
    register mainApp and remove the agent once. Tests: the pure status->copy mapping and the
    migration decision table.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - DEFER: after the first 100 users. The autostart plugin works; there is no user-visible bug. SMAppService needs an objc2-service-management
        crate that is not in Cargo.lock today, and macOS 12.x (the stated floor) still needs the plugin path, so it is a dual path for no visible gain.
  `,
  acceptance: `
    cd desktop && npm ci && cd src-tauri
    cargo test --features custom-protocol --lib autostart
    cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-OS3', prompt: 'yap24-OS', branch: 'loop/yap24-os3-tcc-truth-pass-input-monitoring-and-copy', gated: 'panel',
  title: 'Permission truth: the hotkey asks for Input Monitoring by name, the purpose strings describe today\'s app, and a grant is noticed on focus',
  preflight: `
    grep -q "CGPreflightListenEventAccess" desktop/src-tauri/src/permissions.rs desktop/src-tauri/src/ptt_macos.rs
    ! grep -q "with Whisper" desktop/src-tauri/Info.plist
  `,
  spec: `
    Panel: pending
    DEPENDS: PERM-E (merged #167)

    EVIDENCE
      - ptt_macos.rs:314-323 creates a LISTEN-ONLY tap at the HID location and on failure logs
        "CGEventTapCreate failed — enable Accessibility". A listen-only tap is gated by Input
        Monitoring (permissions.rs:403 says as much); the copy sends people to the wrong pane.
      - Apple: CGPreflightListenEventAccess / CGRequestListenEventAccess, macOS 10.15+
        (https://developer.apple.com/documentation/coregraphics/cgpreflightlisteneventaccess(),
        https://developer.apple.com/documentation/coregraphics/cgrequestlisteneventaccess()).
      - Info.plist NSMicrophoneUsageDescription: "…transcribe dictation with Whisper" — the
        default engine is Parakeet (src/catalog.json), and meetings also use the mic.
      - permissions.rs:536-542 revocation watch polls every 45 s (30 s floor). A grant made in
        System Settings is not seen until the next tick.

    DO
      1. Input Monitoring: preflight with CGPreflightListenEventAccess, request with
         CGRequestListenEventAccess from the onboarding permission step, and route the tap-failure
         copy + deep link (Privacy_ListenEvent, permissions.rs:811) to it.
      2. Purpose strings: mic = "Yap listens only while you hold the dictation key or record a
         meeting, and transcribes on this Mac." (final copy: panel). No engine names.
      3. Re-check every grant on app activation and on every hotkey press (cheap, read-only
         calls), keeping the 45 s poll as the backstop.
    Tests: permission copy table; the activation re-check path is a pure function over grant
    snapshots.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - RESIDUAL ONLY: of the three DO steps two already shipped (Input Monitoring preflight/request and the activation re-check). What remains is
        the first TCC prompt: Info.plist:16 NSMicrophoneUsageDescription still reads "... to transcribe dictation with Whisper", which names the wrong
        engine at the user's first impression. Replace it with the panel copy: "Yap listens only while you hold the dictation key or record a
        meeting, and transcribes on this Mac." The acceptance greps both the absence of the old string and the presence of the new one.
        Ship this line even if everything else slips.
  `,
  acceptance: `
    ! grep -q "with Whisper" desktop/src-tauri/Info.plist
    grep -q 'transcribes on this Mac' desktop/src-tauri/Info.plist
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd src-tauri
    cargo test --features custom-protocol --lib permissions
    cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-OS4', prompt: 'yap24-OS', branch: 'loop/yap24-os4-hardened-runtime-entitlement-diet', gated: 'panel',
  title: 'Hardened-runtime diet: prove whether allow-jit and allow-unsigned-executable-memory are needed, and drop what is not',
  preflight: `
    ( if ! ( test -x scripts/qa/release-smoke.sh ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 242 (release smoke in sign-local.sh) has not landed; land it, then build OS4"; exit 0; fi; ! grep -q "com.apple.security.cs.allow-jit" desktop/src-tauri/Entitlements.plist && test -x scripts/qa/entitlement-diet.sh )
  `,
  spec: `
    Panel: pending (SECURITY-class: the signing path)
    DEPENDS: SEC-A (merged #165 — stable signing identity)

    Panel revisions 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, HIGH, GROUNDED, 2 seats):
      - The experiment as specified cannot fail: sign-local.sh's codesign call has no
        "--options runtime", so the hardened runtime is OFF for every variant and cs.allow-jit /
        cs.allow-unsigned-executable-memory are inert either way — every removal passes
        vacuously, which could strip something the NOTARIZED (runtime-enabled) release actually
        needs. Sign each variant with "--options runtime --timestamp" under the real Developer ID
        identity and assert codesign -d --entitlements shows flags=0x10000(runtime) before
        trusting a pass/fail. Run the SIGNED binary's real work per variant (in-process
        transcribe-cpp Metal decode via --transcribe-file, plus --transcribe-meeting once NT2
        exists), against a scratch YAP_DATA_DIR with the lane's cached model linked in — an empty
        scratch root never reaches Metal/ASR at all. Give the yap-polish / yap-diarize sidecars
        their own entitlement files and drop --deep from sign-local.sh.

    EVIDENCE
      - Entitlements.plist carries device.audio-input, app-sandbox=false, cs.allow-jit and
        cs.allow-unsigned-executable-memory. The last two weaken the hardened runtime for the whole
        process. Whether ggml-Metal, llama.cpp (in the yap-polish sidecar, a separate binary) or
        WKWebView (JIT runs in the WebContent process) needs them in the APP binary is UNVERIFIED.

    DO: build a signed (Apple Development, sign-local.sh) .app with each entitlement removed in
    turn; run the headless smoke (--smoke with YAP_DATA_DIR, --transcribe-file on the fixture) and
    record pass/fail per variant in docs/RELEASE.md. Remove every entitlement whose removal passes.
    app-sandbox stays false. Never ad-hoc sign (resets TCC).

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - THE EXPERIMENT COULD NOT FAIL: its acceptance was greps plus --lib smoke, and sign-local.sh has no --options runtime, so the hardened runtime
        was off for every variant and every removal passed vacuously. THE GATES NOW: sign-local.sh carries --options runtime (grepped); a new
        scripts/qa/entitlement-diet.sh signs one variant per removed entitlement and, for EACH signed variant, (a) asserts
        codesign -dv shows flags=0x10000(runtime), exiting non-zero otherwise, and (b) runs --transcribe-file on that signed binary against a scratch
        YAP_DATA_DIR with the lane's cached model linked in (an empty scratch root never reaches Metal). The acceptance greps both strings in the
        script AND in docs/RELEASE.md, and runs the script. Give the yap-polish and yap-diarize sidecars their own entitlement files and drop --deep.
      - Expectation to test, not assume (CTO seat): WKWebView JIT runs in the com.apple.WebKit.WebContent XPC process and llama.cpp/ggml Metal compiles
        shaders through MTLDevice, not JIT pages, so both entitlements are probably removable. Unproven until the signed-runtime run passes.
      - Dep: PR #242 (it edits sign-local.sh); the preflight holds this item until scripts/qa/release-smoke.sh exists.
  `,
  acceptance: `
    grep -q "<false/>" desktop/src-tauri/Entitlements.plist
    grep -q "com.apple.security.app-sandbox" desktop/src-tauri/Entitlements.plist
    grep -q "Entitlement audit" docs/RELEASE.md
    grep -q 'flags=0x10000(runtime)' docs/RELEASE.md
    grep -q -- '--options runtime' scripts/sign-local.sh
    test -x scripts/qa/entitlement-diet.sh
    bash -n scripts/qa/entitlement-diet.sh
    grep -q 'flags=0x10000(runtime)' scripts/qa/entitlement-diet.sh
    grep -q -- '--transcribe-file' scripts/qa/entitlement-diet.sh
    YAP_DATA_DIR="$(mktemp -d)/yap-state" YAP_MODEL_DIR="$HOME/code/wilson-voice-loop/cache/models" bash scripts/qa/entitlement-diet.sh
    cd desktop && npm ci && cd src-tauri
    cargo test --features custom-protocol --lib smoke
  `,
})

ITEMS.push({
  id: 'yap24-OS5', prompt: 'yap24-OS', branch: 'loop/yap24-os5-idle-wakeups-event-driven', gated: 'panel',
  title: 'Idle means idle: the fixed polls (Secure Input 2 s, pill space-keeper, permission watch) are consolidated and measured',
  preflight: `
    test -f desktop/src-tauri/tests/idle_timer_budget.rs
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26, DEFERRED to the next release (not in panelApproved).
    DEPENDS: yap24-PILL3 (Secure Input state consumer), Y10-F (idle RAM/CPU publication)

    Panel revisions 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, HIGH — REJECTS part of
    yap24-PILL3's original plan, GROUNDED):
      - Do NOT move the Secure Input check to key-down + a >=10 s backstop, which yap24-PILL3
        proposed and which this item's "consolidate the polls" framing would otherwise adopt:
        under Secure Input the fn PTT event tap is BLIND and never receives that key-down, so the
        only signal left would be the slow backstop — worsening detection from ~4 s to as much as
        ~20 s, in both directions (on and off). KEEP the 2 s poll; it is one Carbon flag read.
        Exempt it BY NAME from this item's "every wakeup needs a reason" table, citing
        secure_input.rs:41's own comment on why 2 s is the interval. Spend the consolidation
        effort on the ioreg owner-PID spawn (secure_input.rs:176) instead — run it only on a
        false-to-true transition, not on every poll tick.

    EVIDENCE
      - secure_input.rs:48 POLL_INTERVAL = 2 s; its own doc (secure_input.rs:41) says macOS
        publishes no notification for Secure Input, so a poll is legitimate — but it runs even
        when no hotkey press is pending.
      - float_pill.rs:47 a space-keeper tick re-asserts the panel's dock and level.
      - permissions.rs:542 WATCH_INTERVAL 45 s; lib.rs:5283 hygiene telemetry interval.
      - Yap idles at ~135 MB RSS (yap23 log, pid 87890) — good; wakeups were never measured.

    DO: one table of every periodic timer in the app (name, interval, why, what event could
    replace it) in docs/BUDGETS.md; [STRUCK 2026-10-26: this sentence contradicted the Panel revision above] secure_input POLL_INTERVAL stays 2 s and is exempt BY NAME in idle_timer_budget; the ioreg owner-PID spawn (secure_input.rs:176) runs on a false-to-true edge only; the space-keeper runs on NSWorkspace active-space-change and
    screen-change notifications instead of a tick where the API allows; a test enumerates the
    registered timers and fails if any idle-time interval below 10 s is not on the named exempt list (secure_input::POLL_INTERVAL is the only entry).

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - The DO text still said to check Secure Input on key-down plus a >= 10 s backstop and to fail any interval below 10 s, against this item's own
        revision (keep the 2 s poll). Fixed in place above and gated: the acceptance requires the 2 s constant to stay and the exempt entry to exist by
        name, and requires a test named ioreg_spawns_only_on_rising_edge to run. DEFER: next release (idle-CPU hygiene, no user-visible wow).
  `,
  acceptance: `
    grep -q 'secure_input::POLL_INTERVAL' desktop/src-tauri/tests/idle_timer_budget.rs
    grep -q 'POLL_INTERVAL: Duration = Duration::from_secs(2)' desktop/src-tauri/src/secure_input.rs
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test idle_timer_budget
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib secure_input::tests::ioreg_spawns_only_on_rising_edge 2>&1 | grep -qE 'test result: ok\\. [1-9][0-9]* passed'
    cargo clippy --all-targets --features custom-protocol
  `,
})
