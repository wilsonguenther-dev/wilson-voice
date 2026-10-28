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

// yap-flake-timing-tests-under-load — Written 2026-10-27 by a spec-writer seat. Audit 2026-10-27: false red in
// fix-land on #259 at 1-minute load ~39 (ratio ~21x against a 1.6x ceiling); the identical head passed at load ~6.
// STANDARD class: every file touched (desktop/src-tauri/tests/*.rs, optionally scripts/ none) misses SECURITY_PATHS in
// scripts/loop/review-gate.mjs:113-118 (checked: no tests/ path matches db, migration, sidecar, updater/sign/network/http
// keywords). CAUTION: tests/diarize_sidecar_*.rs and tests/matrix_row7_diarize_sidecar_wedge.rs match /sidecar/i, so if
// the PR edits ANY of those files it becomes SECURITY class (two Opus seats); step 3 therefore defaults those to
// "left alone" unless the sweep proves one is a ratio assert. Say so in the PR body.
ITEMS.push({
  id: 'yap-flake-timing-tests-under-load', prompt: 'yap-flakes', branch: 'loop/yap-flake-timing-tests-under-load', gated: null, builder: 'sonnet',
  title: 'Timing tests stop failing on a loaded Mac: CPU-time ratio plus a load guard that never silently passes',
  preflight: `
    test -f desktop/src-tauri/tests/long_take_budget.rs
    grep -q 'fn chunk_decode_wall_per_audio_second' desktop/src-tauri/tests/long_take_budget.rs
    grep -q '^libc = ' desktop/src-tauri/Cargo.toml
  `,
  notes: `Audit 2026-10-27: false red in fix-land on #259 at load 39; passed at load 6.`,
  spec: `
    Panel: not required — FIX (unambiguous intent: a gate must not go red because other seats are compiling)
    DEPENDS: none
    CLASS: standard (see header comment for the exempt-file rule in step 3)

    EVIDENCE
      - desktop/src-tauri/tests/long_take_budget.rs:381 chunk_decode_wall_per_audio_second times a
        single-window decode (t0, line 420) and a 15 s-window chunked decode (t1, line 428) with
        Instant::now()/elapsed() and asserts the wall ratio against
        CHUNK_DECODE_WALL_PER_AUDIO_SECOND_RATIO_CEILING = 1.60 (const at line 98; assert at line 450).
        With four to six cargo seats running at once, the two arms get different scheduler
        slices; measured ~21x at load ~39, passed at load ~6 on the identical head.
      - The module doc (lines 26-28) says the ratio is "deliberately not a wall-clock number",
        but the implementation measures wall clock; the intent is a CPU-cost ratio.
      - libc = "0.2" is already a direct dependency (desktop/src-tauri/Cargo.toml:45), so NO new
        dependency is needed or allowed. The file already holds heavy() (line 173), a mutex that
        serialises the heavy tests within the binary, so process CPU time is not polluted by
        another heavy test.

    DO
      1. CPU-time ratio. In long_take_budget.rs add a private fn process_cpu_micros() -> Option<u128>
         that calls libc::getrusage(libc::RUSAGE_SELF, &mut ru) (unsafe, zeroed libc::rusage), returns
         None on a non-zero return, else (ru_utime + ru_stime) in microseconds. Bracket the single arm
         and the chunked arm with it exactly where t0/t1 bracket them now, while _heavy is held.
         Assert the SAME 1.60 ceiling against cpu_chunked / cpu_single (max(1) on the divisor) when both
         readings are Some and cpu_single is at least 50_000 us (below that the tick granularity makes
         the ratio noise: print "cpu ratio unusable" and fall to step 2's path). Keep the wall-clock
         figures in the println as information only. Update the doc comment above the test and the
         const's doc comment to say the ratio is CPU time; do not change the 1.60 value.
         If getrusage proves unusable (None), choose step 2 only and say so in the PR body.
      2. Load guard. Add pure fn should_skip_timing(load: f64, cores: usize) -> bool { load > 2.0 * cores as f64 }
         and fn load_average_1m() -> Option<f64> using libc::getloadavg(&mut [f64; 3], 3) (returns < 1 on
         failure -> None), and cores from std::thread::available_parallelism(). Wherever the test would
         assert a WALL-clock ratio (the fallback path of step 1, or any assertion that stays wall-clock),
         run the decode and the functional asserts first (windows > 1 at line 444 stays unconditional),
         then if should_skip_timing(load, cores): eprintln!("skipped: load {load:.1} > threshold {thr:.1}")
         and return WITHOUT asserting the ratio. Never return before the decode runs. Never silently pass:
         the eprintln is mandatory, and if the load is unreadable (None) the assertion runs.
         Put should_skip_timing and load_average_1m in desktop/src-tauri/tests/support/mod.rs so step 3
         files share one copy (mod support already exists in the tree); keep long_take_budget.rs importing it.
      3. Sweep. Run: grep -rn "elapsed\\|Instant::now" desktop/src-tauri/src desktop/src-tauri/tests
         In the PR body list EVERY hit group by file with a disposition. Guard (same skip, same eprintln)
         only an assertion that compares a measured elapsed Duration to a RATIO or to an upper bound the
         scheduler can blow (examples to examine first: tests/matrix_row5_journal_backpressure.rs:131 and
         :173, tests/meeting_capture_rt_safety.rs:194, tests/matrix_new_asr_chunk_timeout.rs:250,
         tests/long_take_budget.rs:314-319 spill-overhead ceiling). Leave alone, with the reason written
         in the PR body: src/ hits (production timing, not assertions), deadline-polling loops
         (while Instant::now() < deadline), already-fixed wait_until shapes (meeting_manual_start_stop.rs),
         and any assertion whose point is the bound itself (a hang detector such as
         matrix_new_asr_chunk_timeout.rs HANG proves a deadline fires; guarding it would hide the bug it
         exists to catch, so exempt it and say so). Any test file whose NAME contains the word for
         an out-of-process helper (the diarize pool, handshake and wedge tests) is EXEMPT: editing one
         would silently raise this PR to security class, so list those hits as "left alone: class
         boundary" and file them in docs/loop/DEFERRED.md instead. Do NOT touch dictation_chunked.rs or
         meeting_manual_start_stop.rs (the sibling item yap-flakes-2026-10-16 owns them).
         Do not add #[ignore], a retry loop, or widen any bound.
      4. Tests. In support/mod.rs add #[cfg(test)] unit tests for should_skip_timing: (39.0, 12) true,
         (6.0, 12) false, (24.0, 12) false (equal is not over), (24.1, 12) true, (0.0, 1) false. If
         support/mod.rs is included by many test binaries, use #[allow(dead_code)] on the helpers so no
         binary emits a warning; confirm cargo test emits no new warnings.
      5. PR body: the step 3 table (file:line, assertion, guarded or left alone, why) plus the exit codes below.
  `,
  acceptance: `
    grep -q 'fn should_skip_timing' desktop/src-tauri/tests/support/mod.rs
    grep -q 'getrusage' desktop/src-tauri/tests/long_take_budget.rs
    grep -q 'skipped: load' desktop/src-tauri/tests/long_take_budget.rs
    ! grep -qF '#[ignore]' desktop/src-tauri/tests/long_take_budget.rs
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    cd desktop/src-tauri && cargo test --features custom-protocol --test long_take_budget ; test $? -eq 0
    cd desktop/src-tauri && cargo test --features custom-protocol ; test $? -eq 0
    grep -rn "elapsed()" desktop/src-tauri/tests | grep -E "assert|<|>" | cat
  `,
})

// yap-flake-sleep-test-worker-race — Written 2026-10-27 by a spec-writer seat. Audit 2026-10-27: false red in the land
// gate on two unrelated heads (1 of 5 runs on one head, twice on another that touches no meeting code):
// meeting_survives_sleep::will_sleep_mid_recording_finalizes_partial_then_acknowledges asserts left "transcribing",
// right "partial". A RACE IN THE TEST: the real pipeline worker takes the row and moves it to transcribing before the
// test reads it. No existing gate precedes that state write (asr, demand and quit in PipelineDeps are all consulted
// AFTER run_meeting_job's set_meeting_state(Transcribing), meeting_pipeline.rs:~499 at e0988a8), so option (a) "reuse an
// existing gate" is NOT available; this item takes option (b): a hook field mirroring the existing after_transcript.
// STANDARD class: securityClassOf (review-gate.mjs:138-145) tests SECURITY_PATHS (:113-118) against every file;
// meeting_pipeline.rs, lib.rs, tests/meeting_survives_sleep.rs and tests/meeting_pipeline_wired.rs match none of
// db|migration|polish_protocol|sidecar|updater|sign|notariz|keychain|licens|entitle|permission|accessibility|
// microphone|network|http|upload|telemetry nor scripts/loop/. So builder: 'sonnet'. If the builder ends up touching any
// path that matches (say so in the PR body), the PR becomes security class and needs two Opus seats.
// ANCHOR CAVEAT: the test file was written by PR #260 (commit e0988a8, yap24-NT10) and is not in the working tree this
// spec was written from; line numbers below are from e0988a8 and the builder re-verifies them on current main.
ITEMS.push({
  id: 'yap-flake-sleep-test-worker-race', prompt: 'yap-flakes', branch: 'loop/yap-flake-sleep-test-worker-race', gated: null, builder: 'sonnet',
  title: 'The sleep-mid-recording test stops racing the pipeline worker: a held worker, then both states asserted in order',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/tests/meeting_survives_sleep.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 260 (NT10, meeting survives sleep) has not landed; land it, then build this item"; exit 0; fi; grep -q 'fn will_sleep_mid_recording_finalizes_partial_then_acknowledges' desktop/src-tauri/tests/meeting_survives_sleep.rs && grep -q 'pub after_transcript' desktop/src-tauri/src/meeting_pipeline.rs )
  `,
  notes: `Audit 2026-10-27: false red in the land gate on two unrelated heads; worker advances the row before the read.`,
  spec: `
    Panel: not required — FIX (unambiguous intent: a gate must not go red on a race in a test)
    DEPENDS: none
    CLASS: standard (see header comment; if you touch a SECURITY_PATHS file, say so in the PR body)

    EVIDENCE
      - desktop/src-tauri/tests/meeting_survives_sleep.rs (as of e0988a8): the test starts with the REAL pipeline
        worker installed (spawn line 200, install line 202). controller.start (208) then the WillSleep injection
        (218) stops the meeting, which enqueues the id on that worker. run_meeting_job
        (src/meeting_pipeline.rs:454) reads the row, then calls set_meeting_state(id, Transcribing, ..) (about line
        499) and only afterwards builds the decoder via (deps.asr)(). The test reads the row in three places that
        expect "partial" BEFORE the worker is drained: inside the ack closure (222-227, logged "ack recording=false
        state=partial", compared at 231-238), and at 241-249 (assert at 242: state == "partial"). wait_idle (254) is
        the first synchronisation. Between enqueue and wait_idle the worker may already have flipped the row, hence
        left: "transcribing" right: "partial". It is a test race, not a product bug: the product is right to move
        the row to transcribing.
      - PipelineDeps (src/meeting_pipeline.rs:129-139) has hooks asr, demand, quit, sink, after_transcript (all
        invoked at or after the state write; after_transcript at ~593). There is NO hook that runs before the
        state write, so there is no existing gate to hold. The struct is built literally in exactly three places:
        src/lib.rs (about line 7190), tests/meeting_pipeline_wired.rs (deps(), line 121 sets after_transcript: None)
        and tests/meeting_survives_sleep.rs (deps(), line 116).

    DO
      1. The gate (option b, the minimum). In src/meeting_pipeline.rs add
           pub type BeforeJob = Arc<dyn Fn(&str) + Send + Sync>;
         and a field pub before_job: Option<BeforeJob> on PipelineDeps, documented as "called on the worker
         thread with the meeting id, before the job reads or advances the row; None in production". In
         run_meeting_job's caller, the worker loop in spawn (the for id in rx body, before catch_unwind(...
         run_meeting_job ...)), call it if Some. Do NOT put it inside run_meeting_job (pipeline tests call that
         function directly). Set before_job: None in lib.rs (the PipelineDeps literal near line 7190) and in
         tests/meeting_pipeline_wired.rs deps(). No cfg gate: the integration tests compile the lib without
         cfg(test), so a cfg(test) hook would not be visible to them. No behaviour change when None. No new
         dependency.
      2. In tests/meeting_survives_sleep.rs add a small test-local gate (about 20 lines): a struct Gate { open:
         Mutex<bool>, cv: Condvar } with hold() (sets closed), release() (sets open, notify_all) and wait()
         (cv.wait_while with a 30 s wait_timeout_while HANG guard that panics naming what never happened; the
         timeout is a hang guard, never a measurement). deps() takes the Gate and sets before_job to call
         gate.wait(). In will_sleep_mid_recording_finalizes_partial_then_acknowledges: build the gate CLOSED
         before spawn, keep an Arc for the test. Never add a sleep of any kind.
      3. Assert both states, in order. While the gate is closed run all the existing partial assertions
         unchanged (ack-time log 231-238, row 241-249 including line 242 state == partial, error note, WAV
         paths, diagnostics). Then gate.release(); pipeline.wait_idle(30 s) (existing line 254); then assert the
         final state: segments on both tracks (existing 259-265) and row.state == "partial" with the sleep
         sentence (existing 266-267). Add one explicit comment line: "partial observed while the worker was
         held, still partial after it drained" so the sequence, not a snapshot, is what the test proves. The
         other two states are NOT interchangeable: the test must not accept "transcribing" anywhere.
         If the held assertion at 222-227 is inside the ack closure on the test thread, that is fine: the
         worker cannot run past before_job while the gate is closed.
      4. Sweep the same pattern (read after a spawn or enqueue with no gate). Run
           grep -rn "meeting_pipeline::spawn\\|pipeline.enqueue\\|\\.enqueue(" desktop/src-tauri/tests
         and read every hit. Known set at e0988a8: meeting_survives_sleep.rs (this test, FIXED), the other three
         tests in that same file (no pipeline spawned: the idle test builds none, the message-table test and the
         power.rs source test never touch a row; say so), and tests/meeting_pipeline_wired.rs:177 (spawns and
         installs a pipeline; read the test at 177-200 onward and decide: if it reads a meeting row between enqueue
         and wait_idle it is racy and gets the same Gate; if it only reads after wait_idle it is not racy, say
         why). Also grep tests/meeting_dictation_preempts_transcription.rs and tests/meeting_capture_rt_safety.rs
         for a worker (run_meeting_job or spawn) and give each a disposition. Put the table in the PR body:
         file:line, what is read, FIXED or "not racy because X". Do not touch dictation_chunked.rs or
         meeting_manual_start_stop.rs (yap-flakes-2026-10-16 owns them). Do not add #[ignore], a retry loop,
         or widen any bound.
      5. Tests. A unit-level proof of the hook in meeting_pipeline.rs tests is NOT required; the integration
         test is the proof. Confirm cargo test emits no new warnings (before_job is read in spawn, so no
         dead_code). Put the five exit codes and the full cargotest exit in the PR body.
  `,
  acceptance: `
    grep -q 'pub before_job' desktop/src-tauri/src/meeting_pipeline.rs
    grep -q 'before_job' desktop/src-tauri/tests/meeting_survives_sleep.rs
    ! git diff origin/main -- desktop/src-tauri/tests/meeting_survives_sleep.rs desktop/src-tauri/src/meeting_pipeline.rs | grep -E '^\\+.*(thread::sleep|[^_a-z]sleep\\()'
    ! grep -qF '#[ignore]' desktop/src-tauri/tests/meeting_survives_sleep.rs
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    cd desktop/src-tauri && for i in 1 2 3 4 5; do cargo test --features custom-protocol --test meeting_survives_sleep; e=$?; echo "RUN$i EXIT=$e"; test $e -eq 0 || exit 1; done
    cd desktop/src-tauri && cargo test --features custom-protocol ; test $? -eq 0
  `,
})
