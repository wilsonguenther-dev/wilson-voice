# yap-insert-bridge-2026-10-26 — evidence

ROOT-CAUSE-PROBE yap-insert-bridge-2026-10-26

## Probe 1 — cargo test on UNMODIFIED main (202fea8): choose_insert_path returns Ax for ax=Granted post=Granted

Run in lane-b, detached at origin/main 202fea8, before any edit:

    (cd desktop/src-tauri && CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=…/target-b \
       cargo test --features custom-protocol --test insertion_matrix ax_trusted_uses_ax)
    probe_exit=0
    test ax_trusted_uses_ax ... ok
    test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out; finished in 0.00s

`ax_trusted_uses_ax` asserts `choose_insert_path(&probe(true, true)) == InsertPath::Ax` — main
routes every take with Accessibility granted to AX first, which is the regression (#229, 6c3f991).

## Probe 2 — read-only yap.log counts (grep -c, no writes)

~/Library/Application Support/WilsonVoice/logs/, 2026-10-26:

    yap.log:   path=Ax:83 unverified-card:32 refused-bridge:45 pasted=false:96 pasted=true:560 paste-confirmed:274
    yap.log.1: path=Ax:0  unverified-card:0  refused-bridge:0  pasted=false:20 pasted=true:1142 paste-confirmed:571
    yap.log.2: path=Ax:0  unverified-card:0  refused-bridge:0  pasted=false:32 pasted=true:1432 paste-confirmed:716
    yap.log.3: path=Ax:0  unverified-card:0  refused-bridge:0  pasted=false:33 pasted=true:1173 paste-confirmed:326

    grep -h 'insert path=' yap.log* | uniq -c:
      83 insert path=Ax (ax=Granted post=Granted)

Patterns: `insert path=Ax (ax=Granted post=Granted)`, `AX insert unverified` (→ Copy card, no ⌘V),
`AX insert refused` (→ paste bridge), `pasted=false`, `pasted=true`, `paste confirmed`.
Every insert line since #229 took the AX path with BOTH grants live (permissions are fine), and
32 of 83 ended "unverified → Copy card" — the take Wilson copy/pastes by hand. The older rotated
logs (pre-#229) never logged an AX path. Per-app split (Claude 41/41 → 29/29, Chrome, Terminal
23/23 refused→bridge) is the diag seat's 2026-10-26T13:45Z reading, not re-derived here.

## Fix proof — test RED on the #229 order, GREEN on the fix

With choose_insert_path temporarily put back to AX-first:

    test post_event_granted_uses_paste_bridge_regardless_of_ax ... FAILED
    assertion `left == right` failed: ax=Granted post=Granted is the 0.9.3-dev regression case
      left: Ax
     right: PasteBridge
    red_run=101

With the fix:

    test post_event_granted_uses_paste_bridge_regardless_of_ax ... ok
    green_run=0
