# Timing-dependent Rust tests: audit status lines

The full audit lives on the held branch loop3/yap-1107-timing-tests-deterministic (not taken here; main had no copy of this file, so this one starts with the status lines only).

## Status

- 2026-11-08T21:56:34Z rows 14-17 (tests/lifecycle.rs: a_meeting_finalize_longer_than_the_whole_quit_budget_is_never_cut_off, a_wedged_exempt_step_is_bounded_by_its_own_deadline_and_the_rest_still_run, a_step_that_misses_its_deadline_is_skipped_and_quit_stays_bounded, the_whole_quit_never_exceeds_its_budget_when_every_step_hangs): converted to an injected QuitClock (cherry-pick of 0e7c0031 plus review fixes) by yap-1108-timing-lifecycle-quit-clock. Steps declare their cost with FakeQuitClock::charge or hang with wedge(); the same bounds are asserted on the fake report.total, which only scheduler-free arithmetic decides. A real 60 s watchdog turns a runner that waits on a wedged step into a failure, not a CI hang. Production run_bounded keeps its signature and runs on SystemQuitClock (Instant plus recv_timeout, identical behaviour). Row 39 (a_real_two_track_meeting_finalize_completes_inside_the_quit) stays on the real clock on purpose. 20/20 stress runs under load: INTEGRATOR, logs to docs/loop/evidence/stress-harness/.
