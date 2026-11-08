# yap-1108-timing-idle-watcher-clock — audit rows 1-2 status

Written 2026-11-08T21:56:36Z by loop-three coder B4 (base ce41786f, origin/loop3/integration-2).

Status line for docs/loop/evidence/timing-tests-audit.md rows 1-2: FIXED IN CODE, NOT YET RUN.
The audit file is not on origin/main or on this base (it lives only on the held branch
loop3/yap-1107-timing-tests-deterministic), so this line is kept here instead of taking the
branch's copy; append it to main's copy once that file lands.

## What changed (src/transcription.rs)

- Production seam (test-only in use, app behaviour identical): `TranscriptionManager::set_idle_clock`
  installs a `ManualLeaseClock` (the #374 lease-clock type) as the idle clock. `touch()`, the
  watcher's idle comparison in `unload_if_idle()` and `status().idle_seconds` read `idle_now_ms()`,
  which is the wall clock (`now_ms()`, unchanged) unless a test installed a clock. The app never calls
  the setter, so the app's idle unload is exactly what it was.
- `idle_watcher_ticks()`: a counter the watcher bumps after each completed evaluation. Tests wait for
  two ticks past the value seen after advancing the clock (the first may be an evaluation that read the
  clock before the advance; the watcher is one thread, so the second began after it).

## Why the new assertions are equivalent or stronger

- `state_transitions_unloaded_to_loaded_to_idle_unloaded`: the old test polled for up to 5 s of wall
  time for the 80 ms watcher to unload. Now: advance 79 ms, watcher evaluates, engine still loaded
  (new: proves the timeout is not undershot); advance 1 ms, watcher evaluates, engine unloaded (the
  original property, at the exact boundary); after the reload a fresh tick with no time passing keeps
  it loaded (new: the #370 stale-stamp property, deterministically). Every original assertion kept.
- `empty_audio_is_a_no_op_and_use_keeps_the_engine_warm`: the old test needed each `sleep(50ms)` +
  use to beat a 200 ms wall timer. Now each step advances the idle clock 50 ms and waits for a watcher
  evaluation before the use (300 ms of idle-clock time against 200 ms), asserting loaded at each step.
  New control at the end: a full 200 ms with no use does unload, so the uses are what kept it warm.
  Every original assertion kept. No `#[ignore]` anywhere.

## Integrator (cannot be run by a coder seat)

From the repo root:

    bash scripts/stress-tests.sh --runs 20 --log-dir docs/loop/evidence/stress-harness lib transcription::tests::state_transitions_unloaded_to_loaded_to_idle_unloaded
    bash scripts/stress-tests.sh --runs 20 --log-dir docs/loop/evidence/stress-harness lib transcription::tests::empty_audio_is_a_no_op_and_use_keeps_the_engine_warm
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib transcription::tests)
