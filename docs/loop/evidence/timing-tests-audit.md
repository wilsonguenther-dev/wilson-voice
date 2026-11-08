# Timing-dependent Rust tests: audit (yap-1107-timing-tests-deterministic)

Written 2026-11-08T05:04:46Z by loop-three coder B1, against base e18a12d6 (origin/main).
Round 2 added 2026-11-08T05:12:53Z by coder B1 (same branch): injected quit clock, src/ `mod tests` audit, production deadline review.
Status: PARTIAL. The coder seat may not compile or run anything (no-compile rule), so the
saturated before/after counts and the 50-run proof are integrator steps (bottom of this file).

## Method

`rg -n "thread::sleep|tokio::time::sleep|recv_timeout|park_timeout|\.elapsed\(\)|wait_timeout"`
over `desktop/src-tauri/tests` (256 hits in 60 files), each read in context and sorted into the
classes the item names: (S) a sleep used to order events, (D) a short deadline a healthy path
must beat, (X) a deadline shared between independent threads, (N) a negative check whose grace
sleep only affects how long a stray event has to show up (passes, never flakes), (B) benign:
no assertion depends on it (a stub's simulated work, a hang guard, a perf print).
Fix rule: wait on the event the test means (a counter, a flag the code raises, a recorded
instant), never a longer sleep, never a deleted / ignored / loosened assertion.

## Fixed in this branch (class, what it assumed, the fix)

| file:line (after fix) | was | class | assumed | fix |
|---|---|---|---|---|
| tests/matrix_new_quit_mid_processing.rs:509 | spin on "thread started" + `sleep(50ms)` | S | the chunk thread is inside the engine 50 ms after it starts | `BlockingEngine.entered` flag raised inside `transcribe`; test waits on it (`support::wait_until`) before the drain |
| tests/http_client_no_redirect.rs:122 | `sleep(200ms)` before `b_hits == 1` | S | B's accept thread runs within 200 ms of a direct connect | `wait_until(b_hits >= 1)` then the exact `== 1` assert |
| tests/http_client_no_redirect.rs:95, :147, control test | `sleep(400/400/200ms)` before counter reads | N/S | counters need time | removed: the response the client read was written after the server's `fetch_add`, so the counters are final when the call returns (comment states the ordering) |
| tests/summary_download_lock.rs:147 | `sleep(60ms)` then assert the job holds the slot | S | the job thread took the slot within 60 ms | `wait_until(download_in_progress(&dest).is_some())` |
| tests/summary_download_lock.rs:214, :246 (`cancel_after`, `since_raise` at :47) | latency = `t0.elapsed() - 300ms` | X | the cancel-setter thread wakes exactly at 300 ms; its lateness was charged to the download | the setter stamps the instant it raises the flag; latency is measured from that stamp. The 500 ms bound is unchanged |
| tests/system_audio_verdict_silence_is_not_denial.rs:333 | `sleep(2_300ms)` then `samples >= 1` | S | the pump thread gets a turn after its ~1 s sample interval within 2.3 s | poll `tap.output_evidence().samples` until >= 1 (60 s hang guard) |
| tests/cancel_long_take.rs:224 | `sleep(CHUNK_DECODE * 4)` then assert a decode is in flight | S | the decoder thread started and crossed boundaries within 160 ms | `wait_until(chunks_done >= 2)` |
| tests/cancel_long_take.rs:451 | `sleep(90ms)` "a couple of windows" | S | two 30 ms windows decoded within 90 ms | `wait_until(windows_decoded >= 2)` |
| tests/capture_journal_recovery.rs:122, tests/matrix_row6_orphan_recovery.rs:68 and :176, tests/meeting_retention_keeps_untranscribed_audio.rs:171, tests/two_track_journal_round_trip.rs:357 | `sleep(300-500ms)` "for the 250 ms flush timer" before `abandon()` | B (slow) | the timer must fire before the crash | removed: `abandon()` joins the writer (`MeetingJournal::hang_up`, meeting.rs), which drains the queue, writes the last index records and flushes every spill before exiting. 2.1 s of dead wall clock gone; outcome identical |

### Round 2 (2026-11-08T05:12:53Z)

| file:line (after fix) | was | class | assumed | fix |
|---|---|---|---|---|
| src/lifecycle.rs:409 `QuitClock`, :471 `FakeQuitClock`, :547 `run_bounded_with` | (production) `run_bounded` read `Instant` and a real `recv_timeout` directly | D | n/a | injected clock. `run_bounded` keeps its signature and runs on `SystemQuitClock` (identical behaviour). `FakeQuitClock`: a step declares its cost with `charge(d)` or hangs with `wedge()`; cost > wait is `TimedOut` and fake time advances by exactly the wait, else `Done` and time advances by the cost. Outcome is arithmetic, not scheduling |
| tests/lifecycle.rs:127 (`took` at :148) | finalize `sleep(QUIT_BUDGET + 300ms)`, then `took < cost + 1 s` on the wall | D | the runner thread and every other step finish within 1 s of slack | finalize `clock.charge(cost)` on a `FakeQuitClock`; the same two bounds are asserted on `report.total` |
| tests/lifecycle.rs:174 (`took` at :192) | wedged exempt step `sleep(10 s)`, `400 ms <= took < 1.2 s` wall | D | `recv_timeout(400ms)` wakes within 800 ms | step calls `wedge()`; same bounds on fake `report.total` |
| tests/lifecycle.rs:278 (:313) | wedged step `sleep(10 s)`, whole run `< 1 s` wall | D | three threaded steps + a panic finish within 1 s | `wedge()`; same `< 1 s` on fake `report.total` |
| tests/lifecycle.rs:334 (:347) | every step `sleep(30 s)`, run `< budget + 200 ms` wall | D | each `recv_timeout` overshoots by under 200 ms in total | `wedge()`; same bound on fake `report.total` |
| src/meeting.rs:4403 `an_abandoned_meeting_is_recovered_as_partial` (:4408) | `sleep(400ms)` "let the writer land the bytes" before `abandon()` | B (slow) | the writer ran within 400 ms | removed: `abandon()` -> `hang_up` (meeting.rs:1498) joins the writer, which drains the queue before exiting |
| src/meeting.rs:4432 `the_launch_scan_never_finalizes_a_journal_that_is_still_live` (:4443) | `sleep(400ms)` then snapshot the dir listing and compare it after the scan | S (real race) | the writer thread created spills, marker and index files within 400 ms; a late create between the two snapshots failed `listing == before`, a late marker failed `marker_exists` | wait until every index file exists (the writer creates spills, then marker, then indexes before its first recv: meeting.rs `meeting_writer_loop`), 60 s hang guard |
| src/record.rs:7254 `capture_ends_within_budget_of_the_stop_edge_while_context_reads_stall` | spawned capture thread timed against `STOP_EDGE_BUDGET` (300 ms) while reads slept 1.2 s | D (high: 300 ms of thread-start latency) | the capture thread is scheduled within 300 ms | explicit ordering: the context read blocks until the capture end has run (`mpsc` gate, 60 s hang guard). The pre-fix order (reads, then Disarm) can never satisfy it, so it fails deterministically instead of by timing |
| src/record.rs:5404 `stop_signal_wakes_waiters_immediately_on_release` | `wait_stopped(5 s)` must return in `< 500 ms` | D | the waiter wakes within 500 ms of a 10 ms-delayed stop | waiter parks on a 3,600 s tick on its own thread and announces it; test raises stop; `recv_timeout(60 s) == Ok(true)`. Waiting the tick out fails; waking on the signal passes |
| src/record.rs:5685, :5756 (helper :5723 `returns_while_nobody_drains`) | 6 / 64 offers into a full queue `< 500 ms` | D | the test thread is not descheduled 500 ms | offers run on a helper thread while nobody drains the receiver; a parking offer never returns and fails the 60 s hang guard. Drop counts and queue contents asserted exactly as before |
| tests/permission_transitions.rs:208 | `recv_timeout(2 s)` for the revoke emit | D | the watcher thread reads and emits within 2 s | positive wait with a 60 s hang guard (returns on the event). The two 400 ms negative checks (:192, :215) stay: under load they can only pass vacuously, never fail (class N) |

Residual note, documented not hidden: the counter waits in cancel_long_take and
summary_download_lock still need the engine / stub to be slower than the test thread's poll
(20 x 40 ms, 12 x 30 ms, 24 x 15 ms of paced work). A test thread starved for that whole span
would make the decode finish first. A one-shot gate inside the stub engine would close it
entirely; it was not done in this seat.

## Open: found, not converted in this seat (needs an injected clock in production code)

| file:line | class | what it assumes | proposed fix |
|---|---|---|---|
| tests/lifecycle.rs:209 `a_real_two_track_meeting_finalize_completes_inside_the_quit` | D (intentional) | a REAL 10-minute two-track finalize fits `MEETING_FINALIZE_CEILING` on the real clock | not converted on purpose: it measures real I/O cost against the production ceiling, a fake clock would delete its point. Residual risk under saturation; candidate for a release-build perf lane |
| tests/lifecycle.rs:468 | D (lower bound only) | `elapsed >= 300ms` after a 300 ms handoff timeout | lower bounds cannot flake under load; no change |
| tests/meeting_dictation_preempts_transcription.rs:171-185, :394 | D | a preempting dictation completes under `DICTATION_BUDGET` while a 400 ms chunk is in flight | count "chunk boundaries waited" in the scheduler instead of elapsed wall time |
| tests/dictation_chunked.rs:324-339 | D | per-window `recv_timeout` budget vs a 50 ms stub delay | inject the per-window budget clock into the chunk worker |
| tests/diarize_sidecar_pool.rs:250 | D | a sticky-failed pool answers in under 200 ms | assert "no launch, no wait entered" via a counter on the pool instead of elapsed |
| tests/diarize_sidecar_pool.rs:279, :308; tests/matrix_row7_diarize_sidecar_wedge.rs:82 | D (lower bound + 10x upper) | a real child process and a real readiness deadline | low risk (10x headroom); a fake clock in `DiarizePool` would make it exact |
| tests/matrix_row5_journal_backpressure.rs:97, :131, :173 | D | an overflowing journal append returns under 500 ms / 2 s | count blocking waits in the journal instead of timing them |
| tests/summary_round3.rs:250; tests/summary_download_lock.rs:195 | D | no retry after cancel (< 2 s); backoff total < 6 s | expose the backoff schedule (attempt count) and assert it |
| tests/polish_long_form.rs:142, :430, :654-672; tests/transforms.rs:71 | S/D | a stub sleeping past its deadline; a 25 s wall budget | stub should block on a gate the deadline path opens |
| tests/cloud_route_mock.rs:119-136, :251 | D | 400 ms TTFT against 20 ms / 1200 ms stub pieces | 3x-30x margins, low risk; fake TTFT clock in `cloud_route` |
| tests/meeting_manual_start_stop.rs:450, :540-573; tests/meeting_diagnostics_row.rs:26 | S/D | ticker thread timing (already reshaped once, YV111) | review again with a fake ticker |
| tests/two_track_journal_round_trip.rs:286-291, tests/capture_journal_recovery.rs:204-208 | S | the writer resumes within 20 ms of unpause | wait on a writer "resumed" signal from `pause_handle` |
| tests/meeting_retention_keeps_untranscribed_audio.rs:276; tests/matrix_row4_disk_preflight.rs:242; tests/meeting_no_model_resident.rs:80; tests/meeting_track_b_wiring.rs:118; tests/dictation_pinned_model.rs:106-108 | N/B | "let periodic work run"; the asserted outcome does not depend on how many ticks happened | no flake; optional speed-up |
| tests/cloud_route_mock.rs:452 | N | grace for a stray connection | no flake |
| tests/meeting_timing_lines.rs:136, tests/support/meeting.rs:136, tests/paste_target_e2e.rs:124, tests/meeting_notes_recall_eval.rs:440, tests/carbon_exclusive_probe.rs:79, tests/syscapture_manual_smoke.rs:65 | B | stub work / manual-only probes | none needed |
| tests/history_at_volume.rs, tests/long_take_budget.rs, tests/formatting_fixtures.rs, tests/meeting_eval.rs, tests/summarize_e2e.rs, tests/meeting_capture_memory.rs | B (perf prints) | elapsed is printed or compared relatively | check none asserts an absolute wall budget |
| src/record.rs:5387 `arm_signal_returns_as_soon_as_the_worker_answers` | D | `await_reply` returns `< 300 ms` after a 10 ms-delayed reply | not converted: a hang-guard rewrite would no longer catch a fixed start-poll under 300 ms (that would loosen it). Needs an injected clock in `await_reply` |
| src/transcription.rs:2981 `empty_audio_is_a_no_op_and_use_keeps_the_engine_warm` (:2994) | D | 6 x `sleep(50ms)` uses keep a 200 ms idle timer from unloading the engine; a starved test thread (> 200 ms between uses, or before the final `is_loaded`) lets the idle watcher unload it | inject a clock into the manager's idle watcher (production) and advance it explicitly |
| src/lib.rs:18258, src/transcription.rs:2815, :2913, :3034 | D (5 s / 3 s caps on drain paths that end on a lease) | 10x+ headroom over a stub path | low risk; same clock injection as engine_owner |
| src/engine_owner.rs tests (:2200, :2296, :2547, :2627, :2907, :2991, :3013: `elapsed < 1-5 s`) | D (wide caps) / lower bounds | helper-process paths with 1-5 s caps | low risk; listed for the next seat |
| src/meeting.rs, record.rs, summary_jobs.rs, stream_decode.rs, lib.rs, transcription.rs, cloud_route.rs, summarize.rs, paste_tx.rs `mod tests` sleeps of 2-150 ms (full list: `awk '/^[[:space:]]*mod tests/{t=1} t && /thread::sleep/' <file>`, 56 hits) | B / poll | stub work (`self.delay`, `load_time`, mock latency) or 2-5 ms poll steps inside `until`-guarded loops | none needed: the outcome does not depend on how long they take |

matrix_new_asr_chunk_timeout.rs was already fixed on base (per-lease `recv_timeout`, see its
header comment), so it is not re-listed.

## Production shared-deadline flaws

Reviewed in round 2: every `let deadline = Instant::now() + ...` in production `src/`
(`rg -n "let (mut )?deadline *= *Instant::now\(\) *\+" desktop/src-tauri/src`). Each bounds ONE unit of
work, none is shared across independent units:
engine_owner.rs:1018 `wait_for` (one job id; stale replies skipped inside the same job's budget),
meeting.rs:3814 `sleep_unless_stopped` (one tick), meeting.rs:3735 and record.rs:2737 (one wait),
summary_jobs.rs:482 `wait_for` / :505 `wait_idle` (one caller's wait), polish.rs:2643 and
summarize.rs:3716 (one request's own `deadline_ms`), transcription.rs:1095 (per lease: the shared
chunk deadline behind matrix_new_asr_chunk_timeout was already fixed on base).
The one production change in this item is a seam, not a flaw: `lifecycle::run_bounded` now takes an
injected `QuitClock` through `run_bounded_with` (behaviour unchanged on `SystemQuitClock`).

## Saturated before/after and the 50-run proof: INTEGRATOR

Not run here (no-compile rule). The merge seat runs, from desktop/src-tauri:

    N=$(sysctl -n hw.ncpu); for i in $(seq $((2*N))); do yes >/dev/null & echo $! >> /tmp/yes.pids; done
    for r in $(seq 20); do cargo test --tests -- --test-threads=8 > run-sat-$r.log 2>&1; echo "$r $?"; done
    kill $(cat /tmp/yes.pids)   # only the PIDs it started
    for r in $(seq 50); do cargo test --tests -- --test-threads=8 > run-$r.log 2>&1; echo "$r $?"; done

once on base e18a12d6 (before) and once on this branch (after), and records the counts here
and the logs under docs/loop/evidence/.
