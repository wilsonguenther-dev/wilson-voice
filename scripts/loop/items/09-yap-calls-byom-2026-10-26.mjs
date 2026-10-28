// yap-calls-byom-2026-10-26 — CALL DETECTION (every calling app, browser vs desktop), BRING-YOUR-OWN-MODEL for notes and
// polish, and WHERE NOTES LIVE (destinations). Written 2026-10-26T14:43Z (date -u) by the research + spec seat.
// Wilson 2026-10-26: "I hope the calling system can detect phone calls like FaceTimes or Zooms like Read AI can … browser
// calls versus desktop calls: Zoom, Read AI, Google Meet, FaceTime, every calling service … build the API model plugin so
// people can bring their own models to do their note summaries and polishing, and they can connect their internal database
// on their phone or computer for where the data lives and notes live."
//
// PLAN + EVERY CITATION: docs/plans/yap-calls-and-byom-2026-10-26.md (read it first; the section numbers below are its; its
// "Panel revisions 2026-10-26" section at the end supersedes it wherever they differ).
// The CALL items EXTEND yap-notetaker-detect (08 file: meeting_detect.rs, the Record offer, the calendar signal) and DEPEND
// on it; they never rebuild it. The BYOM items sit BEHIND PR 250's NotesBackend trait (summary_jobs.rs) and DEPEND on it.
// yap-res-0 ABSORBS the deferred yap24-X1 (same setting key notes_mirror_dir, so X1's preflight passes and it retires).
// Dependency holds use the harness marker: ONE subshell that prints DEPENDENCY-NOT-ON-MAIN and exits 0 when the earlier
// work is absent from origin/main (template.mjs runBuild; the marker is assembled at run time with printf). Open-PR holds
// use: [ "$(gh pr view N --json state -q .state 2>/dev/null)" = OPEN ] — a gh failure never holds an item, only an OPEN PR does.
//
// PANEL 2026-10-26 (Senior Panel on calls + BYOM + destinations; seat A = Security + macOS, seat B = PM + User + Pre-mortem;
// synthesis = this edit). v1 SET = yap-call-0, yap-call-1, yap-call-2, yap-byom-0, yap-byom-1, yap-res-0.
//   FOLDED   yap-call-4  -> yap-call-0 (scripts/call-walk.sh + docs/loop/CALL-COVERAGE.md + tests/call_coverage_ledger.rs;
//                           the in-call walk is Wilson's checklist in the plan doc, not a loop item). Tombstone below.
//   FOLDED   yap-res-1   -> yap-res-0 (the path classifier: Mobile Documents / CloudStorage / else No). Tombstone below.
//   DEFERRED yap-call-3  -> next release (re-opens OS-5; invisible; macOS 26 only). Stays gated and UNAPPROVED; its spec now
//                           carries the CORRECT tap construction + test so it is right when built.
//   DEFERRED yap-byom-2  -> next release (OpenRouter/Custom already reach Claude). Its redirect/x-api-key rule lives in
//                           yap-byom-0's shared client factory so it is covered the day byom-2 is built.
//   DEFERRED res-0 SQLite destination -> next release (the schema is a contract forever). Folder + webhook ship.
//   DEFERRED call-2 Safari + Firefox path -> next release (Chrome-family ships).
// BLOCKING SET encoded below as real gates: consent.ts promise made conditional (+ CONSENT_PILL_LINE) in call-1 and
// yap-notetaker-detect; offer persists as a chip + accept_call_offer(offer_id) policy test; per-process input listeners
// re-registered on every ProcessObjectList change; --probe-calls never prints raw titles; Keychain account = provider id +
// host; sentinel-key leak test; ONE reqwest factory with redirect::Policy::none() + mock-307 test; prompts live in
// polish_protocol.rs; byom-1 own zero-network test; no silent 1.1 GB download; circuit breaker; Ollama context clamp;
// "-cloud" models label Yes; filename sanitiser; hash-of-last-write; destinations on their own worker with a persisted outbox;
// webhook = timestamp.body HMAC-SHA256 with a stable X-Yap-Delivery id and a 300 s replay window.
// REBASE NOTES (each item's preflight holds on the open PRs it overlaps): PR 250 (summary_jobs.rs) and PR 246 (progress.ts,
// Meetings.tsx) under byom/res; PR 241 (cli.rs) under yap-call-0's --probe-calls; PRs 247 and 239 (permissions.rs) under
// yap-call-2.
//
// EVERY ITEM IS gated:'panel' AND CLASS SEC (capture, permissions, keys or outbound network). Label each PR "security"
// (gh pr edit <N> --add-label security) so the gate seats review-opus AND review-opus2. Review: Opus only.
// HAZARD (same as 07/07b/08/00a): no agent launches the GUI app, posts events into, signals or quits Wilson's running
// /Applications/Yap.app, writes TCC, or places a real call. Automated proof = cargo test / vitest / a script self-test over
// RECORDED FIXTURES; on-device checks go in the PR body as WILSON'S WALK. No acceptance line needs a mic, a TCC grant, a
// network connection beyond 127.0.0.1, or a real API key.
// LEDGER: keep Tauri; never sandbox; no Nango (every connector from scratch); no Screen Recording permission; no private
// API; default notes model stays the bundled 1.5B (PR 250); polish_model stays the bundled-model override; nothing records
// without a click; "your words never leave the Mac unless you point them somewhere" stays TRUE and VISIBLE (every provider
// and destination carries a "Leaves this Mac: Yes/No" label and the label must be TRUE); cloud polish is allowed (label Yes)
// but off by default; a call ending only PROMPTS; destinations = meetings only; capture stays everything-except-Yap by
// default. Polling floor 30 s. Zero users: no backwards-compat burden.
// ACCEPTANCE HYGIENE (from 01): every cargo line carries YAP_DATA_DIR="$(mktemp -d)/yap-state" inline. A "!" absence line
// passes on a crash, so each one is paired with a positive line that proves its target exists (telemetry 2026-10-19).
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap-call-0', prompt: 'yap-calls', branch: 'loop/yap-call-0-signal-matrix-and-fixtures', gated: 'panel',
  title: 'Call signal matrix: per-process mic-in-use + bundle id from CoreAudio, a call-app catalog, one fused confidence score, recorded fixtures, a --probe-calls CLI that never prints a raw title, and the walk script + coverage ledger',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/meeting_detect.rs ) || [ "$(gh pr view 241 --json state -q .state 2>/dev/null)" = OPEN ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-notetaker-detect (meeting_detect.rs) has not landed, or PR 241 (it changes cli.rs, where --probe-calls goes) is still open; land it, then build this"; exit 0; fi; test -f desktop/src-tauri/src/call_signals.rs && test -f desktop/src-tauri/src/call_catalog.json && test -f desktop/src-tauri/tests/call_signal_matrix.rs && test -f desktop/src-tauri/tests/call_coverage_ledger.rs && test -x scripts/call-walk.sh && grep -q 'kAudioProcessPropertyIsRunningInput' desktop/src-tauri/src/syscapture.rs )
  `,
  spec: `
    Panel: APPROVED 2026-10-26 (new OS surface: per-process audio state of OTHER apps). Class SEC — label the PR "security".
    DEPENDS: yap-notetaker-detect (owns meeting_detect.rs, the offer and the calendar signal; its app/browser signal (b) is
    STRUCK there and OWNED HERE and by yap-call-2). REBASE: PR 241 changes cli.rs (hold in the preflight).
    ABSORBS the cut yap-call-4: scripts/call-walk.sh, docs/loop/CALL-COVERAGE.md, tests/call_coverage_ledger.rs.

    WHY (plan §2.2, measured on this Mac, macOS 26.6.2): audio is played and captured by HELPER processes — the CoreAudio
    process object for Chrome is com.google.Chrome.helper (parent pid -> Google Chrome), Safari's is com.apple.WebKit.GPU
    (parent pid 1, launchd), FaceTime/phone run through com.apple.avconferenced / callservicesd (com.apple.TelephonyUtilities).
    The UI pid produces no audio for any browser (syscapture.rs:13-25, finding OS-5). kAudioDevicePropertyDeviceIsRunningSomewhere
    (AudioHardware.h:1015 on this SDK, not :918) is useless while Yap holds the mic. The industry signal is "an app started
    using the microphone" (Circleback, Granola auto-stop — plan §2.1). Yap's own state is NOT a constant: a live probe shows Yap
    running with in=0 when idle; "Yap in=1" holds only while its mic is warm, so the idle-yap-holds-mic fixture is ONE state,
    not the baseline (fixture idle-yap-quiet is the other).

    DO
      1. syscapture.rs: generalise process_output_activity (syscapture.rs:3338) into process_audio_activity() returning
         ProcAudio { pid, parent_pid, bundle_id: Option<String>, running_input, running_output } — add
         kAudioProcessPropertyIsRunningInput and kAudioProcessPropertyBundleID (CFString; release it) per object. Keep the
         existing output-only callers working (a thin adapter), keep "unreadable = skipped, never evidence". Scope the
         comment at syscapture.rs:28-29 ("no ProcessObjectList enumeration, no re-enumeration listener") to the TAP TARGET in
         the same PR so it stops contradicting this item.
      2. call_catalog.json (bundled, include_str!): one row per call app — bundle_id, display name, kind (native|browser|
         system_daemon), helper patterns, title patterns (browsers only), verified: true|false. Seed with plan §2.2(4); rows
         not verified on a real install ship verified:false (a Wilson walk flips them).
      3. call_signals.rs: CallSnapshot (plain serde data: procs, running_apps, frontmost, browser_titles: Option<…> where
         None = Accessibility not granted, ax_source: "yap" | "terminal" | "unknown", calendar: Option<…>), attribution
         (catalog bundle -> helper via parent_pid -> WebKit.GPU ONLY with a Safari window-title match (it has ppid 1, so
         "Yap's children" can never remove it) -> avconferenced/callservicesd while FaceTime/Phone runs), and
         score(snapshot, catalog, memory) -> Option<CallCandidate { bundle_id, name, confidence, via }> with the table in
         plan §3 (offer at >= 60; browser input with AX granted and no title match caps at 55). Yap's own pid and its children
         are removed BEFORE scoring. meeting_detect.rs calls score() — there is no second, UI-pid app signal anywhere.
         Also here, pure and tested: offer_copy(candidate, catalog) -> the offer text: "<App> call — record it?" ("Zoom call —
         record it?"), and for kind system_daemon with verified:false (com.apple.FaceTime, com.apple.mobilephone) "FaceTime
         call — record your side?" — the far side of FaceTime/Phone is NOT measured, so the default copy never claims it.
      4. Event-driven cadence (macOS 14.4+, same gate as the tap). A ProcessObjectList listener
         (kAudioHardwarePropertyProcessObjectList) fires only when processes JOIN or LEAVE — Zoom and the Chrome helper are
         already HAL clients, so a call starting changes only kAudioProcessPropertyIsRunningInput. Therefore: on EVERY
         ProcessObjectList change, (re)register an AudioObjectAddPropertyListener for kAudioProcessPropertyIsRunningInput on
         each process object, reusing the listener pattern in input_format.rs:915-1081 (pure fn process_listener_diff(prev,
         next) -> { add, remove } is the tested seam). Plus the existing NSWorkspace notifications and the ONE 30 s timer as
         the BACKSTOP, never faster. The timer is OFF when Settings > Meetings "Offer to record" is off (pure fn
         timer_enabled(offer_enabled)). While recording, "call ended" reuses the 1 s meeting tick (meeting_control.rs:57): no
         new timer. Budget (measured, one property read = 112 µs): a snapshot is about 4x33 reads = 15 ms, so a 30 s backstop
         is about 0.05 % of a core.
      5. CLI: wilson-voice --probe-calls prints the current CallSnapshot as JSON (cli.rs, next to --asr-capabilities) and
         exits; it reads, never records. Window titles are PRIVATE (mail subjects, meeting codes): the JSON carries each
         title as { pattern_id: <catalog id or null>, len } and NEVER the text; a --raw-titles flag exists for a developer
         at a terminal and scripts/call-walk.sh NEVER passes it. tests/call_signal_matrix.rs asserts that no fixture has a
         "title" string field. The JSON also records ax_source: a CLI run from Terminal is attributed by TCC to Terminal
         (the responsible process), so it reports TERMINAL's AXIsProcessTrusted, not Yap's — ax_source says so.
      6. Fixtures: desktop/src-tauri/tests/fixtures/call_snapshots/*.json — at least: idle-yap-holds-mic (the measured
         snapshot, plan §2.2), idle-yap-quiet (Yap in=0), zoom-in-call, chrome-meet-in-call, chrome-youtube-playing (output
         only -> no offer), safari-webkit-input-no-title (-> no offer), facetime-daemon-input (-> offer text "FaceTime call
         — record your side?"), yap-dictating-only (-> no offer). Hand-built fixtures say so in a "synthetic": true field
         until a Wilson walk replaces them with recorded ones.
      7. scripts/call-walk.sh (executable, bash -n clean; folded from the cut yap-call-4): prints the walk steps from the
         plan's "Wilson's walk checklist" and runs --probe-calls into tests/fixtures/call_snapshots/<app>.json. It prints
         WHICH Accessibility grant it saw ("Terminal's, not Yap's" when ax_source is terminal); a Terminal-AX run records
         browser_titles as null = "AX unknown" and NEVER flips a browser row to verified. It never passes --raw-titles, and
         it refuses to write a snapshot that contains a title string.
      8. docs/loop/CALL-COVERAGE.md: one row per catalog entry — app, bundle id (verified with defaults read
         <app>/Contents/Info CFBundleIdentifier, or "UNVERIFIED: not installed"), which process carries the call's input
         (from a snapshot taken DURING a real call), detected? (Y/N), offer text, track-1 non-zero? (from the meeting
         diagnostics row), notes. Rows: Zoom (us.zoom.xos), Teams (com.microsoft.teams2), FaceTime (com.apple.FaceTime, audio
         via com.apple.avconferenced), Phone / iPhone calls on Mac (com.apple.mobilephone, via callservicesd), iPhone
         Mirroring (com.apple.ScreenContinuity), Slack, Discord, WhatsApp, Webex, Chrome-family Meet. The builder fills what it
         can verify on disk; every in-call column stays UNVERIFIED until Wilson's walk. FaceTime and Phone rows default to
         "your side only" until a walk shows a non-zero track 1 — never a two-sided claim without it.
      9. tests/call_coverage_ledger.rs: every catalog bundle id has a ledger row; every verified:true row has a non-synthetic
         fixture; a browser row is verified only from a fixture whose ax_source is "yap"; every fixture scores as its row says;
         every system_daemon row with verified:false yields the "your side" copy.

    TESTS: tests/call_signal_matrix.rs — table over every fixture -> expected (bundle_id, offer yes/no, via, offer text), plus
    "no fixture has a title string field". Unit tests: self-exclusion (Yap in=1 never scores), helper attribution via
    parent_pid, Safari capped without a title, output-only never offers, an unreadable property never raises confidence,
    process_listener_diff re-registers on every list change, timer_enabled(false) = off.
    WILSON'S WALK (PR body, UNVERIFIED until run): a 10-minute check that the HAL actually notifies on
    kAudioProcessPropertyIsRunningInput for a process that is ALREADY a HAL client (start a Zoom call, watch the listener log).
    If it does not, worst-case detection latency is the 30 s backstop and the 10 s offer still works.
    WHAT NOT TO DO: no Screen Recording, no CGWindowList, no private API (no responsibility_get_pid_responsible_for_pid), no
    polling under 30 s, no recording, no network, no raw title in any file, log or CLI output by default.
    RUNTIME DEPENDENCIES: | CoreAudio process objects, NSWorkspace | OS | — nothing found on the machine.
  `,
  acceptance: `
    test -f desktop/src-tauri/src/call_signals.rs
    test -f desktop/src-tauri/src/call_catalog.json
    grep -q 'kAudioProcessPropertyIsRunningInput' desktop/src-tauri/src/syscapture.rs
    grep -q 'kAudioProcessPropertyBundleID' desktop/src-tauri/src/syscapture.rs
    grep -rq 'kAudioHardwarePropertyProcessObjectList' desktop/src-tauri/src
    grep -rq 'process_listener_diff' desktop/src-tauri/src
    grep -rq 'timer_enabled' desktop/src-tauri/src
    grep -q 'probe-calls' desktop/src-tauri/src/cli.rs
    grep -q 'raw-titles' desktop/src-tauri/src/cli.rs
    test "$(ls desktop/src-tauri/tests/fixtures/call_snapshots/*.json | wc -l)" -ge 8
    ! grep -rlE '"title"' desktop/src-tauri/tests/fixtures/call_snapshots
    grep -q 'call_signals::' desktop/src-tauri/src/meeting_detect.rs
    grep -q 'your side' desktop/src-tauri/src/call_signals.rs
    ! grep -rnE 'CGWindowListCopyWindowInfo|responsibility_get_pid_responsible_for_pid' desktop/src-tauri/src
    ! grep -rn "auto_record\\|autoRecord" desktop/src desktop/src-tauri/src
    test -f docs/loop/CALL-COVERAGE.md
    grep -q 'com.apple.FaceTime' docs/loop/CALL-COVERAGE.md
    grep -q 'com.apple.mobilephone' docs/loop/CALL-COVERAGE.md
    grep -q 'us.zoom.xos' docs/loop/CALL-COVERAGE.md
    test -x scripts/call-walk.sh
    bash -n scripts/call-walk.sh
    grep -q 'probe-calls' scripts/call-walk.sh
    grep -q 'AX unknown' scripts/call-walk.sh
    ! grep -q -- '--raw-titles' scripts/call-walk.sh
    test -f desktop/src-tauri/tests/call_coverage_ledger.rs
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test call_signal_matrix ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test call_coverage_ledger ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol call_signals ; test $? -eq 0
    cd ${APP}/src-tauri && cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap-call-1', prompt: 'yap-calls', branch: 'loop/yap-call-1-call-detected-record-prompt', gated: 'panel',
  title: 'The pill says "Zoom call — record it?" (or "FaceTime call — record your side?") with a true consent line, offers once and then stays as a one-click chip for the life of the call, Offer / Never per app, and asks "Call ended" — it never starts or stops on its own',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/call_signals.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-call-0 has not landed; land it, then build this"; exit 0; fi; test -f desktop/src/pill/CallOffer.tsx && test -f desktop/src-tauri/tests/call_offer_policy.rs && grep -rq 'call_offer_policy_v1' desktop/src-tauri/src && grep -q 'CONSENT_PILL_LINE' desktop/src/pill/CallOffer.tsx )
  `,
  spec: `
    Panel: APPROVED 2026-10-26 (consent surface). Class SEC — label the PR "security".
    DEPENDS: yap-call-0 (CallCandidate, offer_copy), yap-notetaker-detect (the offer this extends; reuse its pill slot, do not
    add a second one; it also owns the CONSENT_PILL_LINE export this item renders).

    DO (plan §3 as revised)
      1. Offer copy comes from offer_copy() (yap-call-0): "Zoom call — record it?"; "FaceTime call — record your side?" while
         that row is unverified; "Browser call — record it?" with the site line "site unknown" when no title matched. Under it
         ONE consent line: CONSENT_PILL_LINE, exported from desktop/src/meetings/consent.ts, text "Let them know you're
         recording." (the three-paragraph CONSENT_NOTICE is too long for a pill). Buttons [Record] [Not now].
      2. consent.ts:68 is FALSE for this feature today: "Yap records your Mac's microphone … Nothing is uploaded, and no one
         else can read it." With a call, other people's audio is recorded; and it is false again whenever any provider or
         destination is Leaves-this-Mac: Yes. Replace the copy with a function consentNotice(activeLeaves: string[]) that
         (a) names what is recorded — "your microphone and the call audio playing on this Mac", or "your side only" for the
         system_daemon rows — and (b) renders the sentence "Nothing is uploaded" ONLY when activeLeaves is empty, otherwise
         it LISTS the hosts ("Notes go to openrouter.ai"). yap-byom-0 and yap-res-0 feed activeLeaves from their labels.
         vitest consent.test.ts: empty -> "Nothing is uploaded" present; ["openrouter.ai"] -> absent, host named.
      3. Record is the Tauri command accept_call_offer(offer_id): it starts only if offer_id is the LIVE candidate's id (the
         backend checks; the webview cannot start a recording for a stale or invented id). It starts kind Virtual and stores
         detected_app (bundle id) and detected_via (mic_in_use|title|calendar|manual) on the meeting row (one additive,
         nullable migration). tests/call_offer_policy.rs proves NO transition except UserChoice::Record yields a
         StartRecording effect (expiry, withdraw, offer, never, ended).
      4. 10 s countdown ring. Expiry == Not now for THIS call and NEVER means Record — but it must not make the offer vanish
         (a founder in a green room or full screen would miss it): after expiry the pill keeps ONE one-click chip "Record
         <App> call" (meeting.ts recordLabel) for as long as that candidate lives, no sound, no second banner. The chip calls
         accept_call_offer with the same offer_id; it disappears the moment the candidate does. A second call by the same app
         after running_input dropped is a NEW candidate (re-arms).
      5. Per-app memory: settings_kv call_offer_policy_v1 = { bundle_id: "offer" | "never" } (default offer). Exactly two
         states — "always_offer" and "ask" are collapsed (two "offer" states confuse). "Never for <App>" in the expanded pill
         and in Settings > Meetings (a list with Reset). There is no auto-record setting anywhere.
      6. Call ended: while recording a detected call, the detected app's running_input false for 20 s (read off the 1 s
         meeting tick, no new timer) -> pill shows "Call ended — stop recording?" [Stop] [Keep going]. Nothing stops by
         itself (plan open question 2: if Wilson picks auto-stop, that is a follow-up).
      7. The pill never lies: a recording that started from an offer shows the app name in the meeting badge
         (desktop/src/pill/MeetingBadge.tsx); FaceTime/Phone recordings say "your side" there too.
      8. CONTENT PROTECTION (Wilson 2026-10-26): the call-offer chip is hidden from screen capture and screen share —
         the offer surface's NSWindow sharingType = none, via Tauri \`set_content_protected(true)\` on THAT window only.
         The recording dot / pill recording state is NEVER content-protected. If the offer renders inside the pill
         window today, it moves to its own small window (label "call-offer") so protecting it cannot hide the dot.
         One pure fn decides it, \`content_protected_for(label: &str) -> bool\` (true only for "call-offer"), called
         where each window is built; nothing else calls set_content_protected.

    TESTS: tests/call_offer_policy.rs (table: offer -> offer with timer; never -> no offer; expiry -> chip, no recording;
    chip click -> Record with the live id; stale id -> rejected; candidate gone -> offer and chip withdrawn; ended 20 s -> stop
    prompt, no stop; ONLY UserChoice::Record yields StartRecording). vitest: desktop/src/pill/callOffer.test.ts (CONSENT_PILL_LINE
    rendered, countdown 10 s, the chip is present after 10 s, Never persists, FaceTime copy) and consent.test.ts.
    CONTENT PROTECTION TEST (tests/call_offer_policy.rs, no GUI launch): the offer window reports content-protected
    true (content_protected_for("call-offer") == true, and the offer window builder passes it to set_content_protected);
    the pill / recording-dot window reports false (content_protected_for("pill") == false, and the pill window is never
    built with content protection).
    WHAT NOT TO DO: no auto-record, no auto-stop, no sound/notification spam (one offer per call), no polling < 30 s.
    DECIDED (was OWNER-ONLY): Wilson 2026-10-26 — hide the OFFER from screen capture/share (DO 8), never the recording dot.
    WILSON'S WALK (PR body): start a full-screen share in Zoom/Meet, trigger an offer — it is absent from the shared
    view and visible on his screen; start recording — the recording dot IS visible in the shared view.
    Panel revisions: 2026-10-26 Wilson: hide the offer only, never the recording dot.
  `,
  acceptance: `
    test -f desktop/src/pill/CallOffer.tsx
    test -f desktop/src/pill/callOffer.test.ts
    test -f desktop/src/meetings/consent.test.ts
    test -f desktop/src-tauri/tests/call_offer_policy.rs
    grep -rq 'call_offer_policy_v1' desktop/src-tauri/src
    grep -q 'Call ended' desktop/src/pill/CallOffer.tsx
    ! grep -rn "auto_record\\|autoRecord\\|auto_stop\\|autoStop" desktop/src desktop/src-tauri/src
    grep -q 'export const CONSENT_PILL_LINE' desktop/src/meetings/consent.ts
    grep -q 'CONSENT_PILL_LINE' desktop/src/pill/CallOffer.tsx
    grep -q 'consentNotice' desktop/src/meetings/consent.ts
    grep -rq 'accept_call_offer' desktop/src-tauri/src/lib.rs
    sed -n '/generate_handler!\\[/,/\\]/p' desktop/src-tauri/src/lib.rs | grep -q 'accept_call_offer'
    grep -rq 'fn content_protected_for' desktop/src-tauri/src
    grep -rq 'set_content_protected' desktop/src-tauri/src
    test "$(grep -rn 'set_content_protected(true)' desktop/src-tauri/src | wc -l | tr -d ' ')" -le 1
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test call_offer_policy ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'yap-call-2', prompt: 'yap-calls', branch: 'loop/yap-call-2-browser-calls-by-ax-title', gated: 'panel',
  title: 'Browser calls in Chrome-family browsers (Chrome, Arc, Edge, Brave): Meet, Zoom web, Teams web, Slack huddles, Discord, Whereby, Webex — matched by the front tab\'s AX window title only when the browser holds the mic, with permission truth when Accessibility is off',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/call_signals.rs ) || [ "$(gh pr view 247 --json state -q .state 2>/dev/null)" = OPEN ] || [ "$(gh pr view 239 --json state -q .state 2>/dev/null)" = OPEN ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-call-0 has not landed, or PR 247 / PR 239 (both change permissions.rs) are still open; land them, then build this"; exit 0; fi; test -f desktop/src-tauri/src/call_titles.rs && test -f desktop/src-tauri/tests/call_browser_titles.rs )
  `,
  spec: `
    Panel: APPROVED 2026-10-26 for the Chrome family (reads other apps' window titles via Accessibility). Class SEC — label the PR "security".
    DEPENDS: yap-call-0. REBASE: PRs 247 and 239 change permissions.rs (hold in the preflight).
    DEFERRED to the next release: the Safari path (com.apple.WebKit.GPU attribution rests on an unproven title+running rule)
    and Firefox. Until then a Safari/WebKit.GPU input NEVER produces an offer (the safari-webkit-input-no-title fixture stays
    green) and docs/loop/DEFERRED.md gets one line starting "yap-call-2 Safari + Firefox:".

    DO (plan §2.2(3), §3)
      1. call_titles.rs: for a Chromium-family browser that the snapshot attributes running_input to, read AXWindows ->
         kAXTitleAttribute via AXUIElementCreateApplication(pid) (focus.rs already reads AXTitle at :313 — reuse attr_string;
         focus.rs:278 already reads other apps through AX, so this needs NO Screen Recording). Titles only; no AXURL, no
         web-area traversal, no AppleScript/Automation permission, no CGWindowList. Read once per candidate, never on a timer
         of its own; titles are matched in memory and NEVER stored or logged (log the matched pattern id and the length only).
      2. Patterns live in call_catalog.json (yap-call-0) per service, case-insensitive: Meet ("Meet - ", "meet.google.com"),
         Zoom web ("Zoom"), Teams web ("Microsoft Teams", "teams.microsoft.com"), Slack ("Huddle"), Discord ("Discord"),
         Whereby, Webex, Around. Every pattern carries verified:false until a Wilson walk records a real title.
      3. Permission truth: browser_titles = None when AXIsProcessTrusted() is false -> the offer reads "Browser call (site
         unknown)" and only fires with a calendar match (plan §3); Settings > Meetings shows one line "Turn on Accessibility
         so Yap can tell which site the call is on" linking the existing permissions flow. Never prompt for Accessibility
         from here.
      4. Honest limit, stated in Settings: Chrome's window title is the FRONT tab, so a Meet in a background tab gets no title
         match and no offer. That is the safe failure. Settings > Meetings says "Yap reads the front tab's title; a call in a
         background tab is not offered".

    TESTS: tests/call_browser_titles.rs — fixtures (title lists) -> matched service; AX None -> "site unknown" path; YouTube
    tab with mic open -> no offer; a Meet title in a background tab (not the front title) -> no offer; Safari WebKit.GPU input
    -> no offer; a title is never written to the DB or log (grep the log sink in the test; only pattern id + len appear).
  `,
  acceptance: `
    test -f desktop/src-tauri/src/call_titles.rs
    test -f desktop/src-tauri/tests/call_browser_titles.rs
    ! grep -rnE 'CGWindowListCopyWindowInfo|NSAppleScript|AXURL' desktop/src-tauri/src/call_titles.rs
    grep -q 'site unknown' desktop/src-tauri/src/call_titles.rs desktop/src/pill/*.tsx
    grep -rq 'front tab' desktop/src/views desktop/src/settings
    grep -q 'yap-call-2 Safari' docs/loop/DEFERRED.md
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test call_browser_titles ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test call_signal_matrix ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'yap-call-3', prompt: 'yap-calls', branch: 'loop/yap-call-3-per-app-tap-spike-macos26', gated: 'panel',
  title: 'DEFERRED to the next release (UNAPPROVED): SPIKE on macOS 26 — capture only the call app (CATapDescription.bundleIDs, exclusive = false) behind a default-OFF setting, or record NO-GO with evidence and keep global-exclude-self',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/call_signals.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-call-0 has not landed; land it, then build this"; exit 0; fi; head -1 docs/loop/evidence/yap-call-3.md | grep -qE '^SPIKE-VERDICT yap-call-3 (GO|NO-GO)$' && test -f desktop/src-tauri/tests/tap_app_scoped_plan.rs )
  `,
  spec: `
    Panel 2026-10-26: DEFERRED to the next release and NOT in panelApproved. Seat B: cut until after 100 users (it re-opens the
    settled OS-5 decision, is macOS-26-only, default OFF and NO-GO "until Wilson walks it", so no user can see it pass). Seat A:
    keep, with the inverted-tap fix below. SETTLED: defer, but keep this spec CORRECT so it is right when it is built. The
    setting stays default OFF; global-exclude-self stays the default on every OS.
    Panel: REQUIRED — this re-opens the panel's OS-5 decision (syscapture.rs:13-31: global-exclude-self only, never
    app-scoped). Class SEC — label the PR "security".
    DEPENDS: yap-call-0 (which app is in the call).

    WHY: macOS 26 adds CATapDescription.bundleIDs ("the bundle ID of a process to tap or exclude") and processRestoreEnabled
    ("save tapped processes by bundle ID when they exit, and restore them … when they start up again"), both
    API_AVAILABLE(macos(26.0)) (SDK CATapDescription.h:131-167). That may answer OS-5's helper-process problem (the UI pid
    is silent) without pid resolution — Spotify and chimes would stop landing in track 1.

    THE INVERSION TRAP (seat A, grounded in CATapDescription.h): "exclusive" means "tap all processes EXCEPT the process
    listed", and bundleIDs means "tap or exclude". Setting bundleIDs on today's initMonoGlobalTapButExcludeProcesses
    description would therefore tap EVERYTHING EXCEPT the call app — the exact opposite.

    DO
      1. Pure plan: TrackBPlan gains AppScoped { bundle_ids } chosen ONLY when: macOS >= 26.0, setting
         meeting_app_scoped_tap (default OFF) is on, and a CallCandidate exists. tests/tap_app_scoped_plan.rs proves every
         other combination returns today's plan unchanged.
      2. Construction (correct form): build the AppScoped description with initMonoMixdownOfProcesses:@[] and THEN
         setExclusive(false) and THEN setBundleIDs([the app's bundle id + its catalog helper ids]); setProcessRestoreEnabled(true).
         Global keeps initMonoGlobalTapButExcludeProcesses with exclusive == true. Resolve the setters via objc2 msg_send
         with respondsToSelector so the binary still loads on 14.4 (objc2-core-audio 0.3.2 has setBundleIDs/
         setProcessRestoreEnabled at AudioHardware.rs:2007/2055; same weak-link rule as YV101). tests/tap_app_scoped_plan.rs
         asserts exclusive == false for AppScoped and exclusive == true for Global.
      3. EXCLUDE Safari from app-scoping: the com.apple.WebKit.GPU bundle id taps every WKWebView process, Yap's own webview
         included — a Safari call stays on the global plan.
      4. The silence discriminator must survive: TapLiveness::ever_nonzero (syscapture.rs:196) still separates denial from
         wedge; an app-scoped tap that never delivers a non-zero sample within the existing grace falls back to the global
         plan for that meeting and logs why. Never two taps at once.
      5. Evidence: docs/loop/evidence/yap-call-3.md, FIRST LINE exactly "SPIKE-VERDICT yap-call-3 GO" or
         "SPIKE-VERDICT yap-call-3 NO-GO", then WILSON'S WALK results (Chrome Meet, Zoom, a YouTube tab playing at the same
         time): per-track RMS from the meeting diagnostics row, with and without the setting. Until Wilson walks it, the
         verdict is NO-GO and the setting stays hidden.

    WHAT NOT TO DO: no default change, no ScreenCaptureKit, no second concurrent tap, no change below macOS 26, no bundleIDs
    on an exclusive description.
  `,
  acceptance: `
    head -1 docs/loop/evidence/yap-call-3.md | grep -qE '^SPIKE-VERDICT yap-call-3 (GO|NO-GO)$'
    test -f desktop/src-tauri/tests/tap_app_scoped_plan.rs
    grep -q 'meeting_app_scoped_tap' desktop/src-tauri/src/syscapture.rs desktop/src-tauri/src/meeting_control.rs
    grep -q 'initMonoGlobalTapButExcludeProcesses' desktop/src-tauri/src/syscapture.rs
    grep -q 'initMonoMixdownOfProcesses' desktop/src-tauri/src/syscapture.rs
    grep -q 'setExclusive' desktop/src-tauri/src/syscapture.rs
    grep -q 'exclusive' desktop/src-tauri/tests/tap_app_scoped_plan.rs
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test tap_app_scoped_plan ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_availability_144_gate ; test $? -eq 0
    bash scripts/assert-weak-linked-14_4-symbols.sh
  `,
})

// TOMBSTONE yap-call-4 (panel 2026-10-26, FOLDED, not dropped): its deliverables moved into yap-call-0 — scripts/call-walk.sh
// (no raw titles, prints which AX grant it saw, never verifies a browser row on a Terminal-AX run), docs/loop/CALL-COVERAGE.md,
// tests/call_coverage_ledger.rs, and the FaceTime/Phone "your side only" default copy. The in-call walk is now WILSON'S WALK CHECKLIST,
// a section in docs/plans/yap-calls-and-byom-2026-10-26.md. Reason (seat B): its acceptance passed with every row UNVERIFIED, so
// it proved nothing as a loop item; the machinery is cheap, the walk is a human act.

ITEMS.push({
  id: 'yap-byom-0', prompt: 'yap-byom', branch: 'loop/yap-byom-0-provider-model-keychain-settings', gated: 'panel',
  title: 'Bring your own model, the frame: a provider model (Bundled | OpenAI-compatible), per-task choice for notes and polish, API keys in the macOS Keychain bound to their host, one redirect-free HTTP client factory, Test connection, and a "Leaves this Mac" label on every choice that is TRUE',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/summary_jobs.rs ) || [ "$(gh pr view 246 --json state -q .state 2>/dev/null)" = OPEN ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 250 (default meeting notes, summary_jobs.rs) has not landed, or PR 246 (Meetings UI: progress.ts, Meetings.tsx) is still open; land them, then build this"; exit 0; fi; test -f desktop/src-tauri/src/model_providers.rs && test -f desktop/src-tauri/src/keychain.rs && test -f desktop/src-tauri/src/http_client.rs && test -f desktop/src-tauri/tests/model_providers_contract.rs && test -f desktop/src-tauri/tests/key_sentinel_leak.rs && test -f desktop/src/settings/ModelProviders.tsx )
  `,
  spec: `
    Panel: APPROVED 2026-10-26 (secrets + the product's privacy promise). Class SEC — label the PR "security".
    DEPENDS: PR 250 (summary_jobs.rs NotesBackend; the bundled 1.5B notes model stays the default). REBASE: PR 250 touches
    lib.rs/meetings.rs/NotesView; PR 246 touches progress.ts and Meetings.tsx (hold in the preflight).

    DO (plan §2.3, §5 as revised)
      1. model_providers.rs: ProviderKind { Bundled, OpenAiCompatible } (the Anthropic kind arrives with the deferred
         yap-byom-2; the UI HIDES any kind that is not wired — no "Coming in this release" placeholder, a release can break that
         promise), ProviderConfig { id, kind, label, base_url, model, context_tokens (default 4096), timeout_s }, trait
         ChatProvider { complete(system, user, max_tokens, cancel) -> Result<String, ProviderError>; leaves_this_mac() -> bool }.
         leaves_this_mac is false for Bundled and for a base_url whose host is 127.0.0.1 / ::1 / localhost, true otherwise —
         EXCEPT a loopback model name containing "-cloud" or ":cloud" (Ollama cloud models run on ollama.com THROUGH localhost,
         docs.ollama.com/cloud): those are true and the label reads "Yes — ollama.com". ProviderError has plain-words reasons
         (unreachable, timeout, 401 key rejected, 404 model not found, 429 rate limited, 5xx server error, redirected to
         <host> — not followed, bad response).
      2. Settings: providers list in settings_kv model_providers_v1 (NO key in it); notes_provider and polish_provider ids
         (empty = Bundled). polish_model keeps its meaning (the bundled-model override). Bundled is always present and
         cannot be deleted.
      3. keychain.rs: security-framework passwords API (already in desktop/Cargo.lock 3.7.0 via native-tls; declare it
         directly). Service name is derived from the RUNNING bundle id, com.wilsonguenther.wilson-voice.byom for the shipped
         app and the Yap Test bundle's own for the test channel (same rule as PR 248), so a test build never reads or deletes
         the real keys. Account = provider id + host. Changing a provider's base_url HOST deletes the key and requires
         re-entry. set / get / delete. Keys never reach SQLite, settings, logs, crash reports or the frontend after save (the UI
         shows "Key saved" + last 4 chars). Deleting a provider deletes its Keychain item.
      4. http_client.rs: the ONE reqwest client factory every outbound BYOM and destination call goes through (byom-1, the
         deferred byom-2, and the res-0 webhook). redirect::Policy::none() — reqwest 0.13.5 defaults to limited(10)
         (redirect.rs:161) and strips only Authorization/Cookie across hosts (:239-249), so a 307/308 would re-POST the
         transcript to an unlabelled host and forward x-api-key with it. A 3xx becomes ProviderError/DestError "redirected to
         <host> — not followed". One keep-alive Client per provider (cold TLS inside a short deadline loses). RULE for any
         later change: nothing may follow a redirect; if one ever does it must strip x-api-key, api-key, Authorization and
         anthropic-workspace-id across hosts. This is the outbound allowlist test.
      5. desktop/src/settings/ModelProviders.tsx, reached from views/Settings.tsx: rows with label, kind, model, "Leaves this
         Mac: Yes — <host>" / "No", Test connection, Edit, Delete; two pickers "Notes use" / "Polish uses". A summary line at
         the top when any Yes is active ("Notes go to openrouter.ai"). Copy says plainly what leaves: notes = the meeting
         transcript; polish = every dictation. The active Yes labels feed consentNotice(activeLeaves) in consent.ts (yap-call-1)
         so "Nothing is uploaded" disappears the moment any Yes is active.
      6. Test connection: the Tauri command test_model_provider(provider_id) — it takes the provider ID only, NEVER a URL or key
         from the frontend, so a webview bug cannot aim a stored key at a new host. A 10-token completion (max_tokens 10) with a
         10 s timeout; shows latency and the model's reply or the ProviderError sentence. Registered in generate_handler!.
      7. In this item only Bundled is implemented behind ChatProvider (wrapping the existing sidecar path); OpenAI-compatible is
         wired by yap-byom-1 and is hidden until then — never a fake success.

    TESTS: tests/model_providers_contract.rs — leaves_this_mac table (loopback = No, LAN IP = Yes, https host = Yes, loopback
    with model "gpt-oss:120b-cloud" and "kimi-k2:cloud" = Yes — ollama.com); settings round-trip never contains a key; keychain
    set/get/delete behind a trait with an in-memory fake (no real Keychain write in CI), account = id + host, a host change
    deletes the key, the service name follows the bundle id; Bundled cannot be deleted.
    tests/key_sentinel_leak.rs — save the key "sk-YAPSENTINEL-…" through the fake keychain, run Test connection against the
    127.0.0.1 mock, assert the sentinel is ABSENT from the log sink, settings_kv, every emitted event payload and the command's
    return value (last 4 only). tests/http_client_no_redirect.rs — the mock returns 307 to a SECOND 127.0.0.1 port; that port
    receives ZERO connections and the call ends "redirected to <host> — not followed". vitest ModelProviders (label text,
    pickers, key never rendered beyond last 4, unwired kinds hidden) and consent.test.ts (a Yes label removes "Nothing is uploaded").
    RUNTIME DEPENDENCIES: | Security.framework (Keychain) | OS; security-framework crate ships |.
  `,
  acceptance: `
    test -f desktop/src-tauri/src/model_providers.rs
    test -f desktop/src-tauri/src/keychain.rs
    test -f desktop/src-tauri/src/http_client.rs
    test -f desktop/src/settings/ModelProviders.tsx
    grep -q 'com.wilsonguenther.wilson-voice.byom' desktop/src-tauri/src/keychain.rs
    grep -q 'Policy::none' desktop/src-tauri/src/http_client.rs
    grep -q 'Leaves this Mac' desktop/src/settings/ModelProviders.tsx
    grep -q 'ollama.com' desktop/src-tauri/src/model_providers.rs
    ! grep -rn 'Coming in this release' desktop/src
    sed -n '/generate_handler!\\[/,/\\]/p' desktop/src-tauri/src/lib.rs | grep -q 'test_model_provider'
    test -f desktop/src-tauri/tests/key_sentinel_leak.rs
    grep -q 'YAPSENTINEL' desktop/src-tauri/tests/key_sentinel_leak.rs
    test -f desktop/src-tauri/tests/http_client_no_redirect.rs
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test model_providers_contract ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test key_sentinel_leak ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test http_client_no_redirect ; test $? -eq 0
    cd ${APP}/src-tauri && cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap-byom-1', prompt: 'yap-byom', branch: 'loop/yap-byom-1-openai-compatible-provider', gated: 'panel',
  title: 'OpenAI-compatible provider behind notes and polish: Ollama, LM Studio, OpenRouter and custom presets, "Find local models", a context-safe map/reduce, and a truthful fallback to the built-in model that never starts a silent download',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/model_providers.rs ) || [ "$(gh pr view 246 --json state -q .state 2>/dev/null)" = OPEN ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-byom-0 has not landed, or PR 246 (progress.ts, Meetings.tsx; the meeting card shows which provider wrote the notes) is still open; land them, then build this"; exit 0; fi; test -f desktop/src-tauri/src/provider_openai.rs && test -f desktop/src-tauri/tests/provider_openai_compat.rs && test -f desktop/src-tauri/tests/notes_provider_fallback.rs && test -f desktop/src-tauri/tests/byom_bundled_zero_network.rs && test -f desktop/src-tauri/tests/notes_prompt_identity.rs )
  `,
  spec: `
    Panel: APPROVED 2026-10-26 (outbound network carrying transcripts and dictations). Class SEC — label the PR "security".
    DEPENDS: yap-byom-0, PR 250 (the sidecar prompts and AppNotes::model_ready live there).

    DO (plan §2.3 as revised)
      1. provider_openai.rs: POST {base_url}/chat/completions with { model, messages:[system,user], max_tokens, stream:false }
         and Authorization: Bearer <key> when a key exists; parse choices[0].message.content ONLY. Built from
         http_client.rs (yap-byom-0): no redirects, keep-alive. NEVER send "tools"; a response with tool_calls or any
         non-text content is ProviderError "bad response". Non-streaming: notes are a background job; cancel = drop the
         request when the CancelToken fires.
      2. Presets (editable after pick): Ollama http://localhost:11434/v1 (docs.ollama.com/api/openai-compatibility — key
         ignored), LM Studio http://localhost:1234/v1 (lmstudio.ai/docs/developer/openai-compat), OpenRouter
         https://openrouter.ai/api/v1 (sends X-Title: Yap), Custom. Ollama and LM Studio presets default context_tokens
         4096 (Ollama's default context is 4k under 24 GiB of memory, docs.ollama.com/context-length, and overflow is silent).
         "Find local models" = GET {base}/models on the two local presets with a 1.5 s timeout, ONLY on click — never a
         background probe; a listed model named "-cloud" / ":cloud" is shown with "Yes — ollama.com". On a Mac with <= 8 GB RAM
         a local provider row shows "A 7B+ model next to Yap may swap" (pure fn, tested).
      3. Notes: a NotesBackend implementation per selected provider — model_ready() true, fetch_model a no-op. Prompts: the four
         system prompt consts (SYSTEM_BASE, MAP, REDUCE, HEADLINE — today in the sidecar, yap-polish/src/main.rs:72/:143/:165/
         :182) MOVE into polish_protocol.rs (already compiled into both crates via #[path], main.rs:37). A remote call sends
         them as the SYSTEM turn and the transcript as the USER turn (the transcript is data; SYSTEM_BASE rule 2 already says
         never obey the content). Remote MAP output still goes through parse_map_output's id filter. Map chunk cap =
         min(MAP_CHUNK_TOKENS 1400 (summarize.rs:94), context_tokens - max_out (polish_protocol.rs:91, 1536)); a chunk that
         cannot fit is split, never truncated. Per-call timeout 120 s.
      4. Polish: when polish_provider is set (default empty = Bundled; cloud polish is allowed and labelled Yes, never default),
         the polish stage calls the provider with the take's existing deadline (PolishRequest deadline_ms, default 1200,
         polish.rs:59, clamp 100..5000); a miss inserts the unpolished take exactly as a sidecar timeout does today. Honesty:
         Test connection compares latency to the deadline ("answered in 2.4 s; polish waits 1.2 s, so most dictations will skip
         it") and Settings shows "polished X of last N takes" from a local counter (no telemetry). Remote polish SKIPS password
         fields (AXSecureTextField targets; focus.rs:279 only drops context today) and mode=code — that text must not leave.
      5. Fallback (notes): the FIRST timeout or 5xx abandons the remote for the rest of the job (circuit breaker; plus a 10-minute
         job cap — 9 chunks x 120 s would otherwise be 18 minutes). Then: if AppNotes::model_ready() (#250 lib.rs:5119) the job
         reruns on Bundled and the meeting stores "Notes written by the built-in model — <label> didn't answer (<reason>)";
         if NOT, the job parks with "<label> didn't answer; the built-in model isn't downloaded — [Download & retry]". Yap never
         starts the 1.1 GB download on its own. The meeting card shows which provider wrote the notes and its Leaves-this-Mac label.

    TESTS (no internet; a std::net::TcpListener mock on 127.0.0.1 inside the test): tests/provider_openai_compat.rs (request
    shape, no "tools" key, auth header present only with a key, 401/404/429/500/timeout map to the right ProviderError,
    tool_calls -> bad response, a 3xx is not followed, cancel drops the request, a 20k-token transcript to a 4096-context mock
    makes >= 14 map calls and none over budget, a 2048-context mock yields smaller chunks); tests/notes_prompt_identity.rs (the
    four prompts sent remotely are BYTE-IDENTICAL to the sidecar's, system/user framing); tests/notes_provider_fallback.rs
    (provider 500 with model ready -> Bundled stub writes notes + banner; provider 500 with NO model -> parked "Download & retry",
    zero download started; the first timeout opens the breaker); tests/byom_bundled_zero_network.rs (every provider Bundled ->
    notes + polish make ZERO connections, deny-all seam; this replaces the old "if meeting_notes_stay_local exists" line, which
    was vacuous until yap-notetaker-notes landed); remote polish skipped for a secure-field fixture and for mode=code.
  `,
  acceptance: `
    test -f desktop/src-tauri/src/provider_openai.rs
    grep -q 'localhost:11434/v1' desktop/src-tauri/src/provider_openai.rs desktop/src/settings/ModelProviders.tsx
    grep -q 'localhost:1234/v1' desktop/src-tauri/src/provider_openai.rs desktop/src/settings/ModelProviders.tsx
    grep -q 'openrouter.ai/api/v1' desktop/src-tauri/src/provider_openai.rs desktop/src/settings/ModelProviders.tsx
    grep -q 'http_client::' desktop/src-tauri/src/provider_openai.rs
    ! grep -rnE 'reqwest::Client::(new|builder)' desktop/src-tauri/src/provider_openai.rs
    grep -q 'SYSTEM_BASE' desktop/src-tauri/src/polish_protocol.rs
    test -f desktop/src-tauri/tests/notes_prompt_identity.rs
    test -f desktop/src-tauri/tests/byom_bundled_zero_network.rs
    grep -rq 'Download & retry' desktop/src
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test provider_openai_compat ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test notes_prompt_identity ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test notes_provider_fallback ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test byom_bundled_zero_network ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'yap-byom-2', prompt: 'yap-byom', branch: 'loop/yap-byom-2-anthropic-native-provider', gated: 'panel',
  title: 'DEFERRED to the next release (UNAPPROVED): Anthropic provider on the native Messages API, built on the shared redirect-free client with x-api-key never leaving its host, same fallback and labels',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/model_providers.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-byom-0 has not landed; land it, then build this"; exit 0; fi; test -f desktop/src-tauri/src/provider_anthropic.rs && test -f desktop/src-tauri/tests/provider_anthropic.rs )
  `,
  spec: `
    Panel 2026-10-26: DEFERRED to the next release and NOT in panelApproved (seat B: OpenRouter and Custom already reach Claude, a
    second HTTP client is not first-session wow). Seat A's rule is carried so it holds the day this is built: yap-byom-0's
    http_client.rs factory (redirect::Policy::none()) ALREADY covers it.
    Panel: REQUIRED (outbound network). Class SEC — label the PR "security".
    DEPENDS: yap-byom-0 (the factory, keychain, labels) and yap-byom-1's fallback path (share it, do not copy it).

    DO (plan §2.3)
      1. provider_anthropic.rs: POST https://api.anthropic.com/v1/messages with headers x-api-key, anthropic-version (one
         pinned constant, read from https://platform.claude.com/docs/en/api/overview at build time and cited in a comment),
         content-type application/json, and anthropic-workspace-id when the user filled the optional Workspace field. Body
         { model, max_tokens, system, messages:[{role:user}] }; text = concatenated content[].text blocks; any tool_use or
         non-text block -> "bad response". No "tools", no temperature/top_p. The client comes from http_client.rs ONLY: a 3xx is
         never followed, so x-api-key and anthropic-workspace-id cannot cross hosts.
      2. "Load models" = GET /v1/models on click only. Model id is free text with the loaded list as suggestions.
      3. Same NotesBackend + polish wiring, timeouts, circuit breaker, fallback and the Leaves-this-Mac: Yes —
         api.anthropic.com label as yap-byom-1; the Keychain account is provider id + host as in yap-byom-0.
    TESTS: tests/provider_anthropic.rs over a 127.0.0.1 mock: headers present, workspace header only when set, error mapping
    (401, 404, 429 with retry-after, 529/5xx, timeout), multi-block text joined, cancel drops the request, and a mock 307 to a
    second 127.0.0.1 port: that port receives zero connections (so no x-api-key arrives there).
  `,
  acceptance: `
    test -f desktop/src-tauri/src/provider_anthropic.rs
    grep -q 'anthropic-version' desktop/src-tauri/src/provider_anthropic.rs
    grep -q 'api.anthropic.com/v1/messages' desktop/src-tauri/src/provider_anthropic.rs
    grep -q 'http_client::' desktop/src-tauri/src/provider_anthropic.rs
    ! grep -rnE 'reqwest::Client::(new|builder)' desktop/src-tauri/src/provider_anthropic.rs
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test provider_anthropic ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'yap-res-0', prompt: 'yap-residency', branch: 'loop/yap-res-0-destinations-folder-webhook', gated: 'panel',
  title: 'Where your notes live: a Markdown+JSON folder (Obsidian-ready, edit-safe, with a true "Leaves this Mac" label for iCloud, Obsidian-iCloud and CloudStorage folders) and a signed webhook with a persisted outbox — cloud SaaS connectors and the SQLite file are deferred',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/summary_jobs.rs && test -f desktop/src-tauri/src/keychain.rs && test -f desktop/src-tauri/src/http_client.rs ) || [ "$(gh pr view 246 --json state -q .state 2>/dev/null)" = OPEN ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 250 and yap-byom-0 (keychain.rs, http_client.rs) have not landed, or PR 246 (progress.ts, Meetings.tsx; the card badges) is still open; land them, then build this"; exit 0; fi; test -f desktop/src-tauri/src/destinations.rs && test -f desktop/src-tauri/tests/destinations.rs && grep -q 'notes_mirror_dir' desktop/src-tauri/src/lib.rs && grep -q 'Mobile Documents' desktop/src-tauri/src/destinations.rs )
  `,
  spec: `
    Panel: APPROVED 2026-10-26 for the folder and the webhook (writes user data outside the app; outbound webhook). Class SEC —
    label the PR "security".
    DEPENDS: PR 250 (notes + export_meeting_markdown, lib.rs:5330; summary_jobs.rs), yap-byom-0 (keychain.rs for the webhook
    secret; http_client.rs for the redirect-free client). REBASE: PR 250 and PR 246 (hold in the preflight).
    ABSORBS yap24-X1: use the setting key notes_mirror_dir for the folder destination so X1's preflight passes and X1
    retires as already-done. ABSORBS the cut yap-res-1 (the iCloud label, as a path classifier — below).
    DEFERRED to the next release: the SQLite destination (seat B: the schema is a contract forever; seat A's rules are kept for
    when it is built: never set WAL on a user's file — it persists in the file and WAL plus a sync folder corrupts, sqlite.org/wal.html —
    and refuse paths under com~apple~CloudDocs / Mobile Documents). One line in docs/loop/DEFERRED.md.

    DO (plan §2.4 as revised)
      1. destinations.rs: trait Destination { kind, leaves_this_mac(), deliver(&MeetingExport) -> Result<(), DestError> }, driven
         by its OWN single worker thread fed from a PERSISTED outbox (table destination_outbox: id, meeting_id, destination_id,
         notes_revision, delivery_id, attempt, next_at, state; one additive migration). It must NEVER run on the one wv-summary
         worker (summary_jobs.rs:236-260): retries of 5 s / 30 s / 2 min on that thread would park every later meeting's notes
         for about 2.6 minutes. Notes never wait on a delivery; a quit mid-retry resumes from the outbox on next launch. Never
         concurrently with dictation. Per-destination last result on the meeting card ("Saved to Obsidian · Webhook failed: 401").
      2. Folder: <folder>/Yap Meetings/<date> <title>.md (front matter: date, duration, kind, detected_app, attendees if known)
         + a JSON sidecar <same name>.json { id, title, started_at, segments[], summary, actions[] } — schema documented in
         docs/destinations.md. Atomic (temp file + fsync + rename), never deletes a user file, a missing folder pauses with one
         sentence in Settings. The TITLE comes from the model's headline pass, which a transcript can steer ("../../Library/
         LaunchAgents/x"), so: safe_file_stem() strips / \\ : NUL and every control character, collapses leading dots, caps at
         120 BYTES on a char boundary, falls back to "Untitled meeting" when empty; the resolved path is canonicalised and
         asserted to be INSIDE the folder before any write; every YAML front-matter value is double-quoted and escaped.
      3. Edit-safe rewrite: store the sha256 Yap last wrote per (meeting, destination). When notes change, if the on-disk file's
         hash differs (or a file Yap did not write sits at that name) never overwrite — write "<name> (Yap update).md" and show
         on the card "Edited in Obsidian — kept yours".
      4. Privacy label (absorbed from yap-res-1) — a path classifier on the CANONICAL folder path (symlinks resolved): anything
         under ~/Library/Mobile Documents (this includes iCloud Drive's com~apple~CloudDocs AND Obsidian's iCloud vaults,
         iCloud~md~obsidian) -> "Leaves this Mac: Yes — Apple iCloud"; ~/Library/CloudStorage/<Provider>-<account> ->
         "Yes — <Provider>" (the part before the first "-": OneDrive, GoogleDrive, Dropbox, Box); anything else -> "No".
         A preset "iCloud Drive / Yap" at ~/Library/Mobile Documents/com~apple~CloudDocs/Yap/ is shown only when that directory
         exists, otherwise "iCloud Drive is off on this Mac". No entitlement, no CloudKit, never sandboxed. Evicted files
         ("Optimize Mac Storage"): a rewrite writes a fresh file via temp+rename and NEVER reads the evicted one first; a .icloud
         placeholder is never touched or deleted. WILSON'S WALK (PR body, UNVERIFIED): enable it, finish a meeting, open Files
         on the iPhone > iCloud Drive > Yap and see the note; and note whether the first write to iCloud Drive raised a Files &
         Folders permission prompt.
      5. Webhook: POST the same MeetingExport JSON to an https URL (plain http only for 127.0.0.0/8, ::1, localhost), through
         http_client.rs (no redirects: a 3xx is "redirected to <host> — not followed"). Headers: X-Yap-Timestamp (unix
         seconds), X-Yap-Signature: sha256=<hex HMAC-SHA256(secret, timestamp + "." + body)> — this is the Stripe-style
         timestamp.body scheme; it is NOT GitHub's body-only X-Hub-Signature-256, so do not call it that anywhere —
         X-Yap-Delivery: <uuid> (the outbox delivery_id, STABLE across retries of the same delivery; each retry re-signs with a
         FRESH timestamp) and X-Yap-Event-Id: <meeting id>:<notes revision> (so a receiver can dedupe a re-fire on notes edit).
         The secret is 32 random bytes generated by Yap, stored in the Keychain, shown ONCE with Copy. docs/destinations.md
         gives the 300 s replay window and a ten-line verify snippet; a test runs that snippet's logic. 3 retries (5 s, 30 s,
         2 min) from the outbox, 10 s timeout, then "failed" on the card with a Retry; "Send test event". Leaves this Mac:
         Yes — <host> (No for loopback).
      6. Settings > Meetings > "Where notes go": add/remove destinations, each row with its label; meetings only (dictation
         log is a follow-up). The active Yes labels feed consentNotice(activeLeaves) (yap-call-1's consent.ts).
      7. docs/loop/DEFERRED.md: add — Notion, Google Drive, Dropbox, generic Postgres destinations deferred to after the first 100
         users, built from scratch (no Nango); and a line starting "Yap SQLite destination:" deferred to the next release with
         the no-WAL / no-iCloud rules above.
    TESTS: tests/destinations.rs — folder atomic write + rewrite + never-delete; safe_file_stem over "../../x" and
    "a\\n---\\nb" and a 300-byte title; path-inside-folder check; front matter quotes a hostile value; external edit -> "(Yap
    update).md" and the user's bytes unchanged; classifier over THREE real shapes (~/Library/Mobile Documents/com~apple~
    CloudDocs/x, ~/Library/Mobile Documents/iCloud~md~obsidian/Documents/Vault, ~/Library/CloudStorage/OneDrive-Personal/x
    -> Yes iCloud, Yes iCloud, Yes OneDrive; ~/Documents/x -> No; a symlink into iCloud resolves to Yes); the preset hidden
    when the dir is absent (temp HOME); .icloud placeholders untouched; JSON sidecar validates against the documented fields;
    webhook over a 127.0.0.1 mock: signature verifies with the secret and a 300 s-old timestamp is rejected by the snippet,
    retries on 500 with the SAME X-Yap-Delivery and a NEW signature, gives up after 3, a 307 to a second port gets zero
    connections, no secret in logs; a slow webhook does not delay the summary worker (notes finish first); outbox survives a
    restart (resume from the table); leaves_this_mac table.
    RUNTIME DEPENDENCIES: | reqwest, sha2, hmac (new small crate) | ship |. (rusqlite is NOT added: the SQLite destination is deferred.)
  `,
  acceptance: `
    test -f desktop/src-tauri/src/destinations.rs
    test -f docs/destinations.md
    grep -q 'notes_mirror_dir' desktop/src-tauri/src/lib.rs
    grep -q 'X-Yap-Signature' desktop/src-tauri/src/destinations.rs
    grep -q 'X-Yap-Delivery' desktop/src-tauri/src/destinations.rs
    ! grep -rniE 'x-hub-signature|github' desktop/src-tauri/src/destinations.rs docs/destinations.md
    grep -q 'X-Yap-Signature' docs/destinations.md
    grep -q '300' docs/destinations.md
    grep -q 'http_client::' desktop/src-tauri/src/destinations.rs
    grep -q 'safe_file_stem' desktop/src-tauri/src/destinations.rs
    grep -q 'destination_outbox' desktop/src-tauri/src/db.rs
    grep -q 'Mobile Documents' desktop/src-tauri/src/destinations.rs
    grep -q 'CloudStorage' desktop/src-tauri/src/destinations.rs
    grep -q 'Yap update' desktop/src-tauri/src/destinations.rs
    grep -rq 'Apple iCloud' desktop/src
    grep -qi 'notion' docs/loop/DEFERRED.md
    grep -q 'Yap SQLite destination' docs/loop/DEFERRED.md
    ! grep -rniE 'nango' desktop/src-tauri/src desktop/src
    ! grep -rn 'rusqlite' desktop/src-tauri/src/destinations.rs
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test destinations ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib notes_mirror ; test $? -eq 0
    cd ${APP}/src-tauri && cargo clippy --all-targets --features custom-protocol
  `,
})

// TOMBSTONE yap-res-1 (panel 2026-10-26, FOLDED, not dropped): the iCloud Drive mirror is now a preset of yap-res-0's folder
// destination and its label rule is yap-res-0's path classifier, widened by both seats: anything under ~/Library/Mobile Documents
// (CloudDocs AND iCloud~md~obsidian vaults) -> "Yes — Apple iCloud"; ~/Library/CloudStorage/<Provider>-* -> "Yes — <Provider>"; else
// No; .icloud placeholders never touched; tests over all three. Reason: res-0 would have shipped "No" for an iCloud-synced Obsidian
// vault until res-1 landed (a false label). UNVERIFIED: the Files & Folders / iCloud Drive first-write permission prompt (WILSON'S WALK).
