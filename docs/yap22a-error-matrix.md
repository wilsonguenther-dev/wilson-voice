# yap22-A — error-handling matrix, and where each row is checked

**Item:** YV99 · closes the yap22-A phase.
**Source:** the Notetaker epic plan's §6 matrix, plus the two rows finding #3 added.

The plan's §6 is seventeen rows of prose. Prose does not fail a build, so the
rows 22-A owns live in code — `desktop/src-tauri/src/meeting_matrix.rs`, as the
const table `ROWS` — and `tests/matrix_coverage.rs` walks that table against the
filesystem on every commit. The table below is **rendered from that same const**
and asserted against this file, so the document cannot drift away from what is
actually checked.

Read one row as: *this is the failure, this is what Yap owes the user when it
happens, and this is the thing you can run to see that it does.*

**A row is a cell, not a failure.** Where one failure's two halves have
different truth values, it gets two rows: row 5's bounded queue keeps the disk
off the audio callback and ships in `record.rs`, while the quality note that
would tell the user what the dropped writes cost has no caller anywhere, so the
failure is published as `5` and `5b`. One averaged cell would have to pick a
truth value and lie about the other half.

<!-- generated from meeting_matrix::render_markdown() — do not hand-edit -->

| Row | Failure | Required behaviour | Coverage |
|---|---|---|---|
| 4 | Disk fills during a 2 h recording | Pre-flight refuses with a clear number; the 60 s watchdog stops cleanly below 1 GB free, finalizes the journal, marks `state='partial'` | Lands with #108 (YV91) — `matrix_row4_disk_preflight` |
| 5 | Journal write falls behind | Bounded queue, `try_send`, drops counted, the audio callback never blocks | `cargo test --test matrix_row5_journal_backpressure` |
| 5b | Journal write falls behind — and the meeting never says what it cost | `dropped > 0` becomes an honest quality note on the meeting detail: a real number of blocks, the seconds they were worth, and that they are missing | **Policy only, NOT WIRED** — `cargo test --test matrix_row5_journal_backpressure` covers the decision; **no open PR wires `quality_note`**, so the app does not do this yet |
| 6 | App killed or crashes mid-meeting | An orphan `.in_progress.json` + `.spill.pcm` found at startup finalizes into a wav; the meeting row becomes `partial` with a Resume-processing affordance | Lands with #108 (YV91) — `matrix_row6_orphan_recovery` |
| 15 | A second Yap instance is launched during a meeting | `tauri-plugin-single-instance`, registered as the FIRST plugin, hands the argv to the running app and exits the duplicate — no second recorder, no second SQLite writer | Manual repro — `docs/yap22a-phase-demo.md` |
| 16 | Sleep or lid close mid-meeting | Finalize the journal, mark `paused_by_sleep`; on wake OFFER resume as a new segment rather than pretending the gap did not happen | **Policy only, NOT WIRED** — `cargo test --test matrix_row16_sleep_wake` covers the decision; **no open PR wires `NSWorkspaceWillSleepNotification`**, so the app does not do this yet |
| 17 | Meeting exceeds the 3 h cap | Warn once at 2 h 45 m; hard-stop once at 3 h, cleanly finalized, with a continuation meeting linked to the first | **Policy only** — the enforcement (`MEETING_HARD_CAP`) lands with #108 (YV91); `cargo test --test matrix_row17_meeting_cap` covers only the part this branch owns |
| 3a | User quits mid-processing (finding #3) | Resume from `processed_through_seconds`, never re-decode from zero | Lands with #110 (YV93) — `matrix_new_quit_mid_processing`, which that PR must **rename from `meeting_transcription_resume`** on merge so the acceptance sweep reaches it |
| 3b | An ASR chunk exceeds `TRANSCRIBE_TIMEOUT` (finding #3) | That chunk gets `text=''` + `asr_failed`; its neighbours and the rest of the meeting are unaffected | Lands with #110 (YV93) — `matrix_new_asr_chunk_timeout`, which that PR must **rename from `meeting_chunk_timeout_isolation`** on merge so the acceptance sweep reaches it |

## Why only these eight failures

Rows 1, 2, 3, 7–14 of the plan's §6 are not here, deliberately. Listing a row
with no mechanism behind it makes the matrix look more covered than it is.

| Plan row | Belongs to | Why not 22-A |
|---|---|---|
| 1, 2, 14 | 22-B | CoreAudio process taps, `NSAudioCaptureUsageDescription`, aggregate-device rebuild. 22-A ships mic-only with zero new permissions. |
| 3 | 22-B | "Mic denied ⇒ record system audio instead" needs a system-audio path to fall back to. |
| 7, 8 | yap23 | Diarization sidecar and its sanity gates. |
| 9 | YV93 | Per-chunk ASR isolation — covered here as row `3b`, which is the same failure stated with the timeout that causes it. |
| 10 | YV97 | The summarizer's own degrade ladder, tested with the summarizer. |
| 11 | yap24 | Calendar/EventKit. |
| 12, 13 | — | The 14.4 runtime gate and the model-missing gate; 22-A runs on macOS 12 and its model gate already exists. |

## Which rows the app actually performs today

**Exactly one of the nine cells is a claim that the shipped app handles the
failure**, and it is half of row 5. Read this section before quoting a row as
done.

| Cell | What it means | Rows |
|---|---|---|
| `cargo test --test …` | The behaviour is implemented, wired into the app, and exercised end to end by that test. | 5 |
| **Manual repro** | No in-process test can produce the failure. A human follows the script and records the result. | 15 |
| **Lands with #N** | The mechanism *and* its test belong to an open PR. Nothing on this branch performs it. | 4, 6, `3a`, `3b` |
| **Policy only** | The *decision* is implemented and tested here as pure state. The *call site that would put it in effect does not exist in any branch*. The app does not do this yet. | `5b`, 16, 17 |

### Manual — row 15, and why its three green tests are not its coverage

`matrix_row15_single_instance` is green and stays in the suite, but all three of
its assertions read `lib.rs` and `Cargo.toml` as *text*: they hold the plugin in
first position in the builder chain and keep the duplicate-instance handler away
from capture. That is a useful guard against a reordering, and it is not a
demonstration that a second launch during a live meeting is survivable — which
is two processes and a running recorder, and which nothing in-process can
perform. The backlog specifies this row as a manual repro; it is step 5 of
`yap22a-phase-demo.md`, folded into the same take as the offline demo. An
earlier revision published the row as `Test`, the table's strongest cell, on the
strength of a source grep.

### Lands with — rows 4, 6, `3a`, `3b`

Mechanisms YV91 (PR #108) and YV93 (PR #110) build; their tests ship in those
PRs. `Coverage::LandsWith` is not a shrug: the merge checklist in
`tests/matrix_coverage.rs` fails the moment one of those test files appears
while its row still says `LandsWith`. The fix is one line per row.

Rows `3a`/`3b` carry one extra condition, published in the cell itself: **#110
must rename its two test files** to `matrix_new_quit_mid_processing.rs` and
`matrix_new_asr_chunk_timeout.rs`. It calls them `meeting_transcription_resume`
and `meeting_chunk_timeout_isolation` today, and under those names the phase's
own acceptance command — a sweep over the `matrix_*` binaries — can never reach
them, so "all eight rows green in one run" would have been unsatisfiable by
construction rather than merely deferred. `every_row_names_a_test_the_acceptance_sweep_can_reach`
is the rule that forces the convergence, and the checklist watches for *both*
names so merging under the old ones trips it just as loudly.

### Policy only — rows `5b`, 16 and 17, and why they are not `Test`

Earlier revisions of this document published rows 16, 17 and the quality-note
half of row 5 with a plain `cargo test --test …` cell. That was false in each
case. The policies are complete and their test files are green, but a policy
with no caller is a policy that is not in effect, and a green test next to a
required-behaviour sentence reads as a shipped feature to everyone who did not
write it.

**Row `5b` — the quality note. No pull request owns this either.**
`meeting_matrix::quality_note` turns a drop count into the sentence a meeting
owes the user ("the disk fell behind: 100 audio blocks (about 3.2 s) were
dropped and are missing from this meeting"). It has zero callers: YV94 shipped
the meeting detail before there was a capture path to feed it a drop count, #108
carries the counter and a comment about a quality note but calls nothing, and
this branch changes no UI. The never-block half of the same failure — row 5 —
*is* wired, which is why the two halves are two cells.

**Row 16 — sleep / lid close. No pull request owns this.** The state machine
(`meeting_matrix::SleepPolicy`) is written and tested;
`NSWorkspaceWillSleepNotification` is registered by nothing, in no branch — #108
names it in a comment in `power.rs` and registers nothing — so
`SleepPolicy::observe` has zero callers and closing a lid mid-meeting today does
whatever it did before YV99. With row `5b`, this is one of the two gaps in
22-A's matrix that are not waiting on a queue: **somebody still has to build
it**, and until they do, the honest reading of row 16 is "we know what the app
should do and it does not do it."

**Row 17 — the 3 h cap. The enforcement is #108's, and is deliberately not
duplicated here.** `meeting::MEETING_HARD_CAP`, `meeting::MEETING_CAP_WARN_AT`
and `meeting::watchdog_tick` in PR #108 are the thresholds and the rule that run.
YV99 originally declared a second `MEETING_HARD_CAP`/`MEETING_WARN_AT` pair and a
second warn/stop latch of its own; that has been deleted. Two copies of a number
with no compile-time link between them is a specific trap — change the cap in the
module that ships, and the matrix test stays green while the row published above
as the required behaviour goes quietly wrong. `matrix_row17_meeting_cap` now
asserts the copy is *gone* and covers only the half #108 does not build (the
continuation-title rule); when #108 lands, the row's test is rewritten to import
#108's constants and drive `watchdog_tick` directly.

All three are tripwired in the opposite direction to `LandsWith`: the row's
named call site (`quality_note`, `NSWorkspaceWillSleepNotification`,
`MEETING_HARD_CAP`) must stay ABSENT from `src/` as code — a comment does not
count, and #108 has one — so a row cannot stay `PolicyOnly` after its wiring
lands any more than it can stay `LandsWith` after its test does.

That device is the same one `tests/meeting_event_contract.rs` uses for YV95's
emitter, and it exists for the same reason: a comment saying "depends on #108"
is a thing a merge queue reads zero times.

## Which tripwires gate CI, and which are a merge checklist

A tripwire fires on a change to the tree, and the tree changes underneath this
branch when somebody *else's* PR merges. #108 declares `MEETING_HARD_CAP` and
adds `matrix_row4_disk_preflight.rs`; #110 adds the two transcription tests. Left
as ordinary tests, those tripwires would turn **#108's and #110's** builds red on
assertions those PRs do not own, in a file they do not touch, fixable only by
editing `src/meeting_matrix.rs`. So:

* **CI gates** (always run): the rows nothing open owns — `5b` (`quality_note`)
  and 16 (`NSWorkspaceWillSleepNotification`). Neither symbol appears in #108 or
  #110, so the only way these go red is somebody wiring the row, which is exactly
  the event they exist to catch.
* **Merge checklist** (`#[ignore]`d, and the panic message is the instruction):
  rows 4, 6, `3a`, `3b` and 17, whose landing is on the queue. Run it the moment
  #108 or #110 lands:

```
cd desktop/src-tauri
cargo test --test matrix_coverage -- --ignored
```

**Merge order.** The cheapest sequence is to land #108 and #110 first and this
item after, flipping the four `LandsWith` rows and row 17 in one commit. If this
lands first, the checklist above is the required first step of the next merge —
it is not optional, because nothing else in the suite will tell you a row went
stale.

## Running the whole sweep

Runnable on this branch today:

```
cd desktop/src-tauri
cargo test --test matrix_coverage \
           --test matrix_row5_journal_backpressure \
           --test matrix_row15_single_instance \
           --test matrix_row16_sleep_wake \
           --test matrix_row17_meeting_cap \
           --test matrix_phase_offline
```

The phase's AC2 sweep — "all eight rows green in one run" — once #108 and #110
have landed and their tests carry their `matrix_` names:

```
cd desktop/src-tauri
cargo test --test matrix_coverage \
           --test matrix_row4_disk_preflight \
           --test matrix_row5_journal_backpressure \
           --test matrix_row6_orphan_recovery \
           --test matrix_row15_single_instance \
           --test matrix_row16_sleep_wake \
           --test matrix_row17_meeting_cap \
           --test matrix_new_quit_mid_processing \
           --test matrix_new_asr_chunk_timeout \
           --test matrix_phase_offline
```

Both commands are checked, not just written down:
`the_published_sweeps_are_runnable_and_complete` asserts the first names only
binaries that exist here (a documented command that errors teaches people to
stop running documented commands) and that the second names the test of *every*
row in `ROWS` — which is what makes AC2 a criterion that can go green rather
than a glob that quietly covers six rows of eight. Row 15 is in both sweeps as a
guard; its coverage claim is the manual repro, not the binary.

`matrix_phase_offline` is the machine-checkable half of the phase's "records
with Wi-Fi off" claim: it asserts every module on the meeting path has no
network surface at all. The other half — a real 10-minute meeting, recorded
offline, on camera — is scripted in `yap22a-phase-demo.md`.
