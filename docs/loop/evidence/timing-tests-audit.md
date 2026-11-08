# Timing tests audit — status lines

Main carries no copy of the audit itself (it stayed on the held branch
loop3/yap-1107-timing-tests-deterministic and is deliberately not taken). This
file holds the status lines items append for their rows.

- 2026-11-08T22:00Z yap-1108-timing-src-ordering-resume, rows 7-13, 21, 22: CODE DONE, stress runs pending (integrator).
  record.rs stop edge = mpsc gate, the context read blocks until the capture end ran (no clock);
  stop_signal = waiter announces its park before stop is raised, 60 s hang guard;
  journal never-block (frame + device-change marker) = helper-thread offers while nobody drains, 60 s hang guard;
  meeting.rs launch scan = wait for every index file (the writer's last create) before the snapshot; crash test drops its sleep (abandon joins the writer);
  quit drain = BlockingEngine.entered flag;
  cancel_long_take = one-shot ParkOnce gate inside both stub engines (residual closed: the decode cannot finish before the cancel), plus a new exact assert crossed == PARK_AFTER_CHUNKS;
  summary_download_lock = stub parks mid-body until Settings is seen waiting (mirrored progress > 0 and < total); backoff stub holds every retry until the cancel is raised; stall stub stalls until the client hangs up; latency measured from the stamped raise instant.
  No assertion loosened, no #[ignore]. Wall bound replaced only in the stop-edge, stop-signal and never-block tests, each by an ordering assertion a broken build can never pass.
