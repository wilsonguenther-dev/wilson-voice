# YV107 — manual verification, and the honest gaps

## Revision — review round 3 (a headline claim in this file did not survive a reopen)

The row below reading *"<50 ms residual at 3 h → **0.016 ms**"* was measured on a
fixture in which neither stream is ever REOPENED, and the review found that the
claim does not hold when one is. It now says so, and the code now earns it.

`record.rs::build_capture_stream` rebases `host_ns` to each stream build's own
first callback — it has to, because `cpal` only defines
`StreamInstant::duration_since` within one stream — and
`MeetingCapture::retune_track`, the shipped and documented handler for exactly
that reopen (its own doc comment names the AirPods swap; the track-1 caller is
YV103's `RebuildAggregate`), deliberately carries `captured_samples` and
`spilled_samples` across the seam. So the wav is continuous and the clock under
it restarts at zero.

The round-2 map merely DROPPED the backwards records. That survives only while
the new clock stays under the old maximum; once it passes it, every later record
is believed on the NEW zero and **the whole post-reopen remainder of the meeting
is re-timed by the length of the pre-reopen run**. A five-minute-in swap in a
ninety-minute meeting put a word spoken at session second 4000 at 3700.000 — a
300,000 ms residual against this item's 50 ms budget, reproduced here before it
was fixed. The old doc comment's "dropping it costs the resolution of one
interval" was false whenever the post-reopen run outlives the pre-reopen one,
which is the common case and near-certain on track 1.

The same seam fabricated a measurement. `measure_true_rate` took `first` from
`records[0]` and let `last` advance past the seam, dividing the WHOLE meeting's
samples by the part of it the new clock had reached: a clean 0 ppm device with a
five-minute-in reopen reported **16941.176 Hz, +58,823.5 ppm, flagged, ≈635,000
ms of projected drift** — and that is the exact datum OS-2 says the deferred
single-drift-compensated-aggregate escalation gets decided on, so fabricating it
is worse than reporting nothing.

Both consumers now SEGMENT on the seam. The map closes a segment and opens the
next with its epoch set to the session time already elapsed (derived from the
finalized position the two runs meet at, the one axis the reopen did not
disturb), so session time is continuous and monotone across it by construction.
The measurement is taken over one monotonic run — the longest — and `TrackRate`
carries a new `segments` count so a reader can see the meeting was reopened
rather than having to infer it. The existing rebase test used a 600 s pre / 60 s
post fixture, which is the one regime in which the defect cannot show; the new
fixtures are 300 s pre / 5100 s post. Mutations 14–17 are the guard.

**One honest gap remains, and it is now measured rather than claimed.** The dead
air between the old stream's last callback and the new stream's first is audio
that was never captured, and the two clocks are rebased to different instants, so
nothing in the sidecar measures the distance between them. Post-reopen spans are
early by exactly that gap — sub-second, once, bounded by how long a stream
rebuild takes rather than by how long the meeting runs, and it does NOT
accumulate, which is the whole difference between it and the ppm drift this item
deletes. `a_reopen_is_early_by_its_own_dead_air_and_by_nothing_else` asserts both
halves. Closing it needs a session offset stamped into the sidecar at
`retune_track`; that is a capture-side change and it is not in this commit.

## Revision — review round 2 (a claim in this file was wrong)

The first version of this document, of the PR body, of `HostTimeline`'s doc
comment and of `tests/support/two_track.rs`'s header all asserted the same
premise: *"the finalize splices exactly `captured − spilled`, so the finalized
wav is `captured`-long."* **That premise is false about the
`plan_silence_splices` shipping in this same repository**, and it was load-
bearing: the host-time map was keyed on `captured_samples` because of it.

`plan_silence_splices` splices `dropped.max(stalled)`, and the STALL rule is
derived from the WALL CLOCK (`elapsed × TARGET_RATE − spilled`). A stall is
precisely the case where no callback fires, so `captured` and `spilled` freeze
together — `captured − spilled == 0` — while the splice is large, and the
finalized wav comes out **longer** than `captured`. Everything after the stall
was then mis-timed by the whole stall: 60 s clean / 10 s stalled / 50 s clean
put a word spoken at host second 80 at session second 90, a 9,999 ms residual
against this item's own 50 ms budget. Two secondary paths ran through the same
mismatch — sub-threshold drops (`< SPLICE_MIN_SAMPLES`) widen `captured −
spilled` and are deliberately never spliced, and the system track is the most
exposed of the two, since a process tap gets no callbacks at all while the
tapped app is silent, which makes stall splices routine on track 1 rather than
exotic.

The map is now keyed on the finalized position — `spilled_samples` plus the
silence of every splice planned at or before that record, derived by running the
shipping planner (`meeting::finalized_positions`) rather than by re-deriving its
rules. That reduces to `captured_samples` under the counter rule, which is why
the lossy fixture passed either way and hid it. The rows and mutations added for
it are marked *(round 2)* below.

## No screenshots

This item changes no user-visible surface. It adds one pure function
(`meeting_asr::merge_two_tracks_by_host_time`), one measurement
(`meeting::measure_true_rate`) and one key in a diagnostics blob nothing renders.
The Me/Them transcript a person can actually look at is **YV108's**, which is the
consumer of this function; the Meetings list and the Markdown export are
byte-identical to what they were before this commit. There is nothing to
photograph.

## The manual check the acceptance criteria ask for was NOT performed

The criterion reads:

> Manual, documented in the PR: a real recording with mic on the built-in device
> and system audio tapped while a Bluetooth output (AirPods) is active — the
> real-world clock-mismatch configuration OS-2's evidence names — produces a
> transcript with no visibly out-of-order Me/Them turns.

**That recording was not made, and it could not have been made from this
commit.** Stated plainly rather than approximated:

1. **There is no code path in this repository that starts a system-audio tap.**
   `syscapture.rs` on `main` today is the pure state machine YV104 landed — the
   ghost watchdog, the 7-step rebuild sequence as data, the
   never-delivered-a-non-zero-sample discriminator. Every occurrence of
   `AudioHardwareCreateProcessTap` in that file is a doc comment or an enum
   variant name; the module makes no FFI calls. YV100's CoreAudio call sequence
   is not merged.
2. **Nothing in production constructs a two-track session.**
   `SessionConfig::virtual_meeting` exists (YV106) and has no caller outside
   tests, and `MeetingCapture::accept_track` has no producer for track 1.
3. A real tap would additionally need the `NSAudioCaptureUsageDescription` TCC
   grant on this Mac (the key is in `Info.plist` as of YV101; the grant is not
   held).

So: **no real two-track recording was performed for this PR, and the pure tests
are the gate.** That is exactly the note YV106's own manual-verification file
made for the same reason, and the reason has not changed.

## Why the synthetic fixture is the right gate here anyway — not a consolation prize

OS-2's central observation is that this defect is *invisible to manual checking
on the machine you would check it on*:

> built-in mic + built-in speakers on Apple Silicon are one clock domain
> (drift ≈ 0), so every desk test passes; drift appears exactly in the real
> meeting configuration.

A desk recording on this Mac would therefore have produced a green transcript
whether or not the merge was correct — it would have been evidence of nothing.
The backlog anticipates this and writes the acceptance criterion accordingly:
*"which is why this item's acceptance criterion is a **synthetic offset
fixture**, not a desk recording."* The AirPods run is the human-facing
confirmation on top of that gate, and it belongs to the phase-closing E2E
(YV109) and the real tap (YV100), which is where a two-track recording will
first be possible.

## What WAS verified, and how

Every number below is reproducible from this commit with the commands in
`acceptance-tests.txt`.

| Claim | How it was verified | Result |
|---|---|---|
| <50 ms residual at 3 h, 100 ppm offset, no reopen | `two_track_merge_synthetic_drift`, 10,801 synthetic index records per track | **0.016 ms** (budget 50 ms) |
| A stream REOPENED mid-meeting (round 3) | `index_records_rebased` — 300 s pre / **5100 s post**, `host_ns` back to zero at the seam while both counters run on, as `retune_track` leaves them | word spoken at session second 4000 placed at **4000.000**; the drop-only filter puts it at 3700.000, a 300,000 ms residual |
| The reopen's remaining cost is a one-off, not a drift (round 3) | the same fixture with 1.5 s of dead air across the rebuild, sampled 20 s and 3700 s after the seam | both offsets **1.5 s**, equal to within the budget — it does not grow |
| A clean device is not slandered by a reopen (round 3) | `measure_true_rate` on 300 s / 5100 s at 0 ppm | **0.000 ppm**, not flagged, `segments = 2`; measuring across the seam reports +58,823.5 ppm and flags it |
| The fixture really contains the defect | the same file's naive `samples ÷ nominal_rate` merge, run on the same fixture | **1079.1 ms**, and the last answer lands before its question |
| Correct order across three hours | exact equality against the real-time turn sequence | 8/8 turns in order |
| Ordering with misaligned seams | `two_track_merge_ordering` — track 0 cut on a 30 s clock, track 1 on 47 s silences | strictly monotonic, 28 turns interleaved |
| A tap that started late | epoch offset of 2 s applied and recovered | first word placed at session second 2.000 |
| A track that lost audio | `index_records_lossy` — spilled trails captured by 2 s (spread over the two intervals it really takes; a counter does not run backwards) | word still placed at real second 80 |
| A track whose DEVICE stalled (round 2) | `index_records_stalled` — 60 s clean / 10 s with `host_ns` advancing while **both** counters freeze / 50 s clean; the shipping `plan_silence_splices` asserted to splice all 160,000 samples | word spoken at host second 80 placed at **80.000** (0.0 ms); keying on either stored counter puts it at 89.999 — a 9,999 ms residual |
| Per-track true rate measured | `two_track_true_rate_logging` over 0 / ±12 / ±37.5 / 100 / −220 ppm | within 0.2 ppm of truth in every case |
| Flagged, not silently accepted | ±20–50 ppm band passes clean; 51 / 100 / −180 ppm flag and warn | as specified |
| It lands in the row | real `MeetingController` stop path into a real SQLite file, `diagnostics` read back and parsed | `track_rates` present, mic clean, tap flagged |

## What these tests do NOT prove

* Nothing here proves a live CoreAudio tap's `mHostTime` is delivered in the
  shape `IndexRecord::host_ns` expects, because no live tap exists to ask. The
  merge consumes the anchors YV91/YV106 already write; whether track 1's
  producer fills them correctly is YV100's claim to make.
* `TrackEpochs` is a parameter, deliberately, and nothing in this commit
  computes a real one — a two-track session that never starts cannot have a real
  epoch offset between two streams. The session-side bookkeeping that stamps it
  belongs with the code that starts the second stream. Passing
  `TrackEpochs::SHARED` is a claim the caller makes out loud rather than one this
  module assumes, and `a_tap_that_started_late_is_placed_on_the_sessions_clock_not_its_own`
  is what will fail if a future caller passes zeros when it should not.
* The function has no production caller yet. YV108 renders its output and YV109
  runs it through the eval harness end to end — the backlog sequences it that
  way on purpose. The **measurement** half, by contrast, IS wired all the way
  through on this commit (finalize → `FinalizedMeeting::track_rates` →
  `CaptureOutcome` → `MeetingDiagnostics` → the `diagnostics` column), and
  `a_finished_meetings_row_carries_the_measured_rates` drives that whole path.
