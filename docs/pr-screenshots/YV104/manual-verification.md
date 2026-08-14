# YV104 — manual verification, and what could NOT be verified

## What could NOT be run, stated plainly and first

The backlog's last YV104 criterion is *"Manual repro documented in the PR: a
real ~20-minute recording captures at least one natural silence/renegotiation
event without the meeting badging itself as permission-denied (best-effort; the
bug is intermittent by nature, so the manual note documents what was observed
rather than forcing a repro)."*

**That repro is not runnable at this commit, and this PR does not claim it.**
Three independent reasons, each sufficient on its own:

1. **There is no tap.** `syscapture.rs`'s CoreAudio half — `CATapDescription`,
   `AudioHardwareCreateProcessTap`, the aggregate-device dictionary and the
   IOProc block — is YV100's, and YV100 is not merged. `main` is at YV103
   (`02f7bfa`). This item lands the module's watchdog half and the seven-step
   sequence YV100 will bind its FFI to; it makes no CoreAudio call and cannot,
   because there is nothing to call it on.
2. **A real tap needs the TCC grant**, and TCC is keyed to the code-signing
   identity — YV101's own spec records that precondition, and DGR Labs records
   the half hour they lost to an unsigned build that never prompted. No signed,
   notarized build with `SystemAudioCaptureRequests` granted was available for
   this item.
3. **The bug is intermittent by construction.** Thread 825780's reporter needed
   a 51-minute session on an M2 Air to see it, and the backlog says so itself
   ("best-effort … documents what was observed rather than forcing a repro"). A
   twenty-minute recording that does *not* ghost proves nothing either way.

The backlog's own note anticipates exactly this: *"where the backlog says
hardware taps can't run in CI, the acceptance is the PURE state-machine/fixture
test plus a documented manual-verification transcript if you can run one
locally."* The pure tests are the gate here, and they are the stronger artefact:
they replay OS-4's reported failure deterministically instead of hoping to meet
it.

## What WAS run, on this machine

macOS 26.5.2 (25F84), arm64. All transcripts in this directory.

* `acceptance-tests.txt` — the three named suites (21 tests), the module's own
  8 unit tests, and `matrix_phase_offline` proving the meeting path grew a
  module and stayed offline. All green.
* `ghost-session-transcript.txt` — the substitute for the manual repro, and the
  closest thing to it that exists at this commit: **thread 825780's actual
  51-minute session**, hole for hole (60 s, 53 s, 141 s, 16 min 3 s, 3 min 8 s),
  driven through the shipping state machine on a real 60 s watchdog tick. It
  prints every action the watchdog took and the exact JSON blob that lands on
  the meeting row. This is the criterion's *intent* — "does a healthy-but-
  glitched meeting badge itself permission-denied?" — answered against harder
  data than a twenty-minute recording would have produced.
* `seven-step-sequence-declared-once.txt` — the grep criterion, plus the proof
  that the sequence exists in exactly one function rather than being duplicated.
* Full local gate: `cargo test` — **60 test binaries, 834 tests, 0 failures**;
  `npx tsc --noEmit` clean; `npm test` 86 vitest tests green; `cargo test -p
  yap-polish --release` 11 green; `cargo build --release --features
  custom-protocol` clean; `scripts/assert-weak-linked-14_4-symbols.sh` **PASS**
  (this item adds no CoreAudio symbol, and the check proves it did not).
  `rustfmt --check` and `cargo clippy` are clean on every file this PR touches.

## What the transcript actually shows, including the uncomfortable part

Against thread 825780's own timings the watchdog rebuilds at 6:00, 16:00 and
17:00, recovers each time, and then **degrades during the 16-minute hole** —
because the budget is three rebuilds *per meeting*, which is what OS-4
specifies, not three per incident. A session that survives three separate holes
and meets a fourth loses Track B's banner even though every rebuild before it
worked.

That is the specified behaviour and it is in the transcript on purpose rather
than smoothed over. Whether the budget should replenish after a sustained
recovery is a real question; it is not this item's to answer, and answering it
quietly here would have made the stored `tap_rebuilds` count mean something
different from what OS-4 asked for.

The degrade also turns nothing off. The watchdog stops nothing, mutes nothing
and destroys nothing, so the tap that comes back at 41:03 in that transcript
keeps writing into the journal. The verdict records what happened; it does not
gate what happens next.

## The one thing a reviewer should check by hand

`TapWatchdogAction` has four variants and none of them stops a meeting. Matrix
row #2's rule — a lost system-audio track degrades the meeting and never ends
it, because Track A is still recording the person holding the Mac — is enforced
by the *type*, not by a comment, and
`no_tap_outcome_can_ever_stop_a_meeting` in
`syscapture_ghost_watchdog_rebuild_first.rs` is the exhaustive match that fails
to compile if a stop variant is ever added.


---

# Revision — review round 2, both BLOCKING findings

The review found two ways this watchdog fired on a **healthy** meeting, both
reproduced at the shipping 60 s tick, both the ordinary path rather than an edge
case. Both were real. Neither is argued with here; the reproductions are in
`review-round-2-false-positives.txt`, produced by mutating the shipping module
back to its as-submitted behaviour with the new test file held constant.

## Finding 1 — a granted tap with nothing to record, badged "permission is off"

Reproduced exactly as reported: `system_output_active: Some(false)` held for
twenty minutes produced

    RebuildFull { attempt: 1, kind: AllZeroBuffers, causes: SilenceCauses(2) }   t=60s
    RebuildFull { attempt: 2, kind: AllZeroBuffers, causes: SilenceCauses(2) }   t=120s
    RebuildFull { attempt: 3, kind: AllZeroBuffers, causes: SilenceCauses(2) }   t=180s
    DegradeTrackLost { verdict: PermissionLikelyDenied, after_rebuilds: 3, … }   t=240s

— three real CoreAudio tap+aggregate teardown/recreate cycles in the first four
minutes of an in-person meeting, each one emitting the device-change
notifications YV103's guard exists to absorb, followed by a false privacy
accusation. `SilenceCauses(2)` is bit 1: `EveryoneMuted`. The module computed
its own explanation, attached it to the very same action, and then rebuilt
anyway. That is what "the enumerated causes are decorative" means, in the
module's own output.

**What was wrong in the reasoning, not just in the code.** `ever_nonzero`
distinguishes a TCC denial from OS-4's ghost — that is exactly what OS-4 claims
for it and all it can do. It does not distinguish a denial from *genuine
silence*, and the first cut treated the two as one question.

**The fix, in two parts.**

1. `silence_is_explained` — silence while nothing is playing (and the IOProc is
   still firing) never starts the budget. The cause is now acted on instead of
   recited. A dead IOProc is deliberately NOT covered: a quiet room does not
   explain callbacks that stopped, so `IoProcSilent` still rebuilds.
2. A permission verdict now requires positive evidence — `Some(true)` observed
   while the tap was quiet, latched in `GhostState::output_active_observed`.
   Without it the degrade carries the new `TapVerdict::NoSystemAudioObserved`,
   whose banner says no audio was playing and never mentions permission.
   `TapVerdict::blames_permission()` is the single place that question is
   answered.

The three other causes (wrong output device, inverted `exclusive` flag, nil
dispatch queue) deliberately still ride along without suppressing: the rebuild
might actually fix those.

## Finding 2 — a tap that is DELIVERING, rebuilt again and then degraded

Also reproduced exactly: after one genuine rebuild whose caller never reported
back, a tap with `since_nonzero == 20 ms` — audio on every callback — produced

    RebuildFull { attempt: 2, kind: PreviousRebuildTimedOut, … }   t=160s equivalent
    RebuildFull { attempt: 3, kind: PreviousRebuildTimedOut, … }
    DegradeTrackLost { verdict: GhostTapUnrecovered, after_rebuilds: 3, … }

with the banner "System audio stopped coming through and could not be
restarted" on a tap that was recording fine. And the review's sharpest point was
right: this was the **default** path, not an exception. The in-flight timeout
was 20 s while `meeting::WATCHDOG_INTERVAL` is 60 s, so an in-flight rebuild was
always already expired at the next tick — the "wait for it" branch was
unreachable in the shipping configuration — and no call site in `src/` calls
`finish_rebuild`.

**The fix, in three parts.**

1. `ghost_tick` reads liveness **before** the timeout. A tap that is no longer
   ghost-silent closes its open attempt (`RebuildSettled { Succeeded }`), clears
   `rebuild_issued_at`, and nothing is torn down.
2. `TAP_REBUILD_IN_FLIGHT_TIMEOUT` is derived from `meeting::WATCHDOG_INTERVAL`
   (two intervals) rather than written down as a number, so the wait branch is
   reachable at the interval the product ticks at — and stays reachable if the
   interval ever changes.
3. A stale flag on a tap whose silence turned out to be explained is released as
   `TapRebuildOutcome::Unknown` rather than spending another attempt. "We do not
   know whether that rebuild helped" is a different fact from "it never
   reported", and the log now distinguishes them.

## What did NOT change

The OS-4 ghost path is untouched, and `ghost-session-transcript.txt` re-run
after the fix prints the same ladder line for line and the same stored blob
(rebuilds at 6:00 / 16:00 / 17:00, degrade in the 16-minute hole,
`ghost_tap_unrecovered`), which is why that file is unchanged in this
revision. The fix removes false
positives; it does not make the watchdog slower to answer a real ghost. Rule (a)
is also intact: with silence that has no innocent explanation, the FIRST action
is still a full rebuild however long the silence has run, and
`syscapture_ghost_watchdog_rebuild_first.rs` still asserts it.

## Still not claimed

Everything under "What could NOT be run" at the top of this file stands
unchanged: there is no tap at this commit, no signed build with the TCC grant,
and no hardware repro. The new suite is a pure state-machine test like the rest,
and it is proved non-vacuous by mutation rather than by assertion.
