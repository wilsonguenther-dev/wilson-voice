# Timing-dependent Rust tests: audit status lines

The full audit lives on the held branch loop3/yap-1107-timing-tests-deterministic (not taken here; main had no copy of this file, so this one starts with the status lines only).

## Status

- 2026-11-08T21:56:34Z rows 14-17 (tests/lifecycle.rs: a_meeting_finalize_longer_than_the_whole_quit_budget_is_never_cut_off, a_wedged_exempt_step_is_bounded_by_its_own_deadline_and_the_rest_still_run, a_step_that_misses_its_deadline_is_skipped_and_quit_stays_bounded, the_whole_quit_never_exceeds_its_budget_when_every_step_hangs): converted to an injected QuitClock (cherry-pick of 0e7c0031 plus review fixes) by yap-1108-timing-lifecycle-quit-clock. Steps declare their cost with FakeQuitClock::charge or hang with wedge(); the same bounds are asserted on the fake report.total, which only scheduler-free arithmetic decides. A real 60 s watchdog turns a runner that waits on a wedged step into a failure, not a CI hang. Production run_bounded keeps its signature and runs on SystemQuitClock (Instant plus recv_timeout, identical behaviour). Row 39 (a_real_two_track_meeting_finalize_completes_inside_the_quit) stays on the real clock on purpose. 20/20 stress runs under load: INTEGRATOR, logs to docs/loop/evidence/stress-harness/.
- 2026-11-08T22:00Z yap-1108-timing-src-ordering-resume, rows 7-13, 21, 22: CODE DONE, stress runs pending (integrator).
  record.rs stop edge = mpsc gate, the context read blocks until the capture end ran (no clock);
  stop_signal = waiter announces its park before stop is raised, 60 s hang guard;
  journal never-block (frame + device-change marker) = helper-thread offers while nobody drains, 60 s hang guard;
  meeting.rs launch scan = wait for every index file (the writer's last create) before the snapshot; crash test drops its sleep (abandon joins the writer);
  quit drain = BlockingEngine.entered flag;
  cancel_long_take = one-shot ParkOnce gate inside both stub engines (residual closed: the decode cannot finish before the cancel), plus a new exact assert crossed == PARK_AFTER_CHUNKS;
  summary_download_lock = stub parks mid-body until Settings is seen waiting (mirrored progress > 0 and < total); backoff stub holds every retry until the cancel is raised; stall stub stalls until the client hangs up; latency measured from the stamped raise instant.
  No assertion loosened, no #[ignore]. Wall bound replaced only in the stop-edge, stop-signal and never-block tests, each by an ordering assertion a broken build can never pass.
