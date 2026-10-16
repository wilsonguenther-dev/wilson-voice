// yap-flakes-2026-10-16 — MAIN GOES GREEN. Written 2026-10-16 by the yap-prep seat.
// Sources: RESUME §11.7 ("fix it, assert payload elapsed_seconds, it has cost three investigations"),
// 10-06 context Fact 11 + open item 16. Main CI (workflow "CI", job "Tauri build (Rust + frontend)",
// step "cargo test") failed on the last three main runs: 36256562076 (63452bb), 36270319107
// (90d7861), 36343744919 (6f40697). Their failed-step logs were no longer retrievable on 2026-10-16
// (gh run view --log-failed returned nothing), so WHICH test failed on each run is UNVERIFIED; the
// two named tests are the recorded suspects. FIX, no panel (unambiguous intent).
// Runs before every other 2026-10-16 item: every later gate inherits a red main otherwise.
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap-flakes-2026-10-16', prompt: 'yap-flakes', branch: 'loop/yap-flakes-2026-10-16-main-green', gated: null,
  title: 'Main goes green: per_chunk_deadline_not_per_take and meeting_manual_start_stop stop asserting on the scheduler',
  preflight: `
    test -f docs/loop/FLAKES-2026-10-16.md
    test -f scripts/flake-soak.sh
    grep -q 'per_chunk_deadline_not_per_take' docs/loop/FLAKES-2026-10-16.md
    grep -q 'meeting_manual_start_stop' docs/loop/FLAKES-2026-10-16.md
  `,
  spec: `
    Panel: not required — FIX (unambiguous intent: main must be green before the loop lands anything)
    DEPENDS: none

    EVIDENCE
      - desktop/src-tauri/tests/dictation_chunked.rs:305 per_chunk_deadline_not_per_take: a 300 ms
        per-chunk budget against a fake decode that sleeps 200 ms per window, twelve windows. Each
        window has 100 ms of wall-clock headroom. A loaded box (CI, or two loop lanes compiling)
        that delays one sleep or one thread wake by more than 100 ms trips the per-chunk deadline,
        chunks_failed becomes 1, and the test fails while the code under test is right. Recorded
        red on main at 90d7861 (Obsidian Projects/Yap.md, 2026-09-27).
      - desktop/src-tauri/tests/meeting_manual_start_stop.rs: the YV111 tick-count window was
        already replaced by support::wait_until on the ticker's own output (around line 145), but
        RESUME §11.7 still names this test as the wall-clock flake, and asks for the assertion to
        sit on the PAYLOAD: every recording status's elapsed_seconds checked against the sink's
        receipt Instant (struct Emit, around line 39), not on how many emits a window caught.
      - Three consecutive red main runs on step "cargo test" (ids in the header).

    DO
      1. REPRODUCE FIRST, before any edit. Write scripts/flake-soak.sh <test-target> <filter> <runs>:
         it starts one busy loop per CPU (sysctl -n hw.ncpu; yes > /dev/null), records their PIDs,
         traps EXIT to kill exactly those PIDs, runs "cargo test --features custom-protocol --test
         <test-target> <filter>" <runs> times from desktop/src-tauri, reads every exit code bare,
         prints "<passes>/<runs>", and exits non-zero on any failure. Run it 30x for each test on
         unmodified main and write the counts to docs/loop/FLAKES-2026-10-16.md. If a test passes
         30/30 under load, write that down and harden only what the evidence supports.
      2. per_chunk_deadline_not_per_take: keep what it proves (the deadline is spent per WINDOW, not
         per take: the take's total decode time exceeds the budget while no single window does) and
         remove the scheduler dependence. Preferred: drive the deadline from an injectable clock
         (a test-only seam, no behaviour change) so elapsed time is simulated. Fallback: about 10x
         per-window headroom with more windows (for example 24 windows x 50 ms against a 1 s budget:
         1.2 s total exceeds the budget, each window keeps 950 ms of headroom).
      3. meeting_manual_start_stop: assert on the payload. For every recording status the sink
         received, elapsed_seconds never decreases, and it stays within 1 s plus one tick of
         (receipt Instant minus the start Instant). support::wait_until stays the only wait,
         TICK_DEADLINE stays the death detector, and no sleep-then-count shape exists in the file.
      4. Sweep desktop/src-tauri/tests/ for the same shape (a sleep followed by a count, or a
         wall-clock upper bound under 1 s) and list every hit in the doc. Fix only the two named
         tests here; file the rest as new numbered entries in docs/loop/DEFERRED.md.
      5. Never add #[ignore], never add a retry loop, never widen a bound past what the doc's
         numbers justify. The PR body carries the before and after soak counts.
  `,
  acceptance: `
    test -f docs/loop/FLAKES-2026-10-16.md
    test -x scripts/flake-soak.sh
    grep -q 'elapsed_seconds' desktop/src-tauri/tests/meeting_manual_start_stop.rs
    ! grep -qF '#[ignore]' desktop/src-tauri/tests/dictation_chunked.rs
    ! grep -qF '#[ignore]' desktop/src-tauri/tests/meeting_manual_start_stop.rs
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    bash scripts/flake-soak.sh dictation_chunked per_chunk_deadline_not_per_take 30
    bash scripts/flake-soak.sh meeting_manual_start_stop '' 30
  `,
})
