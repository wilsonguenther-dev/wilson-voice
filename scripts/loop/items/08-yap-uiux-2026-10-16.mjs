// yap-perm-hotkey-2026-10-16 — THE BANNER WAS RIGHT, SETTINGS LIED; THE HOTKEY STARTED A TAKE IT
// COULD NOT STOP. Written 2026-10-16 by the yap-perm-audit seat from Wilson's 0.9.0 report (16:14Z):
// System Settings shows Yap ON in Accessibility while Yap's home says "Accessibility and Input
// Monitoring are off"; and a hotkey take that 15 fn presses would not end. He asked for tools that
// test it for real. FIX, no panel (unambiguous intent). These three items sit at the TOP of
// 08-yap-uiux-2026-10-16.mjs: one file = one lane, run in order, so all three run ahead of
// yap-uiux-2026-10-16-W0 (two adjacent files would land on opposite lanes and run concurrently). Order inside the file: the harness first (it is
// the acceptance tool of yap-hotkey-stop), then perm-truth, then hotkey-stop.
// Review: Opus only (review-opus + review-opus2). No Codex, no Gemini, no third-party reviewer.
// HAZARD (same as 07): no agent launches the GUI app, posts events into, signals or quits Wilson's
// running /Applications/Yap.app, or writes TCC (no tccutil, no sqlite writes). Every automated proof
// is a cargo test, a vitest, or a script self-test. SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.
//
// DIAGNOSIS (observed on Wilson's machine 2026-10-16, read-only):
//   - TCC rows for com.wilsonguenther.wilson-voice: kTCCServiceAccessibility (2026-07-22 20:35Z) and
//     kTCCServiceListenEvent (20:36Z) store csreq = identifier + leaf CN "Apple Development: Wilson
//     Guenther (U8BP8Z86T2)". Yap 0.9.0 is signed "Developer ID Application: Wilson Guenther
//     (VHYV8C2JNU)" (cdhash 5c551c8a...). codesign -v -R <stored csreq> FAILS for 0.9.0 and PASSES for
//     the 0.8.0 backup (Apple Development, cdhash 9f5b8aa7...). The Settings row is keyed by bundle id,
//     so it shows ON; the OS answers false for this binary. The microphone row re-prompted and was
//     re-stamped with the Developer ID requirement at 16:13:46Z; Accessibility/ListenEvent never re-prompt.
//   - yap.log 2026-10-16: 14:03:28Z "Accessibility not trusted"; 16:13:51Z "PTT double-tap ->
//     hands-free ON" + "recording started"; then 7x "tap disabled by kCGEventTapDisabledByUserInput
//     ... re-arm #N issued" (16:13:59Z to 16:15:46Z) and ZERO fn edges logged after 16:13:51Z: no "fn
//     tap ends hands-free". The take ended 16:15:02Z from the window; the next take started 16:15:18Z
//     with no PTT line (window). pasted=false on both (Accessibility false).

ITEMS.push({
  id: 'yap-qa-harness', prompt: 'yap-qa-harness', branch: 'loop/yap-qa-harness-perm-probe-and-hotkey-walk', gated: null,
  title: 'Real permission + hotkey QA tools: scripts/qa/perm-probe.sh (signature, duplicates, LaunchServices, a labelled helper probe) and scripts/qa/hotkey-walk.sh (a real fn take on the installed Yap)',
  preflight: `
    test -x scripts/qa/perm-probe.sh
    test -x scripts/qa/hotkey-walk.sh
    test -f scripts/qa/perm-helper.swift
    grep -q 'helper_ax_trusted=' scripts/qa/perm-probe.sh
  `,
  spec: `
    Panel: not required — FIX (Wilson 2026-10-16: "tools that test it for real")
    DEPENDS: a green main. Blocks: yap-hotkey-stop (its acceptance runs these scripts).

    WHY: the loop QA'd 0.9.0 in headless Chrome against desktop/dist with shimmed permission answers.
    Nothing ever asked macOS what it answers for the SIGNED, INSTALLED binary, and nothing ever pressed
    fn. The signing identity changed (Apple Development -> Developer ID) and every TCC grant pinned to
    the old certificate went dead while System Settings kept showing ON.

    DO
      1. scripts/qa/perm-probe.sh [--app PATH] [--compare PATH] [--self-test]. Default app
         /Applications/Yap.app. READ-ONLY. Prints key=value lines, one per fact:
           app_path, bundle_id (plutil), version, team_id, authority_leaf, cdhash (codesign -dvvv),
           designated_requirement (codesign -d -r-), same_id_bundles (mdfind on kMDItemCFBundleIdentifier
           plus a scan of /Applications and ~/Applications, comma list, the probed app excluded),
           lsregister_count (lsregister -dump, registrations whose identifier is the bundle id, with the
           paths and a stale=1 flag for paths that no longer exist),
           running_pids (pgrep -f on the bundle executable path; read only, never signalled),
           helper_ax_trusted and helper_listen_event (from the helper, item 2; the KEY NAMES carry the
           helper_ prefix and a comment line says "helper values describe the QA helper, NOT Yap").
         --compare NEW.app prints dr_change=yes|no between the installed app and NEW.app, and on yes
         prints "warning: this build changes the designated requirement; every Accessibility, Input
         Monitoring and Full Disk Access grant for the old build stops matching". This is the release
         check that would have caught 0.9.0; docs/RELEASING.md (create if absent) gets one line telling
         the release seat to run it and to put the warning in the release notes when dr_change=yes.
         It never reads or writes TCC.db.
         --self-test runs every code path against /System/Applications/Calculator.app (and --compare
         Calculator vs Calculator, expecting dr_change=no) and exits non-zero on any missing key.
      2. scripts/qa/perm-helper.swift: about 20 lines, prints ax_trusted=<AXIsProcessTrusted()> and
         listen_event=<CGPreflightListenEventAccess()>. perm-probe.sh compiles it with swiftc into
         desktop/target/qa/perm-helper (gitignored target dir), signs it ad hoc (codesign -s -), and
         never uses Yap's identity. The helper is NOT the app and its answers are never presented as Yap's.
      3. scripts/qa/hotkey-walk.sh [--self-test]. Drives the INSTALLED Yap through a real take:
         posts a synthetic fn down (CGEvent flagsChanged with maskSecondaryFn, via a second tiny Swift
         helper scripts/qa/fn-post.swift built the same way), holds 1.5 s, posts fn up; tails
         ~/Library/Application Support/WilsonVoice/logs/yap.log and asserts, in order and within 5 s
         each: "PTT hold armed", "recording started", "PTT hold release", and the take's
         "transcript ok" or "no speech" line; then a second pass: double-tap fn (hands-free ON), wait
         3 s, tap fn once, assert "tap ends hands-free" and that the take ended. Exit codes: 0 pass,
         1 an assertion failed (prints which line never came), 3 the CALLER lacks Accessibility (posting
         CGEvents needs it; print "hotkey-walk needs Accessibility for <terminal/app> — grant it in
         System Settings -> Privacy & Security -> Accessibility, then rerun"), 4 Yap is running and
         YAP_QA_DRIVE_RUNNING is not 1 (print "Yap is running (pid N); this walk posts fn into it —
         rerun with YAP_QA_DRIVE_RUNNING=1 when you are not dictating"). It never launches, quits or
         signals Yap. --self-test exercises the refusal paths and the log-assertion parser against a
         fixture log (scripts/qa/fixtures/yap-walk-pass.log and yap-walk-stuck.log, the stuck one copied
         from the 16:13:51Z excerpt above with the transcript text replaced) and posts no events.
      4. Both scripts: set -u, no pipes in the exit-code path, shellcheck clean.

    WALK SEAT: the loop has no seat named walk; the walks are items (yap-uiux-2026-10-16-W0 and -Z in
    08-yap-uiux). Those specs now tell the walker to run both scripts and attach the output. The full
    hotkey-walk (events into the installed app) is Wilson's walk step, listed in the PR body.
  `,
  acceptance: `
    test -x scripts/qa/perm-probe.sh
    test -x scripts/qa/hotkey-walk.sh
    test -f scripts/qa/perm-helper.swift
    test -f scripts/qa/fn-post.swift
    bash scripts/qa/perm-probe.sh --self-test
    mkdir -p desktop/target && bash scripts/qa/perm-probe.sh --app /System/Applications/Calculator.app > desktop/target/qa-perm-probe.txt
    grep -q '^bundle_id=com.apple.calculator$' desktop/target/qa-perm-probe.txt
    grep -q '^cdhash=' desktop/target/qa-perm-probe.txt
    grep -q '^lsregister_count=' desktop/target/qa-perm-probe.txt
    grep -q '^helper_ax_trusted=' desktop/target/qa-perm-probe.txt
    grep -q '^helper_listen_event=' desktop/target/qa-perm-probe.txt
    bash scripts/qa/perm-probe.sh --app /System/Applications/Calculator.app --compare /System/Applications/Calculator.app > desktop/target/qa-perm-compare.txt
    grep -q '^dr_change=no$' desktop/target/qa-perm-compare.txt
    bash scripts/qa/hotkey-walk.sh --self-test
    shellcheck scripts/qa/perm-probe.sh scripts/qa/hotkey-walk.sh
  `,
})

ITEMS.push({
  id: 'yap-perm-truth', prompt: 'yap-perm-truth', branch: 'loop/yap-perm-truth-banner-names-binary-and-duplicates', gated: null,
  title: 'The permission banner tells the truth about THIS binary: re-checked on activation and every 5 s while shown, names the failing condition and the bundle path, names a duplicate Yap, and explains a stale grant',
  preflight: `
    grep -q 'WatchReason::Activate' desktop/src-tauri/src/lib.rs
    grep -q 'Another copy of Yap at' desktop/src/permission.ts
    test -f desktop/src-tauri/tests/perm_truth.rs
  `,
  spec: `
    Panel: not required — FIX (Wilson 2026-10-16 report; diagnosis at the top of this file)
    DEPENDS: yap-qa-harness (perm-probe.sh is how the PR proves the binary-level facts).

    EVIDENCE (main b2a1e2e)
      - permissions.rs:198 report() reads AXIsProcessTrusted (permissions.rs:126) and IOHIDCheckAccess
        (permissions.rs:384) fresh on every call; nothing is cached. The watcher (permissions.rs:684)
        re-reads on Launch, Focus (lib.rs:5613), Wake and Refusal (lib.rs:4161, lib.rs:5967,
        paste.rs:247) and on a 45 s timer only while something is denied (permissions.rs:542,
        permissions.rs:597). No app-activation trigger; a Settings change while the window is unfocused
        waits up to 45 s.
      - The banner (permission.ts:246) was CORRECT on 0.9.0: the OS answered false for this binary
        because the July grants are pinned to the Apple Development certificate. It just did not say
        why, so Wilson trusted the Settings toggle over it.
      - input_monitoring_state (permissions.rs:425) treats tap_health().re_arms == 0 as "the tap is
        delivering" (permissions.rs:409). A re-arm count is not delivery evidence: on 0.9.0 the tap was
        re-armed 7 times and delivered nothing.

    DO
      1. Re-check on activation: register NSApplicationDidBecomeActiveNotification (one observer, in
         the same place Focus is poked) and poke WatchReason::Activate. While any grant nags, the
         watcher interval is 5 s (WATCH_INTERVAL = 5 s, used ONLY while the banner is shown; parked
         otherwise, as today).
      2. The report carries checkedBundlePath (NSBundle.mainBundle bundlePath of the running process),
         signingAuthority (leaf certificate CN from SecCodeCopySigningInformation on self), and
         failingConditions: a list of "Accessibility false (AXIsProcessTrusted)" / "Input Monitoring
         false (IOHIDCheckAccess listen = denied)". The banner names each failing condition and the path
         it checked, e.g. "Accessibility is off for /Applications/Yap.app (AXIsProcessTrusted = false)".
      3. Stale grant: persist lastTrustedSignature (authority + cdhash + version) whenever
         Accessibility reads true. When it reads false and the stored signature differs from the running
         one, the banner adds: "macOS still lists Yap as allowed, but that permission was given to Yap
         <old version>, signed differently. In System Settings, select Yap, click the minus button, then
         click plus and choose /Applications/Yap.app." (same for Input Monitoring). Pure decision function
         with unit tests; no TCC reads or writes.
      4. Duplicate bundles: at launch, scan /Applications and ~/Applications (one level, plus
         *.app inside folders one level down) for bundles whose CFBundleIdentifier equals ours and whose
         path differs; if any, the banner adds "Another copy of Yap at <path> may hold the permission."
         Pure function over a directory listing, unit-tested with a temp dir.
      5. input_monitoring_state takes delivery evidence = "a key or flags edge was received since the
         last re-arm" (a counter ptt_macos.rs bumps in tap_callback), never re_arms == 0.
      6. Tests: desktop/src-tauri/tests/perm_truth.rs (>= 6 tests: stale-signature message, no message
         when signatures match, duplicate found, duplicate absent, failing-conditions wording, delivery
         evidence after re-arm); desktop/src/permission.truth.test.ts (>= 3 vitest cases on the banner
         text). Keep Tauri, app-sandbox false, never rename the bundle id or the data dir.

    WILSON'S WALK (PR body): bash scripts/qa/perm-probe.sh; open Yap: the banner names the path and,
    until he re-adds Yap in Settings, the stale-grant sentence.
  `,
  acceptance: `
    grep -q 'WatchReason::Activate' desktop/src-tauri/src/lib.rs
    grep -q 'Another copy of Yap at' desktop/src/permission.ts
    grep -q 'checkedBundlePath' desktop/src/permission.ts
    grep -qE 'WATCH_INTERVAL: Duration = Duration::from_secs\\(5\\)' desktop/src-tauri/src/permissions.rs
    test -f desktop/src-tauri/tests/perm_truth.rs
    test -f desktop/src/permission.truth.test.ts
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && npx vitest run src/permission.truth.test.ts)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    (cd desktop/src-tauri && cargo test --features custom-protocol --test perm_truth)
    test "$(cd desktop/src-tauri && cargo test --features custom-protocol --test perm_truth -- --list 2>/dev/null | grep -c ': test$')" -ge 6
    bash scripts/qa/perm-probe.sh --self-test
  `,
})

ITEMS.push({
  id: 'yap-hotkey-stop', prompt: 'yap-hotkey-stop', branch: 'loop/yap-hotkey-stop-fn-ends-every-take', gated: null,
  title: 'A take the hotkey started, the hotkey stops: fn key-up from any app, verified tap re-arm within 100 ms, a HID-state second mechanism, a 90 s still-recording prompt, and pill + menu bar stop',
  preflight: `
    test -f desktop/src-tauri/tests/hotkey_stop.rs
    grep -q 'STILL_RECORDING_AFTER_SECS: u64 = 90' desktop/src-tauri/src/ptt_macos.rs
    grep -q 'CGEventTapIsEnabled' desktop/src-tauri/src/ptt_macos.rs
  `,
  spec: `
    Panel: not required — FIX (Wilson 2026-10-16: 15 fn presses, the take would not stop)
    DEPENDS: yap-qa-harness (scripts/qa/hotkey-walk.sh is this item's acceptance tool; if it is not
      on main when you start, rebase after it lands; never write a second copy).

    EVIDENCE (main b2a1e2e; yap.log 2026-10-16, quoted at the top of this file)
      - START and STOP share ONE mechanism: the listen-only HID CGEventTap (ptt_macos.rs:309-341).
        Hold start = on_combo_down -> hold-arm thread (ptt_macos.rs:456-514); hold stop = on_combo_up
        (ptt_macos.rs:516-534); hands-free (double-tap) start = on_combo_up (ptt_macos.rs:545-556) and
        stop = the next combo down (ptt_macos.rs:466-478). No Carbon/global-shortcut fallback for fn
        (shortcuts.rs:22: there is no Code::Fn); the legacy toggle is off ("secondary disabled").
      - The tap is created whatever the grants say (lib.rs:5961-5981 only logs + pokes); it ran under an
        untrusted 0.9.0 and delivered the 16:13:45-51Z edges, then was disabled by user input 7 times.
      - Re-arm is fire-and-forget: re_arm_tap (ptt_macos.rs:707-716) calls CGEventTapEnable and logs
        "issued" with no check that the tap is enabled or that any edge arrives afterwards.
      - handle_combo_edge returns early when want == was (ptt_macos.rs:436-438): a missed up-edge leaves
        combo_down stuck true and the next press is swallowed; nothing reconciles against
        CGEventSourceFlagsState.
      - No take-duration prompt anywhere (no hold timeout, no "still recording").

    DO
      1. Verified re-arm: after CGEventTapEnable, read CGEventTapIsEnabled within 100 ms and log
         "re-arm verified" or "re-arm FAILED"; after 3 disables in 60 s, invalidate the port and create
         a fresh tap on the same run loop, logged. Count delivered edges since the last re-arm (shared
         with yap-perm-truth DO 5).
      2. Second mechanism for stop: while a take is live (and only then), a 30 ms poll of
         CGEventSourceFlagsState(HID system state) for maskSecondaryFn. A fn up seen by the poll ends a
         hold-mode take; an fn down-up seen by the poll ends a hands-free take. The poll and the tap
         feed the SAME edge function, deduplicated by edge time, so a take is never stopped twice. The
         poll also reconciles a stuck combo_down. Measure and record in the PR whether the HID flags
         read works without Input Monitoring on Wilson-shaped grants (helper values from perm-probe.sh).
      3. Any focused app: the stop path never depends on Yap being frontmost (tap at kCGHIDEventTap,
         poll is process-independent). Unit-test the edge state machine as a pure function: hold
         up-edge, hands-free tap, missed up-edge recovered by the poll, tap disabled mid-take then fn up.
      4. Still recording: STILL_RECORDING_AFTER_SECS: u64 = 90. A hold take with no key-up for 90 s, or
         ANY take during which the tap was disabled, shows on the pill "Still recording — press fn or
         click stop"; one fn tap or the hotkey then ends it.
      5. Crash-safe stop: the menu bar Stop item and the pill's stop control both end the take through
         the same stop function the hotkey uses, and both call end_hands_free (ptt_macos.rs:295) so the
         next fn press is not swallowed. Tests prove both call sites reach it.
      6. desktop/src-tauri/tests/hotkey_stop.rs: >= 8 tests. Keep Tauri, app-sandbox false, never rename
         the bundle id or the data dir.

    WILSON'S WALK (PR body): scripts/rebuild_app.sh; then YAP_QA_DRIVE_RUNNING=1 bash
    scripts/qa/hotkey-walk.sh from a terminal that holds Accessibility; then by hand in Notes and in
    Safari: hold fn, talk, release (take ends); double-tap fn, talk, tap fn (take ends).
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/hotkey_stop.rs
    grep -q 'STILL_RECORDING_AFTER_SECS: u64 = 90' desktop/src-tauri/src/ptt_macos.rs
    grep -q 'CGEventTapIsEnabled' desktop/src-tauri/src/ptt_macos.rs
    grep -q 're-arm verified' desktop/src-tauri/src/ptt_macos.rs
    grep -rq 'Still recording' desktop/src desktop/src-tauri/src
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    (cd desktop/src-tauri && cargo test --features custom-protocol --test hotkey_stop)
    test "$(cd desktop/src-tauri && cargo test --features custom-protocol --test hotkey_stop -- --list 2>/dev/null | grep -c ': test$')" -ge 8
    bash scripts/qa/hotkey-walk.sh --self-test
    bash scripts/qa/perm-probe.sh --self-test
  `,
})

// yap-format-structure — WISPR-STYLE STRUCTURE FROM SPEECH. Written 2026-10-16 by the
// yap-format-item seat. Wilson dictated a long review by voice today; Wispr Flow turned it into an
// intro paragraph, a bulleted list, prose, and a second list of "topic: verdict" bullets. His words:
//   "I just use Wispr Flow for that. You see how it does the formatting? That's what Yap needs to be
//    able to do. It knew I was doing a list of things."
// Yap pastes the same take as flat prose. FIX, no panel (unambiguous intent, Wispr parity).
// ORDER: this block sits in 08-yap-uiux-2026-10-16.mjs after yap-hotkey-stop and BEFORE W0 (one file =
// one lane, run in order), so this item runs ahead of yap-uiux-2026-10-16-W0 — the same placement as
// PR #208's three items. A separate adjacent file would land on the opposite lane and run concurrently.
// WHY A NEW ITEM, NOT AN EXTENSION OF Y4: every Y4 item (10-y4-formatting.mjs) is MERGED or
// ALREADY-DONE and retires at pre-flight. Y4-D proves EXPLICIT-cue lists only ("one, two, three",
// "first/second", the word "bullet": dictation.rs detect_and_format_list ~:1776, test
// list_bullets_only_on_explicit_cue) and guards against over-eager bullets; Y4-C paragraphs at pauses
// (insert_paragraphs ~:2606); Y4-F maps apps to six DictationModes (mode_for_app ~:65). None of them
// INFERS structure from parallel clauses or "topic: verdict" speech, and none emits Markdown per
// target. This item EXTENDS those modules (same pipeline, same fixture discipline) — it never builds
// a parallel formatter.
// Review: Opus only (review-opus + review-opus2), ADVERSARIAL on "did it invent content". No Codex,
// no Gemini, no third-party reviewer. SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.
//
// HOW YAP POLISHES TODAY (main, 2026-10-16, read-only):
//   desktop/src-tauri/src/dictation.rs run_cleanup (~:856): dictionary -> clean_backtrack ->
//     format_dictation (~:123, explicit-cue list detection only, else prose) -> apply_spoken_marks ->
//     format_email_shape (Email) -> insert_paragraphs -> trailing-period rule -> optional LLM polish.
//   desktop/yap-polish/src/main.rs SYSTEM_BASE (~:62) + mode_overlay (~:80): a llama.cpp rewriter
//     that "preserves existing line breaks and list numbering"; it never infers structure, and with
//     the shipped polish_model "" (lib.rs ~:476) it never runs at all. Paste is clipboard + synthetic
//     Cmd-V (paste.rs), so newlines in a paste never SEND a chat message.

ITEMS.push({
  id: 'yap-format-structure', prompt: 'yap-format-structure', branch: 'loop/yap-format-structure-wispr-style-lists-and-paragraphs', gated: null,
  title: 'Wispr-style structure from speech: enumerations become lists, topic-then-verdict becomes "Topic: verdict" bullets, topic shifts become paragraphs, formatted for the paste target',
  preflight: `
    test -f desktop/src-tauri/src/structure.rs
    grep -q 'src-tauri/src/structure.rs' desktop/yap-polish/src/main.rs
    test 8 -le "$(ls -d desktop/yap-polish/fixtures/structure/*/ | wc -l)"
    grep -q 'fn structure_fixture_01_two_lists_and_topic_verdict_bullets' desktop/src-tauri/src/structure.rs
  `,
  spec: `
    Panel: not required — FIX (Wilson 2026-10-16: "That's what Yap needs to be able to do. It knew I
    was doing a list of things.")
    DEPENDS: a green main; Y4-C, Y4-D, Y4-F (all on main). Runs ahead of yap-uiux-2026-10-16-W0.

    GOAL (Wilson): dictate a review the way he talks and get what Wispr Flow gives him — the lists
    are lists, the "this thing: verdict" lines are bullets, the topic shifts are paragraphs — pasted
    in the form the app he is in actually renders.

    WHERE IT LIVES (one module, two crates)
      - desktop/src-tauri/src/structure.rs: a PURE, rules-only, deterministic module. No LLM, no
        clock in the output path, no randomness, no HashMap iteration order in the output. It runs in
        run_cleanup right after format_dictation and before insert_paragraphs (insert_paragraphs must
        never break inside a list it produced), at the shipped level (Medium) and with NO polish model
        installed. Y4-D stays law: list detection is never LLM-dependent.
      - desktop/yap-polish/src/main.rs includes it with
        #[path = "../../src-tauri/src/structure.rs"] mod structure;  — the exact pattern already used
        for polish_protocol.rs — so the module is compiled and tested in BOTH crates and the sidecar
        can re-apply structure if the LLM flattens it. Add a SYSTEM_BASE rule "Preserve every bullet
        and numbered line exactly" (system_prompt_is_stable_per_mode_and_style must stay green). The
        module therefore depends only on std (no serde, no crate-local paths).
      - Tests live INSIDE structure.rs (#[cfg(test)] mod tests, every fn named structure_*) because a
        bin-only crate has no integration-test surface. Fixtures are read from
        concat!(env!("CARGO_MANIFEST_DIR"), "/../yap-polish/fixtures/structure") — a path that
        resolves from both crates because they are siblings under desktop/.

    RULES
      1. Enumerations become bulleted lists. Cues: "one, two, three"; "first / second / then / also /
         and then"; three or more parallel clauses or noun phrases in one run ("the security, the data
         pulling, the telemetry"); repeated sentence openers ("make sure X. make sure Y."); a topic
         followed by a colon-like pause (a PauseSpan, or an ASR comma/period) then a verdict.
      2. Numbered list when order matters ("first ... then ... finally", "step one").
      3. Paragraph breaks at topic shifts and after a verdict sentence that closes a list; reuse
         insert_paragraphs for pause-based breaks, do not duplicate it.
      4. "Topic: comment" bullets when he names a thing and then judges it:
         "Account and licence, perfect." -> "- Account and licence: perfect."
      5. Headings ONLY when he says "heading" or "title" explicitly; never invented.
      6. Target-aware output: pub fn structure_format_for(bundle_id: &str, setting: &str) ->
         StructureFormat { Markdown, PlainBullets, Off }.
           Markdown ("- ", "1. ", "## "): editors, notes, Slack, Notion-like — com.tinyspeck.slackmacgap,
             notion.id, md.obsidian, com.apple.Notes, com.microsoft.Word; a browser only when the user
             setting says so (browser = ambiguous, the Y4-F conservative rule).
           PlainBullets ("• " or "- " with real newlines, never "#" or "**"): com.apple.mail,
             com.apple.MobileSMS, com.microsoft.Outlook, and every unknown text field (the
             conservative default).
           Off: code editors and terminals (com.microsoft.VSCode, com.apple.Terminal,
             com.googlecode.iterm2, dev.warp.Warp-Stable, Cursor, com.apple.dt.Xcode) unless a mode is
             set; DictationMode::Code is always Off.
         Chat: Y4-C/Y4-F say chat gets no paragraph breaks because a typed Return sends. Paste is
         clipboard + Cmd-V (paste.rs), which does not send, so a list in Slack pastes as a list. Keep
         the no-blank-line rule for chat, allow single-newline list items, and pin it with a test.
      7. Never change his words beyond what polish already does: no new content, no summaries, no
         reordering, no dropped words except the cue words consumed as markers ("first", "second",
         "next", "number one", "bullet", "heading", "title"), kept in ONE named const list.
      8. Settings: a global "Structure" switch (default ON) and a per-app override list in
         desktop/src/views/settings/Dictation.tsx, persisted in the settings struct in lib.rs
         (structure_enabled: bool, structure_app_overrides keyed by bundle id), read in run_cleanup.
         Plain words on the control ("Turn spoken lists into bullets"), no engineer words (Y4-H).
      9. Latency: structure inference adds <= 150 ms p95 on a 400-word take on Apple Silicon (the lane
         machine is Wilson's M-series Mac). Measure 50 runs over the 400-word fixture, print p95.
     10. Deterministic: same transcript (+ pauses + target) -> byte-identical output.

    FIXTURES  desktop/yap-polish/fixtures/structure/NN-name/{input.txt, target.txt, expected.md,
      pauses.json (optional)} — at least 8 folders. target.txt holds a bundle id. Required:
      01-wilson-review-2026-10-16 (below); 02 numbered steps "first ... then ... finally";
      03 "make sure X / make sure Y" parallel openers; 04 topic-then-verdict alone; 05 explicit
      "heading"; 06 a 400-word take (the latency fixture); and two NEGATIVE controls that must come out
      as unchanged prose: 07 a narrative "and then we drove, and then we ate" (a story, not a list),
      08 fixture 03's text with target com.microsoft.VSCode (Off). A single-item "list" must not
      become a list (Y4-D single_item rule). Invented, bland text for 02-08; never real dictation.

      FIXTURE 01 \u{2014} Wilson's review, the FULL Wispr Flow output from 2026-10-16 (ground truth, saved at
      Wilson-Brain/Projects/Loop-Logs/yap-fixture-01-wispr-2026-10-16.md). expected.md is exactly the text
      below, byte-for-byte after stripping the 8-space indent of this spec. input.txt is the SAME words as
      flat speech: no list markers, no line breaks, no colon after a topic (use a comma or a period as ASR
      would), so the structure stage must recreate the lists, the "Topic: verdict" bullets and the paragraphs.
      target.txt: notion.id
      expected.md:
        Okay, I really, really liked the slideshow. It was very good, very great, very intuitive, and easily understood. I love everything.
        One thing I'm mainly concerned about is to make sure all of the components are properly structured and straight, and to polish every one of them. Make sure there's no spillage or bubbles that are not properly confined and contained. Every page is good. We think about scrolling, pads, error states, handling, and stuff like that.
        Also, one of my biggest concerns is external devices. Make sure it's properly built:
        - the security
        - the data pulling
        - the telemetry
        - the health
        - constant-time health checks to make sure devices are connected and healthy and not trying to put viruses or inject something into the programme, etc.
        Make sure the whole app actually has proper security handling. Please think about the user experience.
        Is AP okay? The MCP's security, telemetry, deep metadata, and editing capabilities for all AI agents and stuff like that: I don't know if there should be an API system. I don't know. That's up to you.
        Everything is good:
        - First run permissions: make sure that's really verifiable and it's always good.
        - Mic test, all of those.
        - Make sure all of the devices, all the external devices, are properly health-checked, working smoothly, with alerts for the user, data reporting, and all of that.
        - Make sure people can send reports back to me, back to Drivia.
        - Account and licence: perfect. Make sure, just like we did it for Yap with the Stripe and all of that, people can go on it on any device, any account. All they have to do is just copy and paste that licence, so if they have it saved, type it out. I think that's what you mean, but all of that to make sure it's all smooth.
        - Confirmation number, all of that.
        - Guided tour: it looks good. I love all of the tour. It looks very nice and polished.
        - Check the components and make sure everything is smooth.
        - Remember screen record and UI screenshot: you can make that.
        - Library: make sure it looks smooth.
        - Storage: make sure that's properly built out.
        - User experience: users can see a timed live health check update, so they can see lazy loading in the background, skeletons, all of that.
        - Programme: all of that.
        - Prior to server-side SQL Lite: make sure all of that schema is properly built out. Think about every role of security and data isolation, and make sure nobody can hack them, but still fast.
        - New session: make sure that's smooth. All the health check and proper stuff are handled before they get into it. That's good.
        - Solo with gas consent shows the status here. Perfect.
        - Crash report data is properly this telemetry and metadata, so all of that can be used to upgrade for AI agents like Codex or Claude, mostly Claude, because Claude is better.
        - Record pre-roll perfectly. Make sure it's thought out.
        - Record live, perfect.
        - Live check: always okay.
        - Metadata: deep, extensive telemetry, metadata on this, on all levels, from visual, audio, transcript, all of that, because all of that needs to be used for building and everything.
        - Timer: I love that.
        - Guess web page: perfect.
        - Time dial: make sure there's a dark component too, plus stop.
        - Okay, looks good.
        - Hold after stop.
        - Transcribe each check.
        - Make sure to constantly know what's going on and where the system is at. That's perfect.
        - Editor: ooh, perfect. Looks good.
        - Talk: make sure everything is tight, smooth, glossy, polished, and make sure the programme overlays Wilson. Okay, all of that.
        - Struck Ember AI suggestion.
        - Cut word.
        - Except make sure all those tool suites are properly defining whatever coding language that needs to be.
        - Everything looks smooth here. Good job.
        - This looks very intuitive.
        - The clips: perfectly.
        - The long, split, fix, landscape: you got to check the hard.
        - I'll have to connect the hard drive so you can see how we did.
        - They did those videos because all of the script is pretty much what I want those clips and the editing for those videos to be.
        - Top 5 short categories and genre: you can check the session for the social media reel.
        - Honestly, that'd be better.
        - What we did: audio tools, perfect.
        - Improve. Make sure they're good.
        - Programme is good. It's properly structured.
        - People: make sure you think that's well thought out.
        - Security and deletion are good.
        - Not fucking up the computer.
        - Everything is a health check.

    TESTS (all in structure.rs, all named structure_*)
      structure_fixture_01_two_lists_and_topic_verdict_bullets — at least 2 lists, the "Topic: verdict"
        lines present
      structure_golden_fixtures_match — every folder, input + target -> expected.md, byte-equal
      structure_is_deterministic_three_runs — every fixture 3x, outputs identical
      structure_latency_p95_under_150ms — prints "structure p95_ms=<n>", fails above 150
      structure_never_invents_content_words — for every fixture: the lowercase alphanumeric words of
        the output minus list markers are a sub-multiset of the input words, AND every non-cue input
        word appears in the output (nothing dropped). The review's adversarial check; also run it over
        seeded (fixed seed) shuffles of the fixture sentences.
      structure_format_for_bundle_id — the rule-6 table, incl. unknown -> PlainBullets, code -> Off
      structure_chat_paste_has_no_blank_lines
      structure_single_item_is_not_a_list, structure_narrative_and_then_is_not_a_list
      Settings: extend tests/formatting_settings_persist.rs with structure_settings_persist (global
      switch + one per-app override round-trip). UI: a vitest for Dictation.tsx asserting the switch
      renders and calls the setter if the repo has a view test harness; otherwise one line in
      docs/loop/MANUAL-WALK.md (create if absent): "Settings > Dictation > Turn spoken lists into
      bullets: off -> dictate fixture 02 into Notes -> flat prose; on -> numbered list".

    WHAT NOT TO DO
      - Do NOT route structure through the LLM or make it depend on polish_model.
      - Do NOT relax any existing formatting_fixtures / app_aware_formatting / paragraphing expectation
        to make room; a conflict is a finding to report, not a row to edit.
      - Do NOT add words, summaries, or headings he did not say.
      - Do NOT launch the GUI app or touch Wilson's installed /Applications/Yap.app.
  `,
  acceptance: `
    test 8 -le "$(ls -d desktop/yap-polish/fixtures/structure/*/ | wc -l)"
    grep -q 'Account and licence' desktop/yap-polish/fixtures/structure/01-wilson-review-2026-10-16/input.txt
    grep -q 'Account and licence: perfect' desktop/yap-polish/fixtures/structure/01-wilson-review-2026-10-16/expected.md
    grep -q 'src-tauri/src/structure.rs' desktop/yap-polish/src/main.rs
    for t in structure_fixture_01_two_lists_and_topic_verdict_bullets structure_golden_fixtures_match structure_is_deterministic_three_runs structure_latency_p95_under_150ms structure_never_invents_content_words structure_format_for_bundle_id structure_single_item_is_not_a_list structure_narrative_and_then_is_not_a_list; do grep -q "fn $t" desktop/src-tauri/src/structure.rs || exit 1; done
    grep -q 'structure_settings_persist' desktop/src-tauri/tests/formatting_settings_persist.rs
    grep -qi 'structure' desktop/src/views/settings/Dictation.tsx
    cd ${APP} && npm ci
    test 8 -le "$(cargo test -p yap-polish --release structure_ -- --list 2>/dev/null | grep -c ': test$')"
    cargo test -p yap-polish --release structure_ -- --nocapture ; test $? -eq 0
    cd src-tauri
    cargo test --features custom-protocol structure_ ; test $? -eq 0
    cargo test --features custom-protocol --test formatting_fixtures ; test $? -eq 0
    cargo test --features custom-protocol --test app_aware_formatting ; test $? -eq 0
    cargo test --features custom-protocol --test formatting_settings_persist ; test $? -eq 0
  `,
})

// yap-uiux-2026-10-16 — THE UI/UX PASS. Written 2026-10-16 by the yap-prep seat.
// Wilson, 2026-10-06: "All of the Yap stuff: the UI/UX ... There are many items you'll find and
// improve on." Memory: feedback_ui_containment (every panel and shell ships contained),
// feedback_builder_e2e_product_walk ("working" = the full flow as the persona).
// Shape: W0 walks main's build as two personas and writes the ledger; one child per screen fixes
// that screen's rows; Z re-walks after the flush and IS the acceptance of the pass.
// Locked: the pixel-art pill stays (no vector mascot, no origami); NO NEW FEATURES; controls
// first, prose last; contained panels (surface + border, pinned action footer, no infinite scroll,
// no page-level horizontal scroll). Headless only: scripts/smoke-windowed.sh drives headless
// Chrome (--headless=new) against desktop/dist; it never launches the app and never opens a
// visible window. Lane port: export YAP_SMOKE_PORT to your lane's vite port (A 5273, B 5274).
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap-uiux-2026-10-16-W0', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-w0-first-run-and-daily-walk', gated: null,
  title: 'Walk the main build as a first-run user and a daily user, headless, and write down every screen that reads generic, uncontained, confusing or slow',
  preflight: `
    test -f docs/loop/UIUX-WALK-2026-10-16.md
    test -f scripts/smoke-fixtures-first-run.json
    test -f scripts/smoke-fixtures-daily.json
    grep -q 'YAP_SMOKE_FIXTURES' scripts/smoke-windowed.mjs
    grep -q 'YAP_SMOKE_VIEWS' scripts/smoke-windowed.mjs
  `,
  spec: `
    Panel: not required — a walk that writes a ledger and changes no view
    DEPENDS: yap-flakes-2026-10-16 (a green main); Y0-E's smoke (scripts/smoke-windowed.sh, on main)
      QA TOOLS (yap-qa-harness, earlier in this same file, so the same lane, already merged): also run bash scripts/qa/perm-probe.sh and
      bash scripts/qa/hotkey-walk.sh --self-test and paste both outputs into the ledger header and the PR
      body. The full hotkey-walk (it posts fn into the installed Yap) is Wilson's walk step, never an agent's.

    EVIDENCE
      - scripts/smoke-windowed.sh + scripts/smoke-windowed.mjs walk the seven Nav views at 980x700
        and 720x520 in headless Chrome against desktop/dist with a shimmed __TAURI_INTERNALS__,
        and read every answer from ONE file, scripts/smoke-fixtures.json (smoke-windowed.mjs:74).
        One fixture set cannot show both the empty first run and a lived-in day.
      - The 2026-09-15 builders logged "NOT visually QA'd" on Y2-B, Y5-B, Y5-C and Y5-D.
      - Screens: desktop/src/views/Home.tsx (+ src/home/), Dictionary.tsx, Insights.tsx,
        Meetings.tsx (+ src/meetings/), Permissions.tsx, Scratchpad.tsx, Settings.tsx +
        views/settings/{Advanced,Audio,Companion,Dictation,License,Privacy,Shortcut,Snippets}.tsx,
        src/Onboarding.tsx, src/ModelSetup.tsx, src/PermissionHealthRow.tsx.

    DO
      1. smoke-windowed.mjs reads YAP_SMOKE_FIXTURES (a path; default scripts/smoke-fixtures.json,
         so behaviour is unchanged when it is unset) and YAP_SMOKE_VIEWS (a comma list of Nav
         labels, case-insensitive; unset = all views). It also captures the first-run overlay
         (Onboarding / ModelSetup) as the pseudo-view "Onboarding" when the fixture set shows it.
      2. Two fixture sets, only shapes already certain (the _README rule in smoke-fixtures.json):
         scripts/smoke-fixtures-first-run.json (no history, no meetings, permissions not granted,
         no model installed, trial day 14) and scripts/smoke-fixtures-daily.json (40 dictations,
         3 meetings with one still transcribing, every permission granted, trial day 5). Every
         string is invented and bland: never real dictation text (RESUME §11.10).
      3. Walk both personas with YAP_SMOKE_SHOTS pointing at
         ~/Obsidian/Wilson-Brain/Projects/Loop-Logs/yap-uiux-2026-10-16/first-run/ and .../daily/.
         PNGs stay out of git.
      4. Write docs/loop/UIUX-WALK-2026-10-16.md: a header, then one table row per finding, each
         row starting with "| <Screen> |" where Screen is one of Home, Dictionary, Insights,
         Meetings, Scratchpad, Settings, Onboarding:
         | Screen | view file | persona | verdict | evidence (PNG path + what it shows) | fix | child id | status |
         verdict is one or more of generic / uncontained / confusing / slow; fix is named and
         scoped to the view file; status is open, fixed or none (a screen with no finding gets
         one "none" row). Judge by: controls first, prose last; every panel contained; empty
         states say what to do next; nothing spins past 10 s; the pill is out of scope here (it
         stays pixel-art; yap24-PILL owns its states). A finding that needs a new capability goes
         to an "Out of scope (new feature)" list at the bottom, never into a row.
      5. This item changes no view. It ships the env vars, the fixture sets, the ledger and the
         baseline structural findings for both personas.
  `,
  acceptance: `
    test -f docs/loop/UIUX-WALK-2026-10-16.md
    test -f scripts/smoke-fixtures-first-run.json
    test -f scripts/smoke-fixtures-daily.json
    node -e "for (const f of ['scripts/smoke-fixtures-first-run.json','scripts/smoke-fixtures-daily.json']) JSON.parse(require('fs').readFileSync(f,'utf8'))"
    grep -q 'YAP_SMOKE_FIXTURES' scripts/smoke-windowed.mjs
    grep -q 'YAP_SMOKE_VIEWS' scripts/smoke-windowed.mjs
    for s in Home Dictionary Insights Meetings Scratchpad Settings Onboarding; do grep -q "^| $s |" docs/loop/UIUX-WALK-2026-10-16.md || exit 1; done
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
  `,
})

ITEMS.push({
  id: 'yap-uiux-2026-10-16-HOME', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-home', gated: null,
  title: 'Home reads contained and obvious for both personas: every walk row on Home fixed',
  preflight: `
    grep -q '^| Home |' docs/loop/UIUX-WALK-2026-10-16.md
    ! grep '^| Home |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
  `,
  spec: `
    Panel: not required — fixes the W0 ledger names, no new features
    DEPENDS: yap-uiux-2026-10-16-W0
    FILES: desktop/src/views/Home.tsx, desktop/src/home/ (YappyHouse stays pixel-art)
    DO: read every "| Home |" row in docs/loop/UIUX-WALK-2026-10-16.md and fix each in the named
      file only. Controls first, prose last; contained surfaces; the empty state says what to do
      next. Never show a real transcript list in any capture. Re-walk Home for both fixture sets,
      put the after PNGs in the Obsidian walk dir under after/home/, attach one before/after pair
      to the PR, and flip the rows to fixed with the PR number.
  `,
  acceptance: `
    ! grep '^| Home |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Home YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-first-run.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
    YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Home YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-daily.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
  `,
})

ITEMS.push({
  id: 'yap-uiux-2026-10-16-DICT', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-dictionary', gated: null,
  title: 'Dictionary reads contained and obvious for both personas: every walk row on Dictionary fixed',
  preflight: `
    grep -q '^| Dictionary |' docs/loop/UIUX-WALK-2026-10-16.md
    ! grep '^| Dictionary |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
  `,
  spec: `
    Panel: not required — fixes the W0 ledger names, no new features
    DEPENDS: yap-uiux-2026-10-16-W0
    FILES: desktop/src/views/Dictionary.tsx
    DO: read every "| Dictionary |" row in docs/loop/UIUX-WALK-2026-10-16.md and fix each in the
      named file only. Controls first (add word, search), prose last; a contained list with a
      pinned action footer, no infinite scroll. Re-walk Dictionary for both fixture sets, after
      PNGs to the Obsidian walk dir under after/dictionary/, one before/after pair on the PR, rows
      flipped to fixed with the PR number.
  `,
  acceptance: `
    ! grep '^| Dictionary |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Dictionary YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-first-run.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
    YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Dictionary YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-daily.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
  `,
})

ITEMS.push({
  id: 'yap-uiux-2026-10-16-INSIGHTS', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-insights', gated: null,
  title: 'Insights reads contained and obvious for both personas: every walk row on Insights fixed',
  preflight: `
    grep -q '^| Insights |' docs/loop/UIUX-WALK-2026-10-16.md
    ! grep '^| Insights |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
  `,
  spec: `
    Panel: not required — fixes the W0 ledger names, no new features
    DEPENDS: yap-uiux-2026-10-16-W0
    FILES: desktop/src/views/Insights.tsx
    DO: read every "| Insights |" row in docs/loop/UIUX-WALK-2026-10-16.md and fix each in the
      named file only. The first-run persona has no data: the empty state says what produces
      insights instead of a zero-filled chart. Re-walk Insights for both fixture sets, after PNGs
      to the Obsidian walk dir under after/insights/, one before/after pair on the PR, rows flipped
      to fixed with the PR number.
  `,
  acceptance: `
    ! grep '^| Insights |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Insights YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-first-run.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
    YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Insights YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-daily.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
  `,
})

ITEMS.push({
  id: 'yap-uiux-2026-10-16-MEETINGS', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-meetings', gated: null,
  title: 'Meetings reads contained and obvious for both personas: every walk row on Meetings fixed',
  preflight: `
    grep -q '^| Meetings |' docs/loop/UIUX-WALK-2026-10-16.md
    ! grep '^| Meetings |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
  `,
  spec: `
    Panel: not required — fixes the W0 ledger names, no new features
    DEPENDS: yap-uiux-2026-10-16-W0; coordinate with yap24-NT3/NT4 if they are open (same files):
      rebase onto whichever merged first and keep both
    FILES: desktop/src/views/Meetings.tsx, desktop/src/meetings/
    DO: read every "| Meetings |" row in docs/loop/UIUX-WALK-2026-10-16.md and fix each in the
      named files only. Record/stop is the first control; a transcribing meeting shows progress,
      never a bare spinner past 10 s; a needs-model state offers the one-click install, never an
      error. Re-walk Meetings for both fixture sets, after PNGs to the Obsidian walk dir under
      after/meetings/, one before/after pair on the PR, rows flipped to fixed with the PR number.
  `,
  acceptance: `
    ! grep '^| Meetings |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Meetings YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-first-run.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
    YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Meetings YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-daily.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
  `,
})

ITEMS.push({
  id: 'yap-uiux-2026-10-16-SCRATCH', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-scratchpad', gated: null,
  title: 'Scratchpad reads contained and obvious for both personas: every walk row on Scratchpad fixed',
  preflight: `
    grep -q '^| Scratchpad |' docs/loop/UIUX-WALK-2026-10-16.md
    ! grep '^| Scratchpad |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
  `,
  spec: `
    Panel: not required — fixes the W0 ledger names, no new features
    DEPENDS: yap-uiux-2026-10-16-W0
    FILES: desktop/src/views/Scratchpad.tsx
    DO: read every "| Scratchpad |" row in docs/loop/UIUX-WALK-2026-10-16.md and fix each in the
      named file only. Controls first, prose last; the editor surface is contained and its actions
      sit in a pinned footer. Re-walk Scratchpad for both fixture sets, after PNGs to the Obsidian
      walk dir under after/scratchpad/, one before/after pair on the PR, rows flipped to fixed with
      the PR number.
  `,
  acceptance: `
    ! grep '^| Scratchpad |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Scratchpad YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-first-run.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
    YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Scratchpad YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-daily.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
  `,
})

ITEMS.push({
  id: 'yap-uiux-2026-10-16-SETTINGS', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-settings', gated: null,
  title: 'Settings is contained: bordered surfaces, a pinned footer, every walk row on Settings and its eight panes fixed',
  preflight: `
    grep -q '^| Settings |' docs/loop/UIUX-WALK-2026-10-16.md
    ! grep '^| Settings |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
  `,
  spec: `
    Panel: not required — fixes the W0 ledger names, no new features (10-06 new item "settings
      containment (borders/surfaces/pinned footer)")
    DEPENDS: yap-uiux-2026-10-16-W0; follow the Y5-A token layer if #190 has merged, otherwise use
      the existing App.css variables and add none
    FILES: desktop/src/views/Settings.tsx, desktop/src/views/settings/*.tsx
    DO: read every "| Settings |" row in docs/loop/UIUX-WALK-2026-10-16.md and fix each in the
      named files only. Each pane is one contained surface; the toggle or picker comes first and
      its explanation follows in one line; save/reset actions sit in a pinned footer; nothing
      overflows at 720x520. Re-walk Settings for both fixture sets, after PNGs to the Obsidian walk
      dir under after/settings/, one before/after pair on the PR, rows flipped to fixed with the
      PR number.
  `,
  acceptance: `
    ! grep '^| Settings |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Settings YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-first-run.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
    YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Settings YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-daily.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
  `,
})

ITEMS.push({
  id: 'yap-uiux-2026-10-16-ONBOARD', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-onboarding-and-permissions', gated: null,
  title: 'First run is obvious: Onboarding, ModelSetup and Permissions say what to grant and why, one step at a time; every walk row fixed',
  preflight: `
    grep -q '^| Onboarding |' docs/loop/UIUX-WALK-2026-10-16.md
    ! grep '^| Onboarding |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
  `,
  spec: `
    Panel: not required — fixes the W0 ledger names, no new features (10-06 new item "first-run /
      permissions flow (mic, accessibility, input monitoring) with real screenshots"; the real
      screenshots are the headless walk's PNGs, never a visible app window)
    DEPENDS: yap-uiux-2026-10-16-W0; the Y1 permission items if open (same files)
    FILES: desktop/src/Onboarding.tsx, desktop/src/ModelSetup.tsx, desktop/src/views/Permissions.tsx,
      desktop/src/PermissionHealthRow.tsx
    DO: read every "| Onboarding |" row (and any "| Permissions |"-file finding W0 filed under
      Onboarding) in docs/loop/UIUX-WALK-2026-10-16.md and fix each in the named files only. One
      permission per step, the grant button first, one line of why after it; the model download
      shows size and progress; a denied grant says exactly where to fix it in System Settings.
      Never touch the bundle id (TCC grants). Re-walk with the first-run set (YAP_SMOKE_VIEWS
      Onboarding,Permissions), after PNGs to the Obsidian walk dir under after/onboarding/, one
      before/after pair on the PR, rows flipped to fixed with the PR number.
  `,
  acceptance: `
    ! grep '^| Onboarding |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Onboarding,Permissions YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-first-run.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
    YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Permissions YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-daily.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
  `,
})

ITEMS.push({
  id: 'yap-uiux-2026-10-16-Z', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-z-rewalk-after-flush', gated: null,
  title: 'Re-walk after the flush: both personas, every screen, zero open rows, the full smoke green — the acceptance of the UI/UX pass',
  preflight: `
    test -f docs/loop/UIUX-REWALK-2026-10-16.md
    ! grep -q '| open |' docs/loop/UIUX-WALK-2026-10-16.md
  `,
  spec: `
    Panel: not required — verification of the pass
    DEPENDS: every yap-uiux-2026-10-16 child above merged (HOME, DICT, INSIGHTS, MEETINGS, SCRATCH,
      SETTINGS, ONBOARD)
      QA TOOLS (yap-qa-harness, earlier in this same file, so the same lane, already merged): also run bash scripts/qa/perm-probe.sh and
      bash scripts/qa/hotkey-walk.sh --self-test and paste both outputs into the ledger header and the PR
      body. The full hotkey-walk (it posts fn into the installed Yap) is Wilson's walk step, never an agent's.
    DO: re-run the FULL walk (no YAP_SMOKE_VIEWS) for both fixture sets, PNGs to
      ~/Obsidian/Wilson-Brain/Projects/Loop-Logs/yap-uiux-2026-10-16/rewalk-first-run/ and
      .../rewalk-daily/. Write docs/loop/UIUX-REWALK-2026-10-16.md: per screen, the before and
      after verdict, the PR that fixed it, and the PNG paths. A NEW finding on the re-walk becomes a
      ledger row and is fixed here when it stays inside one view file; anything wider becomes a
      numbered entry in docs/loop/DEFERRED.md. The full smoke exiting 0 for both fixture sets is
      the acceptance of the whole UI/UX pass.
  `,
  acceptance: `
    bash scripts/qa/perm-probe.sh --self-test
    bash scripts/qa/hotkey-walk.sh --self-test
    test -f docs/loop/UIUX-REWALK-2026-10-16.md
    ! grep -q '| open |' docs/loop/UIUX-WALK-2026-10-16.md
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-first-run.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
    YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-daily.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
  `,
})
