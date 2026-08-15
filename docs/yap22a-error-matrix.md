# yap22 — error-handling matrix, and where each row is checked

**Items:** YV99 · closed the yap22-A phase. **YV105** · added 22-B's five
tap-scoped rows to this same table.
**Source:** the Notetaker epic plan's §6 matrix, plus the two rows finding #3 added.

The plan's §6 is seventeen rows of prose. Prose does not fail a build, so the
rows yap22 owns live in code — `desktop/src-tauri/src/meeting_matrix.rs`, as the
const table `ROWS` — and `tests/matrix_coverage.rs` walks that table against the
filesystem on every commit. The table below is **rendered from that same const**
and asserted against this file, so the document cannot drift away from what is
actually checked.

Read one row as: *this is the failure, this is what Yap owes the user when it
happens, and this is the thing you can run to see that it does.*

**A row is a cell, not a failure.** Where one failure's two halves have
different truth values, it gets two rows. Row 5: the bounded queue keeps the disk
off the audio callback and ships in `meeting::MeetingJournal`, while the quality
note that would tell the user what the dropped writes cost has no caller
anywhere — so the failure is published as `5` and `5b`. Row 17 is the same shape:
the 3 h cap runs, and the continuation meeting it should hand off to does not, so
`17` and `17b`. Row 12 is the third, added by YV105: the 14.4 gate runs, and the
sentence it produces reached no surface, so `12` and `12b`. YV102 gave `12b` its
surface and both halves are `Test` now — the split stays, because they remain
two claims and a future redesign can drop one without touching the other. One
averaged cell would have to pick a truth value and lie about the other half.

**A `Test` cell names the code it drives.** That is not decoration either. Row 5
was once published as a green `cargo test` cell whose test read `record.rs` — the
*dictation* journal — while the row is about the meeting journal a three-hour
recording lives or dies by. Two bounded queues, two `try_send`s, two drop
counters: the test passed and was evidence for nothing. Every `Test` row now
names its shipping subject and the module it lives in, and
`matrix_coverage::test_rows_drive_the_shipping_code_they_name` fails unless that
symbol is real code in that module *and* appears as code in the row's test —
not in a comment, and not inside a string literal, because a check that counted
literals would be satisfied by a test that only asserts what the table says about
itself.

<!-- generated from meeting_matrix::render_markdown() — do not hand-edit -->

| Row | Failure | Required behaviour | Coverage |
|---|---|---|---|
| 4 | Disk fills during a 2 h recording | Pre-flight refuses with a clear number; the 60 s watchdog stops cleanly below 1 GB free, finalizes the journal, marks `state='partial'` | `cargo test --test matrix_row4_disk_preflight` — drives `required_free_bytes` in `meeting.rs` |
| 5 | Journal write falls behind | Bounded queue, `try_send`, drops counted, the audio callback never blocks | `cargo test --test matrix_row5_journal_backpressure` — drives `MeetingJournal` in `meeting.rs` |
| 5b | Journal write falls behind — and the meeting never says what it cost | `dropped > 0` becomes an honest quality note on the meeting detail: how many seconds of audio the disk cost, and that they are missing | **Policy only, NOT WIRED** — `cargo test --test matrix_row5_journal_backpressure` covers the decision; **nothing calls `quality_note`**, so the app does not do this yet |
| 6 | App killed or crashes mid-meeting | An orphan `.in_progress.json` + `.spill.pcm` found at startup finalizes into a wav; the meeting row becomes `partial` with a Resume-processing affordance | `cargo test --test matrix_row6_orphan_recovery` — drives `recover_orphaned_meetings` in `meeting.rs` |
| 15 | A second Yap instance is launched during a meeting | `tauri-plugin-single-instance`, registered as the FIRST plugin, hands the argv to the running app and exits the duplicate — no second recorder, no second SQLite writer | Manual repro — `docs/yap22a-phase-demo.md` |
| 16 | Sleep or lid close mid-meeting | Finalize the journal, mark `paused_by_sleep`; on wake OFFER resume as a new segment rather than pretending the gap did not happen | **Policy only, NOT WIRED** — `cargo test --test matrix_row16_sleep_wake` covers the decision; **nothing calls `NSWorkspaceWillSleepNotification`**, so the app does not do this yet |
| 17 | Meeting exceeds the 3 h cap | Warn once at 2 h 45 m; hard-stop once at 3 h, cleanly finalized | `cargo test --test matrix_row17_meeting_cap` — drives `watchdog_tick` in `meeting.rs` |
| 17b | …and the recording that was still going has nowhere to carry on | A continuation meeting is created and linked to the one the cap stopped, titled so a 9 h session reads as three meetings rather than three copies of one | **Policy only, NOT WIRED** — `cargo test --test matrix_row17_meeting_cap` covers the decision; **nothing calls `continuation_title`**, so the app does not do this yet |
| 3a | User quits mid-processing (finding #3) | Resume from `processed_through_seconds`, never re-decode from zero | `cargo test --test matrix_new_quit_mid_processing` — drives `JsonProgressStore` in `meeting_asr.rs` |
| 3b | An ASR chunk exceeds `TRANSCRIBE_TIMEOUT` (finding #3) | That chunk gets `text=''` + `asr_failed`; its neighbours and the rest of the meeting are unaffected | `cargo test --test matrix_new_asr_chunk_timeout` — drives `TranscriptionManager` in `transcription.rs` |
| 1 | System-audio (tap) permission denied at start | The meeting still records, mic-only, badged `system audio unavailable`, with one deep link to the right Settings pane — never aborted | **Policy only, NOT WIRED** — `cargo test --test matrix_row1_tap_permission_denied` covers the decision; **nothing calls `start_system_tap`**, so the app does not do this yet |
| 2 | Tap permission revoked, or the tap dies mid-meeting | A `track_lost` marker into the meeting, Track A keeps recording, a banner in the pill — the meeting is NEVER stopped | **Policy only, NOT WIRED** — `cargo test --test matrix_row2_tap_revoked_mid_meeting` covers the decision; **nothing calls `start_system_tap`**, so the app does not do this yet |
| 3 | Mic permission denied | The meeting still starts, system-audio only — a webinar or a lecture stream is a real meeting — and says which track it is missing | **Policy only, NOT WIRED** — `cargo test --test matrix_row3_mic_denied_system_only` covers the decision; **nothing calls `meeting_start_plan`**, so the app does not do this yet |
| 12 | macOS older than 14.4 | The system-audio track is refused with one plain sentence naming the requirement, and mic-only meeting recording keeps working all the way down to Yap's macOS 12 floor | `cargo test --test matrix_row12_macos_144_gate` — drives `meeting_availability_for` in `meeting_asr.rs` |
| 12b | …and the sentence the gate produces has to reach a Notetaker surface | The Notetaker's system-audio control is visible and disabled, carrying that sentence — the plan's own wording for row 12 | `cargo test --test matrix_row12_macos_144_gate` — drives `NotetakerStatus` in `lib.rs` |
| 14 | Output device changes mid-meeting (AirPods connect) | Tear down and rebuild the aggregate around the new output device, splice the spill, log a `device_change` marker — this happens constantly in real use and is not an edge case | **Policy only** — the enforcement (`watch_output`) lands with #123 (YV100); `cargo test --test matrix_row14_output_device_change` covers only the part this branch owns |

## Why these thirteen failures and not the other four

Six of the plan's §6 rows are still not here, and each is absent for its own
reason. Listing a row with no mechanism behind it makes the matrix look more
covered than it is; not saying why it is absent makes the boundary look like an
oversight instead of a decision.

| Plan row | Belongs to | Why not here |
|---|---|---|
| 7, 8 | yap23 | Diarization sidecar OOM and its cluster sanity gate. Diarization does not exist yet to fail. |
| 9 | YV93 | Per-chunk ASR isolation — already general-purpose in the shipped chunker and published here as row `3b`, which is the same failure stated with the timeout that causes it. It applies identically to a tap track, so there is nothing tap-specific to add. |
| 10 | YV97 | The summarizer's own degrade ladder, tested with the summarizer, and track-agnostic. |
| 11 | yap24 | Calendar/EventKit. Calendar does not exist yet to be revoked. |
| 13 | — | Model missing / partially downloaded. 22-B introduces no new downloadable model; the only model-gated feature in the epic is diarization, which is yap23's. |

Rows 1, 2, 3, 12 and 14 **were** on this list until YV105. They are in the table
above now — four of them as `Policy only`, which is the finding of that item and
is explained below.

## Which rows the app actually performs today

**Seven of the sixteen cells are a claim that the shipped app handles the
failure.** Read this section before quoting a row as done.

| Cell | What it means | Rows |
|---|---|---|
| `cargo test --test …` | The behaviour is implemented, wired into the app, and exercised end to end by that test — against the named shipping symbol. | 4, 5, 6, 12, `12b`, 17, `3a`, `3b` |
| **Manual repro** | No in-process test can produce the failure. A human follows the script and records the result. | 15 |
| **Policy only** (an owning PR named) | The decision ships and is tested; the thing that would *produce* the event it decides about is in an open PR. | 14 |
| **Policy only, NOT WIRED** | The *decision* is implemented and tested as pure state. The *call site that would put it in effect does not exist, and no open PR brings it*. The app does not do this yet. | 1, 2, 3, `5b`, 16, `17b` |

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

### Test — rows 4, 5, 6, 12, 17, `3a`, `3b`

Rows 4 and 6 are YV91's mechanisms (disk preflight + watchdog, orphan recovery);
`3a` and `3b` are YV93's (the resume ledger, per-chunk timeout isolation); row 17
is YV91's watchdog cap, driven here through the *imported* `MEETING_HARD_CAP` /
`MEETING_CAP_WARN_AT` rather than a second copy of the numbers. All of them are
on `main`, which is why these cells are `Test` rather than the `LandsWith`
promissory note earlier revisions of this document carried.

Row 5 is the one that was rewritten rather than flipped: its test now drives
`meeting::MeetingJournal` itself — a depth-1 queue with its writer parked, 200
real `append` calls — and asserts what the row actually claims. No hand-off
waits (checked per call, so a blocking regression fails in seconds instead of
parking the suite), every refused sample lands in the counter the gap detector
reads, and the queue takes writes again once the writer catches up rather than
staying poisoned for the rest of a three-hour meeting.

`3a` and `3b` also had a rename outstanding: YV93 shipped them as
`meeting_transcription_resume.rs` and `meeting_chunk_timeout_isolation.rs`, names
the phase's acceptance sweep can never reach, so "all eight rows green in one
run" would have stayed unsatisfiable by construction. They are renamed to
`matrix_new_quit_mid_processing.rs` and `matrix_new_asr_chunk_timeout.rs`, and
`every_row_names_a_test_the_acceptance_sweep_can_reach` is the rule that keeps
every row's test reachable by the published command.

Row **12** is YV105's addition to this list and the only one of the five tap
rows that reached it. It is worth saying why it did while rows 1, 2 and 14 did
not: the 14.4 gate needs no tap in order to be in effect. It is a *refusal*,
`meeting_asr::meeting_availability_for` computes it on every `notetaker_status`
call for both capture modes, and its load-bearing half is a property of the app
as it stands today — **the gate can never refuse mic-only recording, on any OS,
including an OS version that could not be read at all**. A gate that leaked
there would turn "system audio needs a newer Mac" into "meetings do not work on
your Mac" for every pre-14.4 user, with a green build and no error anywhere,
which is why that is the half `matrix_row12_macos_144_gate` spends most of its
assertions on. The half of the plan's row-12 sentence that is about a *visible,
disabled control* is published separately as `12b`, and YV102 made it true: the
Settings step invokes `notetaker_status` on mount and renders
`systemAudioMessage` under a disabled "Set up meeting recording" control, so a
pre-14.4 Mac sees the affordance and the reason instead of nothing at all.

### Policy only — rows 3, `5b`, 16 and `17b`, and why they are not `Test`

Earlier revisions of this document published rows 16, 17 and the quality-note
half of row 5 with a plain `cargo test --test …` cell. That was false in each
case. The policies are complete and their test files are green, but a policy
with no caller is a policy that is not in effect, and a green test next to a
required-behaviour sentence reads as a shipped feature to everyone who did not
write it.

**Row `5b` — the quality note. Nobody owns this.**
`meeting_matrix::quality_note` turns the journal's drop counter into the sentence
a meeting owes the user ("the disk fell behind while recording: about 3.2 s of
audio was dropped and is missing from this meeting"). It has zero callers: YV94
shipped the meeting detail before there was a capture path to feed it a drop
count, YV91 carries the counter and a log line but no surface, and this branch
changes no UI. It does at least take what the caller will have —
`MeetingJournal::dropped_samples()` is samples, so the note takes samples and a
rate rather than a block count nothing measures. The never-block half of the same
failure — row 5 — *is* wired, which is why the two halves are two cells.

**Row 16 — sleep / lid close. Nobody owns this.** The state machine
(`meeting_matrix::SleepPolicy`) is written and tested;
`NSWorkspaceWillSleepNotification` is registered by nothing — `power.rs` names it
in a comment and registers nothing — so `SleepPolicy::observe` has zero callers
and closing a lid mid-meeting today does whatever it did before YV99. This is one
of 22-A's three gaps that are not waiting on a queue: **somebody still has to
build it**, and until they do, the honest reading of row 16 is "we know what the
app should do and it does not do it."

**Row `17b` — the meeting that carries on. Nobody owns this.** The watchdog stops
a recording at three hours and stops there. `continuation_title` decides what the
next meeting is called (`Lecture` → `(continued)` → `(continued 2)`, never
`(continued) (continued)`), and nothing calls it, so a capped meeting today ends
rather than continuing. Row 17 can only be `Test` honestly because this half has
its own cell.

**Row 3 — the system-audio-only meeting. Nobody owns this either.** YV105's
addition to this list. `MeetingSession::start` holds one `CaptureStream`, and a
`hold()` that returns `Err` ends the attempt — right for 22-A, where the mic is
the only source and a meeting without it is nothing, and wrong the moment there
are two sources. `meeting_matrix::meeting_start_plan` is the rule for that case
(a meeting starts if *either* source opened, and says which track it is missing;
it refuses only when neither did, naming the one the user can act on), and
nothing calls it, so a denied microphone today still ends the meeting rather
than recording the lecture stream. The change that would wire it is a change to
the capture session, which is YV106's.

All four are tripwired in the opposite direction to a `Test` row: the row's
named call site (`meeting_start_plan`, `quality_note`,
`NSWorkspaceWillSleepNotification`, `continuation_title`) must stay ABSENT from
`src/` as code — a comment does not count, and `power.rs` has one — so a row
cannot stay `PolicyOnly` after its wiring lands.

That device is the same one `tests/meeting_event_contract.rs` uses for YV95's
emitter, and it exists for the same reason: a comment saying "somebody should
wire this" is a thing a merge queue reads zero times.

### 22-B's tap rows — 1, 2, 14, and what merging #123 and #125 actually moved

These rows are a different shape from the ones above: their decisions have
merged (YV101's gate, YV103's device-change state machine, YV104's ghost
watchdog), and what was missing is the thing that *produces* the events those
decisions consume — the CoreAudio tap itself (#123, YV100) and the pre-warm and
Settings step (#125, YV102). Both have now landed, and the honest result is that
one row was promoted and two were not.

* **Row `12b` → `Test`.** The 14.4 sentence has a surface: `App.tsx` invokes
  `notetaker_status` and renders `systemAudioMessage` beneath a disabled setup
  step. This is the row that merging #125 genuinely delivered.
* **Rows 1 and 2 → `Policy only, NOT WIRED`, owner removed.** `syscapture.rs`
  ships and `prewarm_tap` is called from Settings, but **nothing calls
  `start_system_tap`**: no meeting opens a tap, so `CaptureEnv::tap_liveness`
  still returns `None` for every environment and `meeting::watchdog_tick`'s tap
  branch is still unreachable. **A tap that cannot exist cannot die.** Row 1's
  denial is a denial *at the start of a meeting*; answering the same question in
  Settings, minutes earlier, is a different row. No item in the 22-B backlog
  owns starting the tap inside a meeting — that is a genuine planning gap, and
  publishing a merged PR as the pending owner would read as progress and
  describe none.
* **Row 14** keeps `#123 (YV100)` as its owner: nothing calls
  `InputFormatWatch::watch_output`, so the output half of that watch is inert —
  `record.rs` handles a `RebuildAggregate` action by logging that one arrived on
  the mic path, where it means nothing — and #123 merging is what makes wiring
  it possible.

Each unpromoted row asserts its own named call site is still absent, in its own
test, so the day something opens a tap in a meeting the row goes red with
instructions to promote it, exactly as the unowned rows already do.

## Every tripwire in this matrix is a CI gate

An earlier revision kept some of them behind `#[ignore]` as a merge checklist,
because a tripwire that fires on somebody *else's* merge would redden that PR's
build over assertions it does not own — sound while YV91 and YV93 were open PRs.
They have merged, YV99 consumed the checklist (rows 4, 6, 17, `3a` and `3b`
flipped, and `3a`/`3b`'s files renamed), and nothing this matrix depends on is
outstanding. There is no `--ignored` step left: every assertion runs on every
commit, which is the only state in which "CI checks the matrix" is true without
a footnote.

YV105 kept every one of those rules unchanged and applied them to five new rows.
The two assertions it did edit in `tests/matrix_coverage.rs` are inventories
rather than rules — the list of row ids the table carries, and the count of
policies with no owner — and both exist precisely so that changing the phase's
scope has to be a deliberate act with a diff attached.

## Running the whole sweep

The phase's AC2 sweep — every row of the matrix, green in one run:

```
cd desktop/src-tauri
cargo test --test matrix_coverage \
           --test matrix_row1_tap_permission_denied \
           --test matrix_row2_tap_revoked_mid_meeting \
           --test matrix_row3_mic_denied_system_only \
           --test matrix_row4_disk_preflight \
           --test matrix_row5_journal_backpressure \
           --test matrix_row6_orphan_recovery \
           --test matrix_row12_macos_144_gate \
           --test matrix_row14_output_device_change \
           --test matrix_row15_single_instance \
           --test matrix_row16_sleep_wake \
           --test matrix_row17_meeting_cap \
           --test matrix_new_quit_mid_processing \
           --test matrix_new_asr_chunk_timeout \
           --test matrix_phase_offline
```

That command is checked, not just written down:
`the_published_sweep_is_runnable_and_complete` asserts it names only binaries
that exist here (a documented command that errors teaches people to stop running
documented commands) *and* that it names the test of every row in `ROWS` — which
is what makes AC2 a criterion that can go green rather than a glob that quietly
covers six rows of eight. That rule is also why YV105's five rows appear in the
command above: a row added to the table without being added here fails the
build. Row 15 is in the sweep as a guard; its coverage claim is the manual
repro, not the binary.

`matrix_phase_offline` is the machine-checkable half of the phase's "records
with Wi-Fi off" claim: it asserts every module on the meeting path — capture,
journal, ring, control plane, chunker, transcription, storage, summary, export
and policy — has no network surface at all. The other half — a real 10-minute
meeting, recorded offline, on camera — is scripted in `yap22a-phase-demo.md`.
