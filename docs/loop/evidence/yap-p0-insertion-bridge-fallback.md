ROOT-CAUSE-PROBE yap-p0-insertion-bridge-fallback

# yap-p0-insertion-bridge-fallback — which "bridge" was broken

Gathered 2026-10-19T03:20Z on macOS 26.6.2 (25G83), branch
loop/yap-p0-insertion-fallback-copy-card off origin/main 3569ce4, BEFORE any code change.

## (a) yap.log since 2026-10-16 (read-only grep)

File: ~/Library/Application Support/WilsonVoice/logs/yap.log (+ rotations), lines whose
timestamp is >= 2026-10-16.

    since 2026-10-16: 'pasted=false'          = 16
    since 2026-10-16: 'paste not confirmed'   = 3
    since 2026-10-16: 'Enable Accessibility'  = 0
    since 2026-10-16: 'pasted=true'           = 42
    since 2026-10-16: 'paste confirmed'       = 21

Reading: 16 takes recorded pasted=false and only 3 of them logged "paste not confirmed" (a ⌘V that
went out with no receipt). The other 13 took a branch that posts no keystroke, and the log does not
say which (auto-paste off, no text control focused, a stale take, a wrong target, or Accessibility
untrusted all look the same). "Enable Accessibility" = 0 is consistent with the code path on main:
with Accessibility untrusted, focus::should_auto_paste() returns false BEFORE paste.rs is reached,
so lib.rs passes want_paste=false and the take is a silent "Copied to clipboard (⌘V to paste)" —
paste.rs's own AX-denied message can never be logged for a dictation. That silent no-attempt
branch, plus the 3 receipt-less ⌘Vs that only produced a toast, is the insertion "bridge" this
item fixes. NOT established from the log: how many of the 13 were AX-denied specifically.

## (b) "Bridge error" — the app shell, vitest on unmodified main

desktop/src/appShell.boot.test.ts drives the REAL useAppShell().refreshAll with ONE boot invoke
(list_crash_events) rejecting. On unmodified main (Promise.all):

    MAIN-EVIDENCE status.message="Bridge error: list_crash_events: boom" bootError="list_crash_events: boom" settings=null fullscreen=true

and the same file's verdict on main:

       × boot: Promise.allSettled, one command at a time > one rejected invoke leaves the rest rendered and no Bridge error status 5ms
       ✓ boot: Promise.allSettled, one command at a time > get_settings rejection shows the full-screen retry 2ms
       × boot: Promise.allSettled, one command at a time > a failed list_dictionary is the Dictionary view's own inline error + retry 0ms
    ⎯⎯⎯⎯⎯⎯⎯ Failed Tests 2 ⎯⎯⎯⎯⎯⎯⎯
          Tests  2 failed | 1 passed (3)

i.e. one failed secondary read rejected the whole batch, settings never landed, the status line
became "Bridge error: …" and App.tsx replaced every view with "UI failed to load backend bridge".

## (c) Does CGEventPost(⌘V) need a grant? scripts/qa/post-probe.swift

Built exactly like fn-post.swift (swiftc -O, codesign -s -) into desktop/target/qa/post-probe and
run from the loop's terminal WITHOUT --post:

    post_event_access=true
    ax_trusted=true

post_event_access=true here because macOS attributes the helper to the launching terminal, which
holds Accessibility + PostEvent on this Mac. The "without Accessibility" half was NOT measured:
revoking the terminal's grant is TCC state this loop does not change, and --post (which opens a
TextEdit document and posts ⌘V/⌘S into it) was not run on Wilson's live desktop. What is
established: fn-post.swift:32 and this probe both gate on CGPreflightPostEventAccess(), and
CGEventPost delivers nothing when PostEvent is missing — so the fix never assumes "a keyDown needs
no AX": the bridge is chosen only when perm_probe says PostEvent is Granted, and the Copy card
covers every other case. To finish the measurement by hand: remove the terminal from
Accessibility, run desktop/target/qa/post-probe --post, read delivered=.

## The fix in one line each

- paste.rs: InsertPath { Ax, PasteBridge, CopyCard } chosen by choose_insert_path(&perm_probe::probe_all()).
- focus::should_auto_paste says yes when AX is untrusted (it cannot see the focus), so the path decides.
- CopyCard / no-receipt bridge: "insert-copy-card" -> the pill shows the text with Copy + Repair.
- appShell.ts: Promise.allSettled; per-command bootErrors; "Bridge error" only when every command failed.
