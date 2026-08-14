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

`TapWatchdogAction` has three variants and none of them stops a meeting. Matrix
row #2's rule — a lost system-audio track degrades the meeting and never ends
it, because Track A is still recording the person holding the Mac — is enforced
by the *type*, not by a comment, and
`no_tap_outcome_can_ever_stop_a_meeting` in
`syscapture_ghost_watchdog_rebuild_first.rs` is the exhaustive match that fails
to compile if a stop variant is ever added.
