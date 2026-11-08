# yap-1108-timing-cloud-polish-parallel: audit row 4 status

Status line for docs/loop/evidence/timing-tests-audit.md row 4 (that file is not on main or on loop3/integration-2 yet, only on the held branch loop3/yap-1107-timing-tests-deterministic, so the line lives here to avoid an add/add conflict; the merge seat appends it once main carries the audit):

- 2026-11-08T21:56:45Z row 4 `lib cloud_route::tests::a_700_word_take_cleans_up_in_under_1_5_s_against_the_mock_latency` (src/cloud_route.rs:1427): CONVERTED, test-only. The real 646 ms sleeps against a 1.5 s wall bound are replaced by a gated fake-latency executor check. No production code changed.

## Why the new assertions are equivalent or stronger

The old test slept each chunk's mock latency for real and asserted the whole run took at most 1.5 s, which only implies "some parallelism happened" and lost to thread start-up under load. The new test holds every chunk call at a gate until its wave is full, so it observes the real schedule: MAX_IN_FLIGHT (4) calls inside the model call at the same moment (a serial or under-parallel executor can never fill the first wave, trips a 30 s hang guard and fails the `stalled` assert), the number of waves is exactly ceil(chunks / 4), never more than 4 calls are in flight, every chunk is asked exactly once, the answer is reassembled in order (same assert as before), and the run fits the same 1.5 s budget in fake time (sum over waves of the slowest member's mock latency, 646 + 430 = 1076 ms for 5 chunks). The "serial would be past the budget" assert is kept unchanged. Nothing was deleted, loosened or ignored; the only wall-clock value left is the hang guard, which is a liveness guard and not a budget.

## Stress proof: INTEGRATOR

From desktop/src-tauri:

    scripts/stress-tests.sh --runs 20 lib cloud_route::tests::a_700_word_take_cleans_up_in_under_1_5_s_against_the_mock_latency

(8 yes loaders, --test-threads=8; log saved under docs/loop/evidence/), plus one normal unloaded run of the whole lib target (`cargo test --features custom-protocol --lib`).
