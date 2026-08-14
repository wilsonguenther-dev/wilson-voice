# YV91 — the 60-minute idle repro (OS-1 / matrix row #16)

The plan's stated acceptance line for the power assertion is:

> a meeting survives 60 minutes of zero user input on battery with display sleep
> enabled and default energy settings

That is a manual repro by construction — it needs a real hour, a real battery,
and a machine nobody touches. This file records what was automated, what was
observed, and the exact procedure for the hour itself, so the claim is never
stronger than the evidence behind it.

## Automated, and green in this PR

`cargo test --test meeting_power_assertion -- --include-ignored --nocapture`
(transcript: `power-assertion-pmset.txt` in this directory).

* The assertion type is `PreventUserIdleSystemSleep` and never
  `PreventUserIdleDisplaySleep`. A display kept awake for three hours is a
  battery bug wearing a feature's clothes, so the forbidden variant is a named
  constant with a test that fails if anybody swaps it in.
* While the assertion is held, the OS agrees: `pmset -g assertions` lists

  ```
  pid 82987(meeting_power_assertion-…): […] PreventUserIdleSystemSleep named: "Yap is recording a meeting"
  ```

  and after the release, this process holds nothing. The name is the string a
  curious user finds in Activity Monitor's Energy tab.
* The failure this assertion exists to prevent is now DETECTABLE, which is what
  makes step 4 below worth running.
  `cargo test --test capture_journal_recovery a_device_that_stalls_mid_meeting_is_spliced_not_shifted`
  feeds a meeting three seconds of audio, stalls the device for five seconds of
  host time, feeds three more, and asserts the finalized track is eleven seconds
  with five seconds of explicit silence at the stall and `state = partial`. With
  the wall-clock rule removed the same feed finalizes as "6.00s, complete,
  spliced 0" — a green light on a recording that lost five seconds.
* `MeetingCapture::since_last_block` is a watchdog input, so a device that stops
  delivering for `CAPTURE_STALL_LIMIT` ends the meeting cleanly instead of
  running the clock out over silence
  (`meeting::tests::a_stalled_capture_outranks_everything_but_a_dead_device`).
* `PowerAssertion` releases in `Drop`, so a normal stop, a watchdog stop, an
  error return and a panic unwinding through the session all put the Mac's idle
  timer back the way they found it. `MeetingSession::finish` drops it explicitly
  as the last step, and `MeetingSession::drop` finalizes a session nobody
  stopped.

## NOT run in this PR

The 60-minute unattended battery run itself. It was not performed for this item
and nothing here should be read as claiming it was. It is the acceptance gate
for the phase demo (YV99), where a meeting UI exists to start one.

## Procedure, for whoever runs the hour

1. Unplug. System Settings → Lock Screen → "Turn display off on battery when
   inactive for" 2 minutes; Battery → Options → leave everything at its default
   (in particular, do **not** enable "Prevent automatic sleeping").
2. Start a meeting recording in Yap. Confirm the assertion is live:

   ```
   pmset -g assertions | grep 'Yap is recording'
   ```

   Expect one `PreventUserIdleSystemSleep` line owned by Yap's pid, and **no**
   `PreventUserIdleDisplaySleep` line owned by Yap — the display is supposed to
   go dark, that is the point.
3. Do not touch the machine for 60 minutes. The display should sleep after ~2
   minutes and stay asleep.
4. Wake it and stop the recording. Expected:
   * the finalized track is ≥ 60 minutes long — check the DURATION first, and
     against the clock rather than against itself. A track that is 55 minutes
     long after 60 minutes of wall clock has failed this repro no matter what
     the other three lines say;
   * `spliced_silence_samples` is 0, and this is now a real check rather than a
     restatement of the first one. The index records carry the anchor's
     `host_ns`, and `plan_silence_splices` measures every interval against that
     wall clock as well as against the delivered/spilled counters — so a HAL
     that stopped delivering shows up as spliced silence at the offset where it
     stopped. Before that rule existed, both counters froze together whenever
     the device went quiet, agreed perfectly, and this line read `0` on exactly
     the failure the hour is run to catch;
   * `state` is `complete`, not `partial`. A stall of any size flips the state
     to `partial`, so this line and the one above cannot disagree;
   * `pmset -g assertions | grep 'Yap is recording'` is empty afterwards.

   A stall long enough to matter is also supposed to end the meeting rather than
   be discovered at the end of it: the watchdog stops a meeting that has
   consumed no audio for `CAPTURE_STALL_LIMIT` (90 s) with
   `StopReason::CaptureStalled`. If the hour ends early with that reason in the
   log, the assertion did not hold and the repro has FAILED — with the evidence
   attached, which is the point.
5. Lid close is a **different** failure and is explicitly not covered by this
   assertion — nothing can keep a HAL running through an explicit sleep. That is
   matrix row #16's `NSWorkspaceWillSleepNotification` half: finalize, mark
   `paused_by_sleep`, and on wake offer resume-as-new-segment. This assertion
   pairs with that handling; it does not replace it.
