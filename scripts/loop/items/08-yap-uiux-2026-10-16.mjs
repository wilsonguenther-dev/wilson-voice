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

// yap-meetings-fix — MEETINGS WORKS END TO END. Written 2026-10-16 by the yap-notetaker seat (Opus) from
// Wilson's 20:30Z report: "the meeting thing is not working for shit. I don't know if you ever fix it …
// we need to reverse engineer the Notetaker because ours is not working whatsoever." FIX, no panel
// (unambiguous intent). ORDER: after yap-format-structure and BEFORE yap-uiux-2026-10-16-W0 (one file =
// one lane, run in order), so W0 walks a Meetings view that can actually produce a transcript.
// Parity doc: ~/Obsidian/Wilson-Brain/Projects/Yap/wispr-notetaker-parity-2026-10-16.md.
// Review: Opus only (review-opus + review-opus2). No Codex, no Gemini, no third-party reviewer.
// HAZARD (same as 07/08): no agent launches the GUI app, posts events into, signals or quits Wilson's
// running /Applications/Yap.app, or writes TCC. scripts/qa/meeting-walk.sh drives the INSTALLED app
// only with YAP_QA_DRIVE_RUNNING=1 (the walk seat's consent), exactly like hotkey-walk.sh.
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.
//
// DIAGNOSIS (observed on Wilson's Mac 2026-10-16 ~20:35Z, read-only; main d6cd36a):
//   1. NO TRANSCRIPT, EVER. wilson_voice.db has 4 meetings (2026-08-16, 08-20, 10-10, 10-16 20:05Z), ALL
//      state=transcribing, processed_through_seconds=0.0. meeting_control.rs ~:656 sets Transcribing with the
//      comment "YV93's transcription pipeline moves it on to complete" — but Database::append_meeting_segments
//      and set_meeting_progress have NO caller in src/ (only tests/). yap.log: "meeting 8f9b3c78… stopped after
//      00:00:10 (user stopped) → transcribing" and nothing after. The wiring is yap24-NT2, gated: 'panel' —
//      never built. Audio is NOT the blocker: parakeet-unified-en-0.6b-Q8_0.gguf, qwen2.5-1.5b (polishModel)
//      and wespeaker CAM++ are all installed in ~/Library/Application Support/WilsonVoice/models.
//   2. SILENCE = DENIAL, AND IT LOCKS. TCC kTCCServiceAudioCapture auth=2 at 20:04:52Z with the 0.9.0
//      Developer ID csreq (valid). The meeting at 20:05:04Z ran 10.5 s; permission_verdict (syscapture.rs
//      ~:2334) says LooksDenied after DENIAL_GRACE = 3 s (~:2243) with no non-zero sample — what a silent Mac
//      output produces. meeting_control.rs ~:1094 wrote settings_kv meeting_system_audio_setup_ack_v1 =
//      "looks_denied 20:05:15Z"; track_b_plan (~:2471) now returns MicOnly for every future meeting, so the
//      tap never reopens and the verdict can never heal (only Settings' run_system_audio_setup rewrites it).
//      The row's error tells Wilson "macOS has not granted System Audio Recording" — false. sys_wav_path NULL.
//      This is yap24-NT3, gated: 'panel'.
//   3. NOT SCREEN RECORDING. The capture is a CoreAudio process tap (syscapture.rs header): the grant is
//      "System Audio Recording" (kTCCServiceAudioCapture), not Screen Recording. Do not add an SCK path.
//   4. Side note for perm-truth: kTCCServiceListenEvent's csreq is still "Apple Development … U8BP8Z86T2"
//      (dead for 0.9.0); Accessibility/FDA/Mic/AudioCapture are re-stamped to Developer ID.

ITEMS.push({
  id: 'yap-meetings-fix', prompt: 'yap-meetings-fix', branch: 'loop/yap-meetings-fix-transcript-and-system-audio-end-to-end', gated: null,
  title: 'Meetings works end to end: Stop produces a transcript and a summary, a silent call is never called a denial, and the walk proves it on the installed build',
  preflight: `
    test -x scripts/qa/meeting-walk.sh
    test -f desktop/src-tauri/tests/meeting_pipeline_wired.rs
    test -f desktop/src-tauri/tests/system_audio_verdict_silence_is_not_denial.rs
    grep -q "output_was_running" desktop/src-tauri/src/syscapture.rs
    grep -rln "append_meeting_segments" desktop/src-tauri/src | grep -v "db.rs" | grep -q .
  `,
  spec: `
    Panel: not required — FIX (Wilson 2026-10-16 20:30Z: "the meeting thing is not working for shit … ours
    is not working whatsoever." His observation is ground truth; the DB proves it: 4 of 4 meetings stuck.)
    DEPENDS: a green main; yap-perm-truth (#211, merged), yap-qa-harness (#210, merged). Runs ahead of
    yap-uiux-2026-10-16-W0. ABSORBS the core of the panel-gated yap24-NT1, NT2 and NT3: read their specs in
    scripts/loop/items/01-yap24-notetaker.mjs (~:107-425) as the detailed design, and make their PREFLIGHT
    markers true so they retire when the panel ungates them. Do NOT pull in NT0's eval corpus gate: the WER
    run (meeting_eval) is the thing that hit the 120 s cap earlier today and stays in NT0/NT2.

    ROOT CAUSE FIRST (from the diagnosis block above — fix these before any UI):
      A. Wire stop → transcribe. When MeetingController::stop lands a whole capture (state Transcribing), hand
         the meeting id to ONE background job that runs the shipped MeetingAsr path (meeting_asr.rs
         plan_windows / assemble / merge_two_tracks_by_host_time, asr_engine::transcribe_timed) over t0 (and t1
         when present), calls db.append_meeting_segments per window with on_track, advances
         set_meeting_progress monotonically, and sets Complete (or Failed with a plain-words error). One job
         at a time; dictation keeps priority on the shared engine (the job yields between windows). Remove any
         allow(dead_code) from meeting_asr.rs that this makes live.
      B. Reconcile stranded rows on launch (reconcile_stranded_meetings): a row in transcribing/summarizing
         whose WAV still exists is re-queued from processed_through_seconds; one whose WAV is gone becomes
         Failed "the audio for this meeting was removed before it was transcribed". Retention never purges a
         WAV whose meeting is not Complete (NT1). Wilson's 4 stuck rows are the real-world case: 2 have WAVs.
      C. Silence is not a denial (NT3). The verdict reads "did the OUTPUT device run during the meeting"
         (kAudioDevicePropertyDeviceIsRunningSomewhere on the default output, sampled by the pump; field
         output_was_running). Zero samples + output never ran = Unknown (write nothing). Zero samples +
         output ran >= 60 s across the whole meeting, in TWO consecutive meetings = LooksDenied (NT3 guards,
         01-yap24-notetaker.mjs ~:376-378: a muted call or an output-device switch runs the output with a silent
         tap; one meeting or DENIAL_GRACE alone is never a denial). And the verdict is never sticky: a Virtual/Unknown meeting
         ALWAYS attempts the tap (a fresh tap costs ~200 ms) and lets this meeting's evidence overwrite the
         row; LooksDenied only changes the BADGE, never skips the attempt. In-person meetings never attach.
      D. After Complete, if a local summary model is installed (settings polishModel, today
         qwen2.5-1.5b-instruct-q4_k_m), run summarize_meeting automatically; "no model" leaves the transcript
         and shows "Add a summary model in Settings" — a state, not an error.

    LIVE STATE (the user can see it is working): the pill shows recording + elapsed + two level meters
    (You / Call audio) while recording; after Stop it shows "Transcribing 42%" then "Notes ready". The
    Meetings view row shows the same state and progress (meeting:progress event), and a Failed row shows the
    error and Retry. Keep the locked pixel-art pill and mascot; no new visual language.

    PERMISSION TRUTH: extend the perm-truth banner (permission.ts / PermissionHealthRow.tsx) with System Audio
    Recording, with the SAME truth rules as #211 (re-checked on activation and every 5 s while shown; names
    the failing condition and the bundle path). Source of truth = this binary's own probe (a short tap open in
    run_system_audio_setup with output running), never the settings_kv row alone. Copy names the real
    System Settings path: Privacy & Security › Screen & System Audio Recording › System Audio Recording Only.

    scripts/qa/meeting-walk.sh (new; same shape and refusal discipline as hotkey-walk.sh):
      - Refuses unless Yap is running (exit 6) and YAP_QA_DRIVE_RUNNING=1 (exit 4); caller needs
        Accessibility to post the shortcut (exit 3); usage / app / DB / log missing (exit 2).
      - Makes its sample at run time with say -o "$TMP/sample.aiff" -f scripts/qa/fixtures/meeting-script.txt
        (a ~120-word two-voice script, committed as text; no audio binary in git), plays it in a loop with
        afplay for 30 s, and starts/stops the meeting with ⌃⌘M posted through a QA Swift helper built ad hoc
        (pattern: fn-post.swift) — the documented keyboard path; no new socket.
      - Reads the DB READ-ONLY (sqlite3 -readonly) and Yap's log: the newest meeting reaches complete within
        90 s of Stop; segments on track 1 (call audio) total > 40 words; summary IS NOT NULL when a summary
        model is installed (else prints "summary skipped: no model" and still requires the transcript).
      - If the meeting's own verdict is LooksDenied while afplay was running (true denial), exit 5 with
        "System Audio Recording is not granted for /Applications/Yap.app (designated requirement …) — allow it
        in System Settings › Privacy & Security › Screen & System Audio Recording, then rerun". Never a pass.
      - Bare exits: every "exit N" is the script's own; no pipeline masks a status. Exit 0 pass, 1 assertion.
      - --self-test: refusal paths + the DB/log parsers against fixtures in scripts/qa/fixtures/meeting/;
        posts no events, plays no audio.
      - Cleanup: the walk deletes the meeting it created (through the app's own delete path) at the end —
        no fixture residue.

    RUNTIME DEPENDENCIES (README rule: ship it or manage its install — nothing found on the machine):
      | parakeet-unified-en-0.6b-Q8_0 | downloaded by the app (ModelSetup), sha256-verified | exists |
      | qwen2.5-1.5b-instruct (summary) | downloaded by the app on opt-in | exists |
      | CoreAudio process tap | OS (macOS 14.4+), gated by os_version_gate | exists |
      | say / afplay | macOS built-ins, QA only, never in the app | QA only |

    TESTS: tests/meeting_pipeline_wired.rs (real SQLite file, a 6 s two-track WAV fixture, a stub decoder
    behind the existing seam so it finishes in < 20 s: stop → segments on both tracks → Complete; a second
    run resumes from processed_through); tests/system_audio_verdict_silence_is_not_denial.rs (silent output =
    Unknown, output ran + zeros for < 60 s or in one meeting only = Unknown, output ran >= 60 s + zeros in two
    consecutive meetings = LooksDenied, LooksDenied row still attempts the tap next meeting, in-person
    never attaches); tests/meeting_retention_keeps_untranscribed_audio.rs (NT1); a vitest for the banner row.

    WHAT NOT TO DO
      - Do NOT add ScreenCaptureKit or ask for Screen Recording.
      - Do NOT mark a meeting Complete without segments, and do NOT write a verdict from a silent output.
      - Do NOT drive /Applications/Yap.app from a builder or gate; the walk seat runs meeting-walk.sh.
      - Do NOT run meeting_eval here (the 120 s cap); it belongs to NT0/NT2.

    Critiqued once (self-pass): the first draft only re-ran setup on launch, which still leaves a silent
    first meeting locking every later one — replaced by "always attempt, badge only" (C). It also asserted on
    track 0 words, which a mic in a quiet room can fail; the walk now asserts on track 1, the audio it plays.
  `,
  acceptance: `
    test -x scripts/qa/meeting-walk.sh
    test -f scripts/qa/fixtures/meeting-script.txt
    bash scripts/qa/meeting-walk.sh --self-test ; test $? -eq 0
    test -f desktop/src-tauri/tests/meeting_pipeline_wired.rs
    test -f desktop/src-tauri/tests/system_audio_verdict_silence_is_not_denial.rs
    test -f desktop/src-tauri/tests/meeting_retention_keeps_untranscribed_audio.rs
    grep -q "output_was_running" desktop/src-tauri/src/syscapture.rs
    grep -q "fn reconcile_stranded_meetings" desktop/src-tauri/src/meetings.rs desktop/src-tauri/src/lib.rs desktop/src-tauri/src/meeting_control.rs
    grep -rln "append_meeting_segments" desktop/src-tauri/src | grep -v "db.rs" | grep -q .
    ! grep -n "allow(dead_code)" desktop/src-tauri/src/meeting_asr.rs
    grep -rl "System Audio Recording" desktop/src --include=*.ts --include=*.tsx | grep -q .
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_pipeline_wired ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test system_audio_verdict_silence_is_not_denial ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_retention_keeps_untranscribed_audio ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_manual_start_stop ; test $? -eq 0
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

// yap-notetaker-* — WISPR FLOW NOTETAKER PARITY, YAP'S WAY (local, private). Written 2026-10-16 by the
// yap-notetaker seat (Opus). Wilson 20:30Z: "Wispr Flow is open right now … check the website, they'll
// literally tell you all of their features. You see how to set up Notetaker. I already like what our Yap
// looks like, I like the mascot, but we need to reverse engineer the Notetaker." His screenshot: a
// "Set up Notetaker" card — Take 2-min tour · Connect calendar · Import from Granola or Otter · Set up MCP.
// Sources (cited in ~/Obsidian/Wilson-Brain/Projects/Yap/wispr-notetaker-parity-2026-10-16.md):
// https://wisprflow.ai/notetaker, https://docs.wisprflow.ai/articles/1737760052 (calendar: Google +
// Outlook, NOT iCloud; Cloud Sync required), https://docs.wisprflow.ai/articles/9551372685 (remote MCP,
// read-only, no dictation history), https://docs.wisprflow.ai/articles/5166327995 (Granola/Otter kept as
// second MCP connectors). ORDER: after yap-uiux-2026-10-16-Z in this file (same lane, sequential), all
// three DEPEND on yap-meetings-fix. Not blockers for the uiux pass. They absorb the core of the
// panel-gated yap24-NT4, NT5, NT8 and satisfy their preflight markers.
// Yap stays local: nothing leaves the Mac unless the user picks a cloud model. We do NOT copy: Cloud Sync,
// remote account-wide MCP, auto-start recording, web search in ask, hosted share links, persistent voiceprints.
// Consent sentence (one, everywhere a recording is offered): "Recording is sacred: tell everyone on the call
// that Yap is taking notes, and it stays on this Mac."
// Review: Opus only (review-opus + review-opus2). SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap-notetaker-onboard', prompt: 'yap-notetaker', branch: 'loop/yap-notetaker-onboard-set-up-card', gated: null,
  title: 'A "Set up Notetaker" card: 2-minute tour, connect Apple Calendar, import from Granola or Otter, set up MCP — progress dots, dismissible, reopen from Help',
  preflight: `
    test -f desktop/src/meetings/NotetakerSetupCard.tsx
    test -f desktop/src-tauri/src/meeting_import.rs
    test -d desktop/yap-mcp
  `,
  spec: `
    Panel: not required — Wilson named the four steps from Wispr's card (2026-10-16).
    DEPENDS: yap-meetings-fix (a real transcript must exist for the tour, import and MCP to mean anything).

    THE CARD: top of the Meetings view until done or dismissed; four rows, one progress dot each (empty /
    done), a plain verb per row, contained panel (surface + border, pinned footer), mascot allowed, no new
    visual language. Dismiss = X with "Reopen anytime from Help"; Help menu gains "Set up Notetaker".
    State persists in settings_kv notetaker_setup_v1 (JSON: steps done + dismissed).
      1. Take the 2-minute tour — a local scripted walkthrough on a bundled SAMPLE meeting (a read-only
         fixture row, never recorded): start, live meters, stop, transcript, notes, export. Done = finished.
      2. Connect calendar — v1 Apple Calendar via EventKit, READ-ONLY, on-device (EKEventStore
         requestFullAccessToEvents on 14+, NSCalendarsFullAccessUsageDescription in Info.plist). This also
         sees Google/Exchange accounts the user already added to macOS Calendar — say so in one line.
         Google/Microsoft OAuth listed as "Coming". Done = access granted and >= 1 calendar readable.
      3. Import from Granola or Otter — pick a file or folder. Otter: .txt / .docx / .srt exports (formats
         per a third-party guide — UNVERIFIED against Otter's own docs: the builder must confirm with real
         sample exports and record what it confirmed in the PR). Granola: export format UNVERIFIED — research
         first (Granola help pages; its Markdown/Notion export); if no stable file format is documented,
         ship Otter only and show Granola as "Coming" with the reason. Imports become meetings with
         source = 'imported:otter' | 'imported:granola', segments + any summary, searchable via FTS.
         Never call their cloud APIs; files only.
      4. Set up MCP — Yap has NO MCP server today (grep: zero MCP code). Build desktop/yap-mcp: a local
         stdio MCP server (official Rust SDK rmcp — pin a released version after reading its docs), shipped
         inside the bundle (Contents/MacOS/yap-mcp, signed with the app), opening wilson_voice.db READ-ONLY.
         Resources/tools: list_meetings, get_transcript, get_summary, list_action_items, search_meetings.
         Permission table (settings_kv mcp_permissions_v1), each OFF until the user turns it on: meetings
         list · transcripts · summaries · action items. Dictation history is NEVER exposed (Wispr's line
         too). The row shows a "Copy config" button with the claude_desktop_config / Cursor JSON snippet
         pointing at the bundled binary. Done = config copied and a self-test call answered.

    RUNTIME DEPENDENCIES: | EventKit | OS | | yap-mcp | built + signed in our bundle | | rmcp crate | vendored
    at build time, pinned | — nothing found on the machine.

    TESTS: vitest NotetakerSetupCard (4 rows, dots, dismiss persists, Help reopens); cargo test
    meeting_import (Otter txt/srt fixtures → segments; malformed file → a plain error, no partial row);
    cargo test -p yap-mcp (permission OFF → resource refused; read-only: an INSERT through the connection
    fails); tests/calendar_permission_readonly.rs (no EKEventStore save/remove call anywhere: grep gate).

    WHAT NOT TO DO: no OAuth, no network in import or MCP, no write access over MCP, no new mascot.
    Critiqued once (self-pass): the draft exposed MCP by default; Wispr's "account-wide, no per-meeting
    consent" is exactly what we are not copying, so every class now starts OFF.
  `,
  acceptance: `
    test -f desktop/src/meetings/NotetakerSetupCard.tsx
    test -f desktop/src-tauri/src/meeting_import.rs
    test -d desktop/yap-mcp
    grep -q "NSCalendarsFullAccessUsageDescription" desktop/src-tauri/Info.plist
    ! grep -rn "saveEvent\|removeEvent\|save(event\|remove(event" desktop/src-tauri/src
    grep -q "Set up Notetaker" desktop/src/meetings/NotetakerSetupCard.tsx
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
    cd ${APP} && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test -p yap-mcp ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol meeting_import ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test calendar_permission_readonly ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'yap-notetaker-detect', prompt: 'yap-notetaker', branch: 'loop/yap-notetaker-detect-offer-to-record', gated: null,
  title: 'Yap notices a meeting — a calendar event with a call link in the next 2 minutes, or Zoom/Meet/Teams/FaceTime with live audio — and the pill offers "Record this meeting?" with the consent line; it never auto-records',
  preflight: `
    test -f desktop/src-tauri/src/meeting_detect.rs
    test -f desktop/src-tauri/tests/calendar_prompt_policy.rs
  `,
  spec: `
    Panel: not required — Wilson asked for Wispr parity (2026-10-16); Wispr: "Notetaker will detect that you
    are in a call and ask if you want to start taking notes" (wisprflow.ai/notetaker).
    DEPENDS: yap-meetings-fix, yap-notetaker-onboard (EventKit access lives there).

    SIGNALS (any one raises the offer; meeting_detect.rs, a pure policy fn + thin OS adapters):
      a. Calendar: an EventKit event starting within 2 minutes (or in progress < 5 min) whose URL, location
         or notes contain a conferencing link (zoom.us/j, meet.google.com, teams.microsoft.com,
         facetime.apple.com, whereby, webex). Read-only.
      b. App: a running zoom.us / Microsoft Teams / FaceTime process AND that app's audio is live
         (CoreAudio process object kAudioProcessPropertyIsRunningInput or IsRunningOutput, macOS 14.4+; below
         14.4 the app signal is off and only the calendar signal runs). Meet in a browser: a browser window
         title matching "Meet - " / "meet.google.com" via Accessibility (already granted for paste) AND the
         browser's audio live.
    OFFER: the pill (locked pixel-art mascot) expands with "Record this meeting?" + the meeting title when
    known + the consent sentence + [Record] [Not now]. Record starts kind = Virtual. Not now snoozes that
    event / app session (no repeat for the same calendar event or the same app run). Never starts recording
    on its own; there is no auto-start setting. Settings › Meetings: "Offer to record detected meetings"
    (default ON) and "Use my calendar" (default follows step 2 of the setup card). Polling: event-driven
    (NSWorkspace launch/terminate + an EventKit change notification + a single 30 s timer) — never faster.

    TESTS: tests/calendar_prompt_policy.rs (table: link+2 min → offer; no link → none; snoozed → none; in
    progress 4 min → offer, 6 min → none); meeting_detect unit tests for the app table (audio live required;
    Zoom open but silent → none); vitest for the pill offer (consent sentence present; Not now emits snooze).

    RUNTIME DEPENDENCIES: | EventKit, NSWorkspace, CoreAudio process objects | OS |.
    WHAT NOT TO DO: no auto-record, no network, no reading meeting content to decide, no polling < 30 s.
    Critiqued once (self-pass): "Zoom is running" alone would prompt all day for people who leave Zoom open;
    the app signal now requires live audio from that app.
  `,
  acceptance: `
    test -f desktop/src-tauri/src/meeting_detect.rs
    test -f desktop/src-tauri/tests/calendar_prompt_policy.rs
    grep -q "Recording is sacred" desktop/src/pill/*.tsx desktop/src/meetings/*.tsx
    ! grep -rn "auto_record\|autoRecord" desktop/src desktop/src-tauri/src
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test calendar_prompt_policy ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol meeting_detect ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'yap-notetaker-notes', prompt: 'yap-notetaker', branch: 'loop/yap-notetaker-notes-speakers-summary-actions', gated: null,
  title: 'Notes worth reading: transcript in speaker turns, a summary with decisions and action items from the user’s chosen model, searchable, Markdown export, MCP-readable — nothing leaves the Mac unless the user picks a cloud model',
  preflight: `
    grep -q "CREATE TABLE IF NOT EXISTS meeting_actions" desktop/src-tauri/src/db.rs
    test -f desktop/src-tauri/tests/meeting_notes_auto_summary.rs
    test -f desktop/src-tauri/tests/meeting_notes_stay_local.rs
  `,
  spec: `
    Panel: not required — Wilson 2026-10-16 (Wispr parity). Wispr: live transcript "separates you from other
    speakers"; summary pulls out "timelines, decisions, and next steps"; labels "not voiceprints".
    DEPENDS: yap-meetings-fix (segments exist), yap-notetaker-onboard (yap-mcp exists).

      1. Speaker turns: two-track meetings label track 0 "You" and track 1 "Call" (TrackSpeakers); in-person
         and track 1 with >1 voice run the SHIPPED diarize sidecar (diarize.rs; diarize::pool() gets its first
         caller) after transcription: "Speaker 1/2/…", renamable once and applied across the meeting (Wispr
         parity). Calendar attendees (when step 2 is done) are offered as rename choices. Embeddings are per
         meeting and discarded — no cross-meeting voiceprints.
      2. Live transcript (light): while recording, a rolling decode every ~10 s on each track shows the last
         lines under You / Call in the Meetings view. The final transcript replaces it after Stop.
      3. Summary + action items: automatic after Complete via the user's chosen model — local default
         (polishModel; qwen2.5-1.5b today) or a BYO-key cloud model ONLY if the user picked one in Settings.
         Sections: Summary · Decisions · Action items (owner, text, cited segment) · Open questions.
         Action items are rows in meeting_actions (meeting_id, owner, text, segment_id, done). No model =
         "Add a summary model in Settings", never an error.
      4. Search: FTS over segments + summaries + action items, from the Meetings search box.
      5. Export: Markdown with speakers, timestamps, summary, action items as "- [ ]" (export_meeting_markdown).
      6. MCP: yap-mcp resources return the same transcript/summary/action items, under the permission table.
    PRIVACY TEST: tests/meeting_notes_stay_local.rs — with a local model, the summarize path makes zero
    network calls (a deny-all HTTP client seam that panics on use).

    RUNTIME DEPENDENCIES: | wespeaker CAM++ / diarize sidecar | downloaded by the app (already in models/) |
    | summary model | downloaded by the app on opt-in | | cloud model | only with the user's own key |.
    TESTS: tests/meeting_notes_auto_summary.rs (Complete → summary + >= 1 action item row on the fixture
    transcript with a stub model; no model → state message); diarize wiring test on a 2-voice fixture;
    vitest for the Meetings detail (speaker turns, rename applies to all, action item checkbox persists).
    WHAT NOT TO DO: no upload of audio or transcript by default; no persistent voiceprints; no web search.
    Critiqued once (self-pass): live transcript on a 3B-param budget would starve dictation; it is capped to
    a light rolling decode that yields to any dictation take.
  `,
  acceptance: `
    grep -q "CREATE TABLE IF NOT EXISTS meeting_actions" desktop/src-tauri/src/db.rs
    test -f desktop/src-tauri/tests/meeting_notes_auto_summary.rs
    test -f desktop/src-tauri/tests/meeting_notes_stay_local.rs
    grep -rln "diarize::pool()" desktop/src-tauri/src | grep -v "diarize.rs" | grep -q .
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
    cd ${APP} && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test -p yap-mcp ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_notes_auto_summary ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_notes_stay_local ; test $? -eq 0
  `,
})

// yap-rereview-followups-2026-10-16 — the advisories the #216/#217 re-reviews raised and did not block on.
// Written 2026-10-16 by the items-followups seat. Review: Opus only (review-opus + review-opus2).
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap-structure-followups', prompt: 'yap-format-structure', branch: 'loop/yap-structure-followups-keep-every-word-finish-every-list', gated: null,
  title: 'The structure stage keeps every word and finishes every list: narration "Then"/"Finally" survive, four-step lists stay one list, product names keep their casing',
  // Pre-flight asserts COMPLETION (the six regression inputs live in structure.rs's tests and the
  // module's tests pass), never a dependency: a passing pre-flight retires the item as already-done.
  preflight: `
    grep -qF 'Then the meeting ran long.' desktop/src-tauri/src/structure.rs
    grep -qF "Finally I'm home." desktop/src-tauri/src/structure.rs
    grep -qF 'Then we test it. Then we ship it.' desktop/src-tauri/src/structure.rs
    grep -qF 'Then you tap General. Finally, tap Reset.' desktop/src-tauri/src/structure.rs
    grep -qF 'Finally at checkout' desktop/src-tauri/src/structure.rs
    grep -qF 'eBay' desktop/src-tauri/src/structure.rs
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib structure ; test $? -eq 0
  `,
  spec: `
    Panel: not required — FIX (re-review advisories on PR #217, merged cf7c8c2).
    Wilson's words (2026-10-16): "You see how Wispr Flow does the formatting? That's what Yap needs to be able to do. It knew I was doing a list of things."
    DEPENDS: none (PR #217 merged cf7c8c2).

    WHY: the structure stage (desktop/src-tauri/src/structure.rs) turns spoken enumerations into lists. The re-review
    found cases where it still drops a word, stops a list early, or forces prose. Each advisory below becomes ONE unit
    test in structure.rs with the exact input and expected output; write the test first, watch it fail, then fix.

    TESTS (exact input -> expected output):
      1. Narration after a list keeps "Then". Input: a list followed by "Then my wife called…" and, separately,
         "Then the meeting ran long." Expected: the sentence stays as prose with "Then" intact. The narration check
         knows only pronouns today; extend it so a sentence-initial "Then" followed by a noun phrase (a determiner or a
         possessive: my, the, our, his, her, their, a) is narration, not a list cue.
      2. "Finally I'm home." keeps "Finally" (a "Finally" followed by a pronoun + contraction or a pronoun is narration).
      3. "First, we design it. Then we build it. Then we test it. Then we ship it." -> ONE list of FOUR items
         (today it stops at three and drops the last step to prose). Find why the run ends at three and let a list
         continue while each following sentence opens with a cue.
      4. "First, open Settings. Then you tap General. Finally, tap Reset." -> a list of THREE items (regressed to prose
         because "you" read as narration). An imperative-with-"you" step inside an established list is a step.
      5. "Finally at checkout, pay…" keeps every word and is not forced to prose only because of "at".
      6. Product names keep their dictated casing when list items are capitalised: iPhone, eBay, macOS stay exactly
         as dictated (assert "iPhone", "eBay", "macOS" appear verbatim in the output; only a lowercase ordinary first
         letter is capitalised). Use a small never-recase list or the rule "first letter is lowercase and the second
         letter is uppercase -> leave alone".

    GOLDENS: desktop/yap-polish/fixtures/structure/* stay unchanged. If one must change, name that fixture and the reason
    in the PR body.
    NOTHING DROPPED: the existing every-input-word-appears-in-the-output check runs over all six new inputs.
    RUNTIME DEPENDENCIES: none new.
    WHAT NOT TO DO: no model calls, no new crate, no rewording of the user's words.
    Critiqued once (self-pass): widening "Then" as narration could break genuine "First ... Then ..." lists; test 3 and 4
    guard that, and the narration rule only applies when a determiner or possessive follows.
  `,
  acceptance: `
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib structure ; test $? -eq 0
    cd ${APP} && npx tsc --noEmit
  `,
})

ITEMS.push({
  id: 'yap-recovery-followups', prompt: 'yap-hotkey-stop', branch: 'loop/yap-recovery-followups-no-duplicates-no-race-backend-confirm', gated: null,
  title: 'Recovered long takes: no duplicate history entries, no race, confirmation enforced in the backend',
  // Pre-flight asserts COMPLETION (the four named tests exist), never a dependency: a passing
  // pre-flight retires the item as already-done, a failing one dispatches the builder. PR #216 is MERGED
  // (2026-10-17, fff7816), so the gh pr view 216 state check (first acceptance command) now passes; it is
  // kept as a guard so a build cut from a base without #216 can never go green or open a mergeable PR.
  preflight: `
    git grep -q 'fn db_b_two_concurrent_saves_yield_one_row' -- desktop/src-tauri
    git grep -q 'fn db_b_keep_without_confirm_is_refused' -- desktop/src-tauri
    git grep -q 'fn db_b_keep_while_retry_in_flight_is_refused' -- desktop/src-tauri
    git grep -q 'fn db_b_retry_and_keep_after_live_partial_leave_one_row' -- desktop/src-tauri
  `,
  spec: `
    Panel: not required — FIX (re-review advisories on PR #216).
    Wilson's words (2026-10-16): "A crash or quit mid-long-take loses nothing."
    DEPENDS: PR #216 (long-take recovery) is MERGED (2026-10-17, fff7816); the FIRST acceptance command checks it and
    passes. If it ever fails, stop, push nothing, report BLOCKED on #216. Do not re-implement #216's work here.

    ADVISORIES, each with a test:
      (a) Two concurrent saves can add the same take twice. Add a backend idempotency key per take (the take id, a
          UNIQUE column or an in-memory claim taken under the same lock as the insert) so the second save is a no-op
          that returns the first entry. Test: two concurrent saves of one take yield exactly one history row.
      (b) The "Keep what was transcribed" confirmation exists only in the UI. The backend command must require an
          explicit confirm flag (confirm: true; absent or false returns a typed refusal) and must refuse while a
          retry for that take is in flight. Tests: no flag -> refused, nothing written; flag while retry in flight ->
          refused; flag otherwise -> saved.
      (c) Retry or Keep on a live take that partly failed creates a second history entry beside the one saved live.
          Merge into the existing entry (update its text and status) instead of inserting. Test: after a live partial
          save, Retry and Keep each leave exactly one history row for the take.
      (d) The toast after a failed Finish reads like engineer output. Rewrite it in plain words naming what the user
          can do: Retry, Keep what was transcribed, or Discard. No error codes, no ids, no stack text. Vitest asserts
          the exact copy and that it names all three actions.
      (e) When the recovered words are entirely a repetition loop, or the resume point cannot be read, the retry already
          re-decodes the whole clip from the start. Do NOT flag a gate cut in that case: flag only when the gated
          recovered words are non-empty AND shorter than the raw, so a clean full decode converts on its own. Test:
          both histories (all-loop recovered words; unreadable resume point) convert without Keep.
      (f) The row's "Already transcribed" line must show the GATED recovered words (the same text as "Transcribed so
          far"), never the raw loop. Test: vitest or Rust test as appropriate, asserting a row whose raw text is a loop
          renders the gated text on both lines.
      (g) The hallucination gate drops every word after a loop. For retries, keep clean words that resume after a short
          loop (bounded: loop <= N tokens followed by >= M distinct tokens; pick N and M as named constants) so Keep does
          not discard 40 clean words after "the" x8. Test with that exact input: "the" repeated 8 times followed by 40
          distinct clean words keeps all 40 on retry.
      (h) With no model installed, or the audio file missing, Finish must still offer Keep when words are saved, and the
          missing-audio path must never delete the row's recovered words. Test both: no model -> Keep offered; missing
          audio -> recovered words still present after the attempt.

    TESTS: backend tests in the db tests module, named EXACTLY (the pre-flight greps for them):
    db_b_two_concurrent_saves_yield_one_row (a), db_b_keep_without_confirm_is_refused and
    db_b_keep_while_retry_in_flight_is_refused (b), db_b_retry_and_keep_after_live_partial_leave_one_row (c);
    desktop/src-tauri/tests/long_take_recovery.rs
    extended for the same three; a vitest next to the toast component for (d).
    RUNTIME DEPENDENCIES: none new.
    WHAT NOT TO DO: no schema change that needs a destructive migration; no UI-only enforcement; do not weaken the
    UI confirmation, the backend check is in addition.
    Critiqued once (self-pass): an in-memory-only idempotency claim would not survive a restart, so the key is also
    persisted with the row; the confirm flag defaults to refusing so an old caller cannot bypass it.
  `,
  acceptance: `
    gh pr view 216 -R wilsonguenther-dev/wilson-voice --json state -q .state | grep -qx MERGED || { echo "BLOCKED: PR #216 (long-take recovery) is not merged yet; this item builds on it"; exit 1; }
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib db_b_ ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test long_take_recovery ; test $? -eq 0
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
  `,
})

// yap-review-pass-2-audit (2026-10-17T01:2xZ, seat yap-pass-summary, read-only audit of review pass
// wf_d0209003-efe 20:17Z->01:20Z). Two items, FIX, no panel (harness + test hygiene, unambiguous):
//   1. THE LAND GATE WAS CI-ONLY. All six merged PRs (#178 #192 #195 #212 security, #196 #213 standard)
//      and #214 landed with NO reviewer PASS on the merged head. The reviewers' verdicts were BLOCK on the
//      pre-fix heads; the fix-land seat fixed its own findings and posted "Primary/Independent adversarial
//      review (Opus)" bodies on the new head itself ("Verdict at review time: BLOCK. Disposition after the
//      fix round: FIXED"). The land sweeper's gate was the CI check on headRefOid plus sidecar build, fmt and
//      clippy ("I did not run cargo test locally"). template.mjs says "THERE IS NO SECOND REVIEW ROUND IN THIS
//      PASS" and CI_GATE never reads a review record. #196's only PASS was on cb60606; it landed at 16f7241
//      after a float.css conflict resolution nobody reviewed. #214 was merged by the drain seat on its own
//      fixes to Codex-bot suggestions, with no Opus seat at all.
//   2. meeting::tests ARE NOT HERMETIC. SessionConfig::new (meeting.rs) defaults env to SystemEnv, whose
//      battery() runs /usr/bin/pmset. Seven meeting::tests failed at recon with
//      "LowBattery { percent: 11, budgeted: 10800s }" because the host Mac was at 11% on battery.

ITEMS.push({
  id: 'yap-land-gate-pass-required', prompt: 'yap-land-gate', branch: 'loop/yap-land-gate-pass-required-review-record-on-head', gated: null,
  title: 'No PR lands without an Opus PASS bound to its head: one seat for standard, two different seats for security, enforced by scripts/loop/review-gate.mjs in every merge path (LAND, fix-land, land sweeper, drain)',
  preflight: `
    test -f scripts/loop/review-gate.mjs
    test -f scripts/loop/review-gate.test.mjs
    grep -q "review-gate.mjs gate" scripts/loop/template.mjs
    ! grep -q "THERE IS NO SECOND REVIEW ROUND IN THIS PASS" scripts/loop/template.mjs
  `,
  spec: `
    Panel: not required — harness defect found by the review pass 2 audit (2026-10-17). Opus only, every seat.
    EVIDENCE (pass wf_d0209003-efe): every merged head carries only the FIXER's disposition review, never a
    reviewer PASS. #178 96e5380, #192 45fa4cd, #195 1dfb328, #212 0b7449b, #213 d956c11: reviewer verdicts BLOCK
    on the earlier heads, fixer self-attested FIXED on the merged head. #196 16f7241: reviewer PASS on cb60606
    only. #214 8bfd8a5: no Opus review at all (drain fixed Codex-bot suggestions and merged). The sweeper's gate
    was CI on headRefOid + build/fmt/clippy, no cargo test.

      1. Port Learn's gate: scripts/loop/review-gate.mjs, modelled line for line on Drivia Learn's H-GATE-1 gate
         (Learn repo, main branch, file scripts/loop/review-gate.mjs; read it with gh api). DEFAULT_REPO wilsonguenther-dev/wilson-voice,
         RECORD_MARKER "loop-review (Opus)", seats review-opus and review-opus2, EXIT_REVIEW 6, EXIT_USAGE 64,
         REVIEW_AUTHORS the account the seats post as. Subcommands: gate --pr N [--head SHA], record --pr N
         --seat --lens --head --verdict PASS|BLOCK --blocking N, klass --pr N. The record is a PR comment whose
         first line is the marker and which names seat, lens, head (40 hex) and "verdict: PASS blocking=0" or
         "verdict: BLOCK blocking=n". record refuses a sha that is not the PR's current head.
      2. Decision against the CURRENT head only. SECURITY class (the triage klass rule: SECURITY_IDS,
         SECURITY_PATHS, a security label, or a file list longer than one page) needs a PASS from review-opus
         AND a PASS from review-opus2 on the head. STANDARD needs one PASS from either seat on the head. There is
         NO unreviewed exception in Yap: no record on the head refuses. Not a record: a fixer seat (any seat
         starting "fix"), a drain/sweeper/land seat, an unknown seat, a bot author, an edited comment, PASS with
         blocking > 0, a short head. A seat whose newest record on the head is BLOCK refuses. Records on older
         heads are history and never count, so any push (a rebase, a conflict resolution, a fix) needs a fresh
         PASS.
      3. Wire it into every agent that can merge: the build-mode LAND step, fix-land, the land sweeper and the
         drain. Each runs "node scripts/loop/review-gate.mjs gate --pr N --head <headRefOid>" immediately before
         gh pr merge --match-head-commit <same sha>, and does not merge on a non-zero exit (it labels the PR
         review-pending and names the missing seat). CI_GATE and the local gate stay as they are — they are
         necessary, not sufficient.
      4. The review pass gets its second round. After fix-land pushes a fix it does NOT merge: the chain
         re-dispatches the reviewer seat(s) that blocked (same lens, fresh context) at the new head, and they
         post their own record with "record". Cap: two fix rounds; a third BLOCK ends as needs-human. Replace
         the "THERE IS NO SECOND REVIEW ROUND IN THIS PASS" contract text accordingly. Reviewer seats post the
         record themselves; the fixer never posts a verdict body titled as a review.
      5. build.mjs validation: every prompt that contains "gh pr merge" also contains "review-gate.mjs gate";
         review-gate.mjs is in the harness SECURITY_PATHS; and ONLY reviewer-seat prompts (review-opus,
         review-opus2) contain "review-gate.mjs record" — a fix, fix-land, land, sweeper or drain prompt that
         contains it fails validation.
      6. TRUST LIMIT — say it, do not over-claim it. Every seat posts through the same GitHub account, so the
         record's seat field is self-declared: the gate CANNOT authenticate which seat wrote a record, and a
         seat that runs "record --seat review-opus" is indistinguishable from the real reviewer. What this item
         enforces is (a) a PASS bound to the current head exists, and (b) only reviewer prompts are ever handed
         the record subcommand (validation in 5). Write that limit, in those words, in the review-gate.mjs
         header and in the PR body. Never describe the gate as authenticating or unforgeable. Seat
         authentication (per-seat credentials, or records the controller signs) is out of scope here.
    TESTS: scripts/loop/review-gate.test.mjs (node --test), hermetic — gh is a stub fed from fixture JSON via an
    injectable runner, never the network. Cases: (a) CI green, MERGEABLE, no record on any head -> gate exit 6;
    (b) standard PR, one review-opus PASS on the head -> 0; (c) PASS on an older head only (the #196 shape) -> 6;
    (d) security PR, PASS from review-opus only -> 6; two PASSes both from review-opus -> 6; review-opus +
    review-opus2 PASS on head -> 0; (e) a fix-land seat record saying PASS on the head (the #178 shape) -> 6;
    (f) newest review-opus2 record on head is BLOCK -> 6; (g) edited record -> 6; (h) record --head with a sha
    that is not the current head -> 64; (i) build.mjs validation fails on a stub fix-land prompt containing
    "review-gate.mjs record".
    WHAT NOT TO DO: no Codex/Gemini/bot review counts; never accept a verdict from a loop log instead of the PR.
  `,
  acceptance: `
    test -f scripts/loop/review-gate.mjs
    test -f scripts/loop/review-gate.test.mjs
    node --test scripts/loop/review-gate.test.mjs
    grep -q "review-gate.mjs gate" scripts/loop/template.mjs
    ! grep -q "THERE IS NO SECOND REVIEW ROUND IN THIS PASS" scripts/loop/template.mjs
    cd ${APP} && npm run loop:build && npm run loop:validate
  `,
})

ITEMS.push({
  id: 'yap-tests-hermetic-power', prompt: 'yap-tests-hermetic', branch: 'loop/yap-tests-hermetic-power-injected-power-source', gated: null,
  title: 'Meeting tests stop reading the host battery: an injected PowerSource, pmset only in production wiring, so cargo test passes on a Mac at 5% on battery',
  preflight: `
    grep -q "trait PowerSource" desktop/src-tauri/src/meeting.rs
    grep -q "YAP_HOST_BATTERY_OVERRIDE" desktop/src-tauri/src/meeting.rs
  `,
  spec: `
    Panel: not required — test-design defect (non-hermetic tests), found by the review pass 2 audit.
    EVIDENCE: recon 2026-10-16T20:2xZ, cargo test exit 101, 7 failures all in meeting::tests
    (a_dictation_take_mid_meeting_costs_the_meeting_nothing, a_meeting_registers_and_deregisters_the_active_capture,
    a_meeting_session_holds_its_own_audio_stream_for_its_whole_length, a_meeting_whose_stream_refuses_never_starts,
    a_refused_start_releases_its_claim, a_second_meeting_is_refused_not_silently_swapped_in,
    a_session_that_lost_the_slot_finalizes_partial_and_leaves_it_alone), panic "LowBattery { percent: 11,
    budgeted: 10800s }"; pmset showed the host on battery at 11%. Root cause: SessionConfig::new sets
    env: Arc::new(SystemEnv { path }), SystemEnv::battery() calls probe_battery() (/usr/bin/pmset -g batt), and
    the tests build sessions with ..SessionConfig::new(&dir, ...). The CaptureEnv seam exists
    (tests/matrix_row4_disk_preflight.rs uses FakeDisk) but the default constructor wires the real machine, and
    tests/matrix_row3_mic_denied_system_only.rs plus matrix_row4 inherit it for battery and thermal too.

      1. A PowerSource trait (fn battery(&self) -> BatterySnapshot) in meeting.rs. PmsetPower is the only
         implementation that runs pmset. SystemEnv takes its battery from a PowerSource it is given.
      2. Production wiring passes the real machine explicitly: the session constructors used by
         meeting_control.rs (~942) and syscapture.rs (~2106, ~2749) build SystemEnv with PmsetPower. The
         generic constructor used by tests (SessionConfig::new or a new SessionConfig::hermetic) defaults to a
         HermeticEnv: AC power at 100%, nominal thermal, ample disk — so no unit or integration test reads pmset,
         statvfs or the thermal state unless it injects them on purpose.
      3. A debug-only seam to PROVE hermeticity: PmsetPower honours YAP_HOST_BATTERY_OVERRIDE="<percent>,<ac|battery>"
         under cfg(debug_assertions) only, so a test run can simulate a dying host. Release builds compile it out.
      4. Battery behaviour stays tested, through injection: a test that injects 5% on battery with the default
         3 h budget asserts PreflightError::LowBattery, and one on AC asserts it starts.
    TESTS: meeting::tests::hermetic_power_override_is_real (with the override set to 5,battery, PmsetPower reports
    5% on battery — so the acceptance run below is not vacuous); the injected-5% LowBattery test; every existing
    meeting::tests case unchanged in intent.
    WHAT NOT TO DO: do not mark the seven tests #[ignore]; do not loosen the LowBattery threshold; do not read the
    override in release builds.
  `,
  acceptance: `
    grep -q "trait PowerSource" desktop/src-tauri/src/meeting.rs
    grep -q "YAP_HOST_BATTERY_OVERRIDE" desktop/src-tauri/src/meeting.rs
    cd ${APP}/src-tauri && YAP_HOST_BATTERY_OVERRIDE="5,battery" YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib meeting:: ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_HOST_BATTERY_OVERRIDE="5,battery" YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test matrix_row3_mic_denied_system_only --test matrix_row4_disk_preflight ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib meeting::tests::hermetic_power_override_is_real ; test $? -eq 0
  `,
})
