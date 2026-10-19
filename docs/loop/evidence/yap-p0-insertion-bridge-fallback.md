ROOT-CAUSE-PROBE yap-p0-insertion-bridge-fallback

# yap-p0-insertion-bridge-fallback — which "bridge" was broken

Gathered 2026-10-19T03:20Z on macOS 26.6.2 (25G83), branch
loop/yap-p0-insertion-fallback-copy-card off origin/main 3569ce4, BEFORE any code change.

## (a) yap.log since 2026-10-16 (read-only grep)

File: ~/Library/Application Support/WilsonVoice/logs/yap.log, lines whose timestamp is >= 2026-10-16.

CORRECTED in fix round 1 (PR #229 review): every take logs `pasted=` on TWO lines (`latency …` and
`transcript ok …`), so the first draft of this table double-counted. Recount, per line kind, with
exactly this command (run 2026-10-19 from that logs directory):

    for pat in "transcript ok.*pasted=false" "transcript ok.*pasted=true" "latency .*pasted=false" \
               "latency .*pasted=true" "pasted=false" "pasted=true" "paste not confirmed" \
               "paste confirmed" "Enable Accessibility"; do
      n=$(cat yap.log* | awk -v p="$pat" '$1 >= "[2026-10-16" && $0 ~ p' | wc -l | tr -d ' ')
      echo "since 2026-10-16: '$pat' = $n"; done

    since 2026-10-16: 'transcript ok.*pasted=false' = 8
    since 2026-10-16: 'transcript ok.*pasted=true' = 22
    since 2026-10-16: 'latency .*pasted=false' = 8
    since 2026-10-16: 'latency .*pasted=true' = 22
    since 2026-10-16: 'pasted=false' = 16      (8 takes x 2 lines)
    since 2026-10-16: 'pasted=true' = 44       (22 takes x 2 lines)
    since 2026-10-16: 'paste not confirmed' = 3
    since 2026-10-16: 'paste confirmed' = 22
    since 2026-10-16: 'Enable Accessibility' = 0

Reading: 8 TAKES recorded pasted=false (not 16) and 3 of them logged "paste not confirmed" (a ⌘V
that went out with no receipt). The other 5 (not 13) took a branch that posts no keystroke, and the
log does not say which (auto-paste off, no text control focused, a stale take, a wrong target, or
Accessibility untrusted all look the same). "Enable Accessibility" = 0 is consistent with the code
path on main: with Accessibility untrusted, focus::should_auto_paste() returns false BEFORE
paste.rs is reached, so lib.rs passes want_paste=false and the take is a silent "Copied to
clipboard (⌘V to paste)". On main a ⌘V is only posted with AX trusted, so the 3 receipt-less ⌘Vs
happened WITH Accessibility granted — which is why, after fix round 1, a no-receipt card never
offers Repair (paste.rs repair_service_for). NOT established from the log: how many of the 5 were
AX-denied specifically.

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
- CopyCard / no-receipt bridge: "insert-copy-card" -> the pill shows the text with Copy (+ Repair ONLY on the
  no-permission card; fix round 1).
- Fix round 1: the YV21 guard compares NSWorkspace bundle ids (no AX needed) and a dictation take with an
  unknown target posts nothing; an accepted-but-unverified AX set never falls through to ⌘V; dismissing the
  card never hides the pill of a take in flight, and a new take clears an old card.
- appShell.ts: Promise.allSettled; per-command bootErrors; "Bridge error" only when every command failed.
