# Timing-dependent Rust tests: audit (yap-1107-timing-tests-deterministic)

Written 2026-11-08T05:04:46Z by loop-three coder B1, against base e18a12d6 (origin/main).
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
| tests/summary_download_lock.rs:150 | `sleep(60ms)` then assert the job holds the slot | S | the job thread took the slot within 60 ms | `wait_until(download_in_progress(&dest).is_some())` |
| tests/summary_download_lock.rs:205, :235 (`cancel_after`, `since_raise` at :47) | latency = `t0.elapsed() - 300ms` | X | the cancel-setter thread wakes exactly at 300 ms; its lateness was charged to the download | the setter stamps the instant it raises the flag; latency is measured from that stamp. The 500 ms bound is unchanged |
| tests/system_audio_verdict_silence_is_not_denial.rs:333 | `sleep(2_300ms)` then `samples >= 1` | S | the pump thread gets a turn after its ~1 s sample interval within 2.3 s | poll `tap.output_evidence().samples` until >= 1 (60 s hang guard) |
| tests/cancel_long_take.rs:224 | `sleep(CHUNK_DECODE * 4)` then assert a decode is in flight | S | the decoder thread started and crossed boundaries within 160 ms | `wait_until(chunks_done >= 2)` |
| tests/cancel_long_take.rs:450 | `sleep(90ms)` "a couple of windows" | S | two 30 ms windows decoded within 90 ms | `wait_until(windows_decoded >= 2)` |
| tests/capture_journal_recovery.rs:122, tests/matrix_row6_orphan_recovery.rs:68 and :176, tests/meeting_retention_keeps_untranscribed_audio.rs:171, tests/two_track_journal_round_trip.rs:357 | `sleep(300-500ms)` "for the 250 ms flush timer" before `abandon()` | B (slow) | the timer must fire before the crash | removed: `abandon()` joins the writer (`MeetingJournal::hang_up`, meeting.rs), which drains the queue, writes the last index records and flushes every spill before exiting. 2.1 s of dead wall clock gone; outcome identical |

Residual note, documented not hidden: the counter waits in cancel_long_take and
summary_download_lock still need the engine / stub to be slower than the test thread's poll
(20 x 40 ms, 12 x 30 ms, 24 x 15 ms of paced work). A test thread starved for that whole span
would make the decode finish first. A one-shot gate inside the stub engine would close it
entirely; it was not done in this seat.

## Open: found, not converted in this seat (needs an injected clock in production code)

| file:line | class | what it assumes | proposed fix |
|---|---|---|---|
| tests/lifecycle.rs:143, :181-187, :296, :331, :465 | D | `run_bounded` quit steps finish within budget + 200 ms / under 1 s of wall clock | give `lifecycle::run_bounded` a `Clock` (now + wait_until) and drive a fake clock; assert the budget arithmetic, not the wall |
| tests/meeting_dictation_preempts_transcription.rs:171-185, :394 | D | a preempting dictation completes under `DICTATION_BUDGET` while a 400 ms chunk is in flight | count "chunk boundaries waited" in the scheduler instead of elapsed wall time |
| tests/dictation_chunked.rs:324-339 | D | per-window `recv_timeout` budget vs a 50 ms stub delay | inject the per-window budget clock into the chunk worker |
| tests/diarize_sidecar_pool.rs:250 | D | a sticky-failed pool answers in under 200 ms | assert "no launch, no wait entered" via a counter on the pool instead of elapsed |
| tests/diarize_sidecar_pool.rs:279, :308; tests/matrix_row7_diarize_sidecar_wedge.rs:82 | D (lower bound + 10x upper) | a real child process and a real readiness deadline | low risk (10x headroom); a fake clock in `DiarizePool` would make it exact |
| tests/matrix_row5_journal_backpressure.rs:97, :131, :173 | D | an overflowing journal append returns under 500 ms / 2 s | count blocking waits in the journal instead of timing them |
| tests/summary_round3.rs:250; tests/summary_download_lock.rs:188 | D | no retry after cancel (< 2 s); backoff total < 6 s | expose the backoff schedule (attempt count) and assert it |
| tests/permission_transitions.rs:192, :205, :212 | N + D | no emit within 400 ms; the revoke emit within 2 s | wait for the watcher's read counter to advance past the poke, then `try_recv` |
| tests/polish_long_form.rs:142, :430, :654-672; tests/transforms.rs:71 | S/D | a stub sleeping past its deadline; a 25 s wall budget | stub should block on a gate the deadline path opens |
| tests/cloud_route_mock.rs:119-136, :251 | D | 400 ms TTFT against 20 ms / 1200 ms stub pieces | 3x-30x margins, low risk; fake TTFT clock in `cloud_route` |
| tests/meeting_manual_start_stop.rs:450, :540-573; tests/meeting_diagnostics_row.rs:26 | S/D | ticker thread timing (already reshaped once, YV111) | review again with a fake ticker |
| tests/two_track_journal_round_trip.rs:286-291, tests/capture_journal_recovery.rs:204-208 | S | the writer resumes within 20 ms of unpause | wait on a writer "resumed" signal from `pause_handle` |
| tests/meeting_retention_keeps_untranscribed_audio.rs:276; tests/matrix_row4_disk_preflight.rs:242; tests/meeting_no_model_resident.rs:80; tests/meeting_track_b_wiring.rs:118; tests/dictation_pinned_model.rs:106-108 | N/B | "let periodic work run"; the asserted outcome does not depend on how many ticks happened | no flake; optional speed-up |
| tests/cloud_route_mock.rs:452 | N | grace for a stray connection | no flake |
| tests/meeting_timing_lines.rs:136, tests/support/meeting.rs:136, tests/paste_target_e2e.rs:124, tests/meeting_notes_recall_eval.rs:440, tests/carbon_exclusive_probe.rs:79, tests/syscapture_manual_smoke.rs:65 | B | stub work / manual-only probes | none needed |
| tests/history_at_volume.rs, tests/long_take_budget.rs, tests/formatting_fixtures.rs, tests/meeting_eval.rs, tests/summarize_e2e.rs, tests/meeting_capture_memory.rs | B (perf prints) | elapsed is printed or compared relatively | check none asserts an absolute wall budget |
| desktop/src-tauri/src `#[cfg(test)]` modules (lib.rs 52, engine_owner.rs 43, transcription.rs 39, record.rs 32 timing hits incl. production) and desktop/yap-* | not audited | | next seat: same grep restricted to `mod tests` blocks |

matrix_new_asr_chunk_timeout.rs was already fixed on base (per-lease `recv_timeout`, see its
header comment), so it is not re-listed.

## Production shared-deadline flaws

None fixed in this seat. The chunk-timeout shared deadline was fixed on base. The audit of
`src/` production deadlines is open (row above).

## Saturated before/after and the 50-run proof: INTEGRATOR

Not run here (no-compile rule). The merge seat runs, from desktop/src-tauri:

    N=$(sysctl -n hw.ncpu); for i in $(seq $((2*N))); do yes >/dev/null & echo $! >> /tmp/yes.pids; done
    for r in $(seq 20); do cargo test --tests -- --test-threads=8 > run-sat-$r.log 2>&1; echo "$r $?"; done
    kill $(cat /tmp/yes.pids)   # only the PIDs it started
    for r in $(seq 50); do cargo test --tests -- --test-threads=8 > run-$r.log 2>&1; echo "$r $?"; done

once on base e18a12d6 (before) and once on this branch (after), and records the counts here
and the logs under docs/loop/evidence/.
