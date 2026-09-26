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
    Panel: pending
    DEPENDS: none

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
  `,
  acceptance: `
    ! grep -q "with Whisper" desktop/src-tauri/Info.plist
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
    ! grep -q "com.apple.security.cs.allow-jit" desktop/src-tauri/Entitlements.plist
  `,
  spec: `
    Panel: pending (SECURITY-class: the signing path)
    DEPENDS: SEC-A (merged #165 — stable signing identity)

    EVIDENCE
      - Entitlements.plist carries device.audio-input, app-sandbox=false, cs.allow-jit and
        cs.allow-unsigned-executable-memory. The last two weaken the hardened runtime for the whole
        process. Whether ggml-Metal, llama.cpp (in the yap-polish sidecar, a separate binary) or
        WKWebView (JIT runs in the WebContent process) needs them in the APP binary is UNVERIFIED.

    DO: build a signed (Apple Development, sign-local.sh) .app with each entitlement removed in
    turn; run the headless smoke (--smoke with YAP_DATA_DIR, --transcribe-file on the fixture) and
    record pass/fail per variant in docs/RELEASE.md. Remove every entitlement whose removal passes.
    app-sandbox stays false. Never ad-hoc sign (resets TCC).
  `,
  acceptance: `
    grep -q "<false/>" desktop/src-tauri/Entitlements.plist
    grep -q "com.apple.security.app-sandbox" desktop/src-tauri/Entitlements.plist
    grep -q "Entitlement audit" docs/RELEASE.md
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
    Panel: pending
    DEPENDS: yap24-PILL3 (Secure Input state consumer), Y10-F (idle RAM/CPU publication)

    EVIDENCE
      - secure_input.rs:48 POLL_INTERVAL = 2 s; its own doc (secure_input.rs:41) says macOS
        publishes no notification for Secure Input, so a poll is legitimate — but it runs even
        when no hotkey press is pending.
      - float_pill.rs:47 a space-keeper tick re-asserts the panel's dock and level.
      - permissions.rs:542 WATCH_INTERVAL 45 s; lib.rs:5283 hygiene telemetry interval.
      - Yap idles at ~135 MB RSS (yap23 log, pid 87890) — good; wakeups were never measured.

    DO: one table of every periodic timer in the app (name, interval, why, what event could
    replace it) in docs/BUDGETS.md; Secure Input is checked on key-down and on a slow backstop
    (>= 10 s) instead of every 2 s; the space-keeper runs on NSWorkspace active-space-change and
    screen-change notifications instead of a tick where the API allows; a test enumerates the
    registered timers and fails if any idle-time interval drops below 10 s.
  `,
  acceptance: `
    cd desktop && npm ci && cd src-tauri
    cargo test --features custom-protocol --test idle_timer_budget
    cargo test --features custom-protocol --lib secure_input
    cargo clippy --all-targets --features custom-protocol
  `,
})
