// yap-lifecycle-2026-10-16 — QUIT MEANS QUIT, REOPEN MEANS A WINDOW. Written 2026-10-16 by the
// yap-prep seat from the 10-06 prompt D6 and the 10-06 context Facts 12-14 / open item 1.
// Wilson, 2026-10-06: "the way it shuts down is wrong — I can't open the app but I can still see
// the sideways pill." FIX, no panel (unambiguous intent). Item #1 of the 10-06 new items.
// Headless only: no agent launches the GUI app (see HAZARD below). SHARED PREAMBLE + STANDARD
// GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap-lifecycle-2026-10-16', prompt: 'yap-lifecycle', branch: 'loop/yap-lifecycle-2026-10-16-quit-reopen-teardown', gated: null,
  title: 'Quit means quit, reopen means a window: RunEvent::Reopen, one clear Quit, the pill gone at quit, bounded teardown, RSS flat under load',
  preflight: `
    grep -q 'RunEvent::Reopen' desktop/src-tauri/src/lib.rs
    test -f desktop/src-tauri/tests/lifecycle.rs
    test -f scripts/lifecycle-soak.sh
  `,
  spec: `
    Panel: not required — FIX (10-06 prompt D6: this is item #1 of the new items)
    DEPENDS: yap-flakes-2026-10-16 (a green main). Independent of yap24-NT and yap24-PILL.

    EVIDENCE (10-06 context Facts 12-14; line numbers re-checked on main 6f40697, 2026-10-16)
      - lib.rs ~5605: the main window's CloseRequested handler calls prevent_close() and hide():
        the red X hides the window and never quits. The activation policy is Regular (~5586), so
        the Dock icon stays.
      - lib.rs ~6149: the .run() closure matches only RunEvent::Exit. Nothing in src/ handles
        RunEvent::Reopen, so a Dock click or a Finder double-click on a Yap whose window is hidden
        shows nothing. That is "I can't open the app".
      - lib.rs ~5342: the single-instance callback calls focus_main_window (~4535), yet yap.log shows
        "second instance launched" once (2026-09-12) in the whole log.
      - The pill is an NSPanel in the same process (float_pill.rs), so a visible pill means the
        process is alive. Tray Quit (~5837) stops any meeting, checkpoints the WAL, then app.exit(0).
      - Memory: Wilson's running Yap was 2.2 GB RSS after 2.5 days on 10-06 (pid 23686) and 1.4 GB
        after 2d15h on 2026-10-16 (pid 1640), against a ~928 MB lazy-load start. A leak is
        suspected and UNMEASURED.

    DO
      1. RunEvent::Reopen (has_visible_windows false) shows, unminimizes and focuses the main window
         through ONE function shared with the single-instance callback and the tray "Open Yap".
         Put the decision in a pure function (event, window state) -> action with unit tests; the
         run-loop arm only dispatches.
      2. ONE unambiguous Quit. Cmd-Q (app menu), tray "Quit Yap" and the Dock's Quit all reach the
         same quit path. The red X keeps hiding (Yap is a menu-bar utility), and the FIRST time it
         hides, the window's own surface says once where Yap went and how to quit. No modal.
      3. The pill is ordered out as the FIRST step of quit, before the meeting stop and the WAL
         checkpoint, so a slow teardown never leaves a pill on screen.
      4. Bounded teardown. Every quit step has a deadline, the whole quit finishes within 3 s, and a
         step that misses its deadline is logged and skipped before the process exits. Log one
         "exit teardown <ms>" line with per-step timings.
      5. Stale instance. If the single-instance handoff to a running instance gets no answer
         within 2 s, log it and say so instead of exiting silently.
      6. RSS flat under load. scripts/lifecycle-soak.sh <runs> runs the BUILT binary headless
         (YAP_DATA_DIR="$(mktemp -d)/yap-state", --transcribe-file
         tests/fixtures/quick-brown-fox-16k.wav, or a --smoke soak mode if the builder adds one) in
         a loop, samples RSS with ps every iteration, prints the series, and exits non-zero when
         RSS after the warm-up iterations grows more than 10 percent. Find and fix what it shows
         (idle model unload; tie to yap24-OS5 "idle means idle"). Record the numbers in the PR.
      7. HAZARD: a GUI launch of a second copy reaches the single-instance plugin and would FOCUS
         WILSON'S RUNNING YAP. No agent launches the GUI app. Every automated proof is a cargo test
         or the headless CLI path (cli::handle_args exits before the Tauri builder). Never kill or
         signal any /Applications/Yap.app process.
      8. Keep Tauri; app-sandbox stays false; never rename the bundle id or the data dir.

    WILSON'S WALK (after merge, on his installed build; NOT an agent step, the PR body lists it):
      scripts/rebuild_app.sh; close the window, click the Dock icon: the window shows; Cmd-Q: the
      process is gone AND the pill is gone; relaunch: the window shows; open -a Yap a second time:
      the first instance focuses.
  `,
  acceptance: `
    grep -q 'RunEvent::Reopen' desktop/src-tauri/src/lib.rs
    test -f desktop/src-tauri/tests/lifecycle.rs
    test -x scripts/lifecycle-soak.sh
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    (cd desktop/src-tauri && cargo test --features custom-protocol --test lifecycle)
    test "$(cd desktop/src-tauri && cargo test --features custom-protocol --test lifecycle -- --list 2>/dev/null | grep -c ': test$')" -ge 4
    (cd desktop/src-tauri && cargo build --release --features custom-protocol)
    bash scripts/lifecycle-soak.sh 40
  `,
})
