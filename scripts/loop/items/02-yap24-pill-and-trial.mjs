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
    grep -q "14d" desktop/src/pill/license.test.ts
    ! grep -q "trial, more than 7 days   → nothing" desktop/src/pill/license.ts
  `,
  spec: `
    Panel: pending (product copy/threshold — the panel may keep a quiet style, it may not keep silence)
    DEPENDS: Y2-B (open PR #186 — the numeral in both pill styles and the 30px side dock)

    EVIDENCE
      - pill/license.ts:136-165 display policy: "trial, more than 7 days -> nothing (ambient
        silence)". For half the trial the pill says nothing; Wilson's report is exactly this.
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
  `,
  acceptance: `
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/pill/license.test.ts
    npm test
    npm run build
  `,
})

ITEMS.push({
  id: 'yap24-PILL2', prompt: 'yap24-PILL', branch: 'loop/yap24-pill2-backend-stage-events-for-polish-and-paste', gated: 'panel',
  title: 'The backend says which stage a take is in, so "polishing" and "pasting" finally appear on the pill',
  preflight: `
    grep -q "TAKE_STAGE_EVENT" desktop/src-tauri/src/lib.rs
    grep -q "take_stage" desktop/src/float-main.tsx
  `,
  spec: `
    Panel: pending
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
  `,
  acceptance: `
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/pill
    npm run build
    cd src-tauri
    cargo test --features custom-protocol --lib take_stage
    cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-PILL3', prompt: 'yap24-PILL', branch: 'loop/yap24-pill3-blind-hotkey-and-blind-paste-states', gated: 'panel',
  title: 'When the hotkey cannot hear you, the pill says so: Secure Input, a disabled tap, and missing Accessibility are pill states',
  preflight: `
    grep -q "secure_input" desktop/src/float-main.tsx
    grep -q '"blind"' desktop/src/pill/live.ts
  `,
  spec: `
    Panel: pending
    DEPENDS: PERM-C (merged #161), Y1-A (merged #162)

    EVIDENCE
      - lib.rs:893-922 build_status already computes secure.blocked, the tap health message and
        accessibility. float-main.tsx:60-65 BackendStatus declares only recording, busy,
        engine_loading, last_error.
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
  `,
  acceptance: `
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/pill
    npm test
    npm run build
  `,
})

ITEMS.push({
  id: 'yap24-PILL4', prompt: 'yap24-PILL', branch: 'loop/yap24-pill4-hands-free-looks-different-from-hold', gated: 'panel',
  title: 'Hands-free looks different from hold: a lock mark and "tap fn⌃ to stop", in both characters and all docks',
  preflight: `
    grep -q "hands_free" desktop/src/float-main.tsx
    grep -q "hands-free" desktop/src/pill/live.test.ts
  `,
  spec: `
    Panel: pending
    DEPENDS: yap24-PILL2 (same reducer), Y5-K when merged (characters are data)

    EVIDENCE
      - lib.rs:896 status carries hands_free; the pill never reads it (float-main.tsx:60-65).
      - ptt_macos.rs:34 DOUBLE_TAP_MS = 450, :550 double-tap -> hands-free ON; Wilson's log shows
        dozens of hands-free takes, some several minutes long, with the same pill as a hold.

    DO: a listening variant "listening-locked" (phaseVisual label "Hands-free — tap fn⌃ to stop"),
    drawn by the shell as a small lock chip; the live commentary keeps running; Escape/cancel copy
    unchanged. Tests in live.test.ts.
  `,
  acceptance: `
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/pill
    npm run build
  `,
})
