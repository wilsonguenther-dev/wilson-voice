# yap22 — error-handling matrix, and where each row is checked

**Items:** YV99 · closed the yap22-A phase. **YV105** · added 22-B's five
tap-scoped rows to this same table. **YV110** · wired the tap into a live
meeting, which promoted rows 1 and 2. **YV132** · added yap23's three
diarization rows (7, 8, 13), all three as `Policy only`.
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
| 1 | System-audio (tap) permission denied at start | The meeting still records, mic-only, badged `system audio unavailable`, with one deep link to the right Settings pane — never aborted | `cargo test --test matrix_row1_tap_permission_denied` — drives `track_b_plan` in `syscapture.rs` |
| 2 | Tap permission revoked, or the tap dies mid-meeting | A `track_lost` marker into the meeting, Track A keeps recording, a banner in the pill — the meeting is NEVER stopped | `cargo test --test matrix_row2_tap_revoked_mid_meeting` — drives `TappedEnv` in `syscapture.rs` |
| 3 | Mic permission denied | The meeting still starts, system-audio only — a webinar or a lecture stream is a real meeting — and says which track it is missing | **Policy only, NOT WIRED** — `cargo test --test matrix_row3_mic_denied_system_only` covers the decision; **nothing calls `meeting_start_plan`**, so the app does not do this yet |
| 12 | macOS older than 14.4 | The system-audio track is refused with one plain sentence naming the requirement, and mic-only meeting recording keeps working all the way down to Yap's macOS 12 floor | `cargo test --test matrix_row12_macos_144_gate` — drives `meeting_availability_for` in `meeting_asr.rs` |
| 12b | …and the sentence the gate produces has to reach a Notetaker surface | The Notetaker's system-audio control is visible and disabled, carrying that sentence — the plan's own wording for row 12 | `cargo test --test matrix_row12_macos_144_gate` — drives `NotetakerStatus` in `lib.rs` |
| 14 | Output device changes mid-meeting (AirPods connect) | Tear down and rebuild the aggregate around the new output device, splice the spill, log a `device_change` marker — this happens constantly in real use and is not an edge case | **Policy only** — the enforcement (`watch_output`) lands with #123 (YV100); `cargo test --test matrix_row14_output_device_change` covers only the part this branch owns |
| 7 | Diarize sidecar OOM, panic or wedge | Deadline + kill + restart budget 1; on give-up a plain single-speaker transcript, the meeting `complete` with `diarization_failed` — never `failed`, because the user still gets their notes | **Policy only, NOT WIRED** — `cargo test --test matrix_row7_diarize_sidecar_wedge` covers the decision; **nothing calls `diarize_sidecar_degrade`**, so the app does not do this yet |
| 8 | Diarization returns garbage — one cluster for five people, or forty clusters | A ranking + floor, never a hard reject: clusters under the caller's floor roll into one bucket, and a pass with nothing above it degrades to the same plain transcript as row 7 — logged distinctly, so a crashed sidecar and a noisy one stay diagnosable apart. **The degrade half is a requirement on YV126's `diarize::rank_and_floor` (#141, open) that it does not meet today** — there an all-below-floor pass renders as one "Other" chip with no degrade and no marker; `matrix_row8_diarize_garbage_clusters` fails the day that gate ships with nothing applying the degrade | **Policy only, NOT WIRED** — `cargo test --test matrix_row8_diarize_garbage_clusters` covers the decision; **nothing calls `cluster_sanity`**, so the app does not do this yet |
| 13 | Diarization model missing, or partially downloaded | The same gate the polish stage uses — the speaker-detection affordance is OFF, not broken, naming the combined download; recording and transcripts are untouched | **Policy only, NOT WIRED** — `cargo test --test matrix_row13_diarize_model_missing` covers the decision; **nothing calls `speaker_detection_gate`**, so the app does not do this yet |

## Where the plan's other rows are

Three of the plan's §6 rows are still not here, and each is absent for its own
reason. Listing a row with no mechanism behind it makes the matrix look more
covered than it is; not saying why it is absent makes the boundary look like an
oversight instead of a decision. The two rows this table used to hold and no
longer does are kept in it, pointing at where they went, because a boundary that
moves is worth recording rather than quietly deleting.

| Plan row | Belongs to | Where it is |
|---|---|---|
| 7, 8 | yap23 · YV132 | **In the table above.** Diarization exists to fail now: YV121 shipped the `yap-diarize` sidecar and its pool, YV123 shipped the two vendored models. Both rows are `Policy only` — no meeting is ever diarized — which is [explained below](#the-yap23-rows--7-8-and-13-and-why-none-of-them-is-test). |
| 9 | YV93 | Per-chunk ASR isolation — already general-purpose in the shipped chunker and published here as row `3b`, which is the same failure stated with the timeout that causes it. It applies identically to a tap track, so there is nothing tap-specific to add. |
| 10 | YV97 | The summarizer's own degrade ladder, tested with the summarizer, and track-agnostic. |
| 11 | yap24 | Calendar/EventKit. Calendar does not exist yet to be revoked. |
| 13 | yap23 · YV132 | **In the table above.** 22-B introduced no downloadable model, so this row had nothing to gate; yap23's two diarization models are that something, and `models::is_diarize_downloaded` is the gate over them. `Policy only`: no Notetaker surface offers speaker detection to switch off. |

Rows 1, 2, 3, 12 and 14 **were** on this list until YV105. They are in the table
above now — four of them as `Policy only`, which is the finding of that item and
is explained below.

## Which rows the app actually performs today

**Ten of the nineteen cells are a claim that the shipped app handles the
failure.** Read this section before quoting a row as done.

| Cell | What it means | Rows |
|---|---|---|
| `cargo test --test …` | The behaviour is implemented, wired into the app, and exercised end to end by that test — against the named shipping symbol. | 1, 2, 4, 5, 6, 12, `12b`, 17, `3a`, `3b` |
| **Manual repro** | No in-process test can produce the failure. A human follows the script and records the result. | 15 |
| **Policy only** (an owning PR named) | The decision ships and is tested; the thing that would *produce* the event it decides about is in an open PR. | 14 |
| **Policy only, NOT WIRED** | The *decision* is implemented and tested as pure state. The *call site that would put it in effect does not exist, and no open PR brings it*. The app does not do this yet. | 3, `5b`, 16, `17b`, 7, 8, 13 |

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

### 22-B's tap rows — 1, 2, 14, and what YV110 actually moved

These rows are a different shape from the ones above: their decisions merged
first (YV101's gate, YV103's device-change state machine, YV104's ghost
watchdog), and what was missing was the thing that *produces* the events those
decisions consume — a tap inside a real meeting. #123 (YV100) shipped the tap
module deliberately unwired and #125 (YV102) shipped the Settings pre-warm, so
for two merges the honest cell was still `Policy only, NOT WIRED`: **a tap that
cannot exist cannot die.**

**YV110 is the wiring, and it promoted rows 1 and 2.** A meeting now asks
`syscapture::track_b_plan` at T-0 whether this Mac and this install attach Track
B — YV101's runtime OS gate, then YV102's setup row — and `SessionEngine::start`
calls `start_system_tap` when the answer is yes. When the answer is no the
meeting records mic-only and carries the sentence saying why, on the same 1 Hz
status payload the pill and the main window already render. That is row 1.
`syscapture::TappedEnv` is the first shipping `CaptureEnv` to answer `Some` to
`tap_liveness`, folded from the drain that feeds track 1, so
`meeting::watchdog_tick`'s tap branch runs in a real session and the `track_lost`
degrade is reachable. That is row 2.

Both promotions were forced by the tripwires, not chosen: each row's absence
test and `matrix_coverage`'s unowned-policy sweep went red on the commit that
added the caller, and the fix was to promote the rows and rewrite their tests
against the shipping surface.

**What row 2 still does not claim.** The seven CoreAudio calls of a mid-meeting
rebuild are *decided and logged, not executed*. A rebuilt tap gets a fresh
`TapClock` and rebases its `host_ns` epoch, and YV107's cross-track merge reads
exactly that field, so running the rebuild without solving clock continuity
would move every post-rebuild span of Track B to the start of the meeting. The
behaviour row 2 publishes — marker, mic keeps recording, banner, meeting never
stopped — is what ships; the executor is a separate item.

**What no test in this repository proves.** That a real Zoom / Meet / Teams call
lands on Track B. That needs a signed, notarized build, a TCC grant and a human,
and it is written down as a manual script in
[`MEETING-DEMO.md`](MEETING-DEMO.md).

* **Row 14** keeps `#123 (YV100)` as its owner: nothing calls
  `InputFormatWatch::watch_output`, so the output half of that watch is inert —
  `record.rs` handles a `RebuildAggregate` action by logging that one arrived on
  the mic path, where it means nothing — and #123 merging is what makes wiring
  it possible.

Rows 1 and 2 assert the inverse of what they used to: their tripwires now check
that `start_system_tap` is still CALLED from `src/`, so a refactor that drops the
call turns both rows red instead of leaving them published as `Test` about a
meeting with no second track. Row 14 still asserts its own call site is absent.

## The yap23 rows — 7, 8 and 13, and why none of them is `Test`

`ROWS` reserved these three for "yap23, which does not exist yet to fail". It
exists now — **YV121** shipped the `yap-diarize` sidecar and `diarize::DiarizePool`
(readiness handshake, deadline, one-restart budget, idle sweep), **YV123** shipped
the two vendored models and `models::is_diarize_downloaded` over them — so the
reservation is spent and the rows are published.

**All three are `Policy only, NOT WIRED`, and that is this item's finding rather
than a shortfall in it.** Nothing in the shipping app runs a diarization pass:
`diarize::pool()` has no caller anywhere in `src/`, no meeting is handed to a
sidecar, and no surface offers speaker detection to turn on or off. A `Test` cell
on row 7 would tell a reader *"if the diarizer wedges, your meeting still
completes with a plain transcript"* — about a meeting that never asks a diarizer
anything. That is the false-capability claim rows 1 and 2 were held back from
through four merges, and the rule does not get an exemption for the phase this
table has only just started covering.

**Row 7 — the sidecar wedges, OOMs or panics.** The budget half genuinely ships:
`matrix_row7_diarize_sidecar_wedge` drives the real pool against **stub
processes** (a `/bin/sh` script that never answers; one that announces itself and
dies), and asserts the wedge is killed at the readiness budget and the crash is
respawned exactly once. The half that is not wired is what the meeting becomes:
`meeting_matrix::diarize_sidecar_degrade` maps every `DiarizeError` to a plain
single-speaker transcript on a **complete** meeting marked `diarization_failed`.
The mapping is exhaustive over the error enum rather than a catch-all, so a new
failure mode is a compile error and a decision, and `SpeakerLabels` has **no
variant that fails or discards a meeting** — "the user still gets their notes" is
a property of the type, not a branch somebody has to remember.

**Row 8 — the pass ran and returned noise.** Same artifact, different reason, and
that difference is the whole of the row: a crashed sidecar and a noisy one are
two different bugs, and one undifferentiated `diarization_failed` makes them one
line in a log that tells nobody which happened. **The published behaviour is not
the plan's.** §6 says "cluster count > `max(8, attendees×2)` ⇒ reject"; merged
finding #25 killed that rule, because a manually started meeting has no attendee
count (so the cap is 8) and a real six-person far-field room legitimately
produces 10–15 raw clusters — the case yap23 prioritises would have its whole
diarization thrown away by the gate meant to protect it. `cluster_sanity` ranks,
applies the caller's floor, rolls the rest into one "Other" bucket, and degrades
only when nothing survives. And it says out loud what it cannot see: *one cluster
for five people* clears every floor and is undetectable without ground truth —
that half is YV126's DER gate on the eval fixtures, which is a measurement, not a
runtime check.

**Row 8's degrade is a requirement on YV126's gate, and it is unmet today.**
This is the one place in the table where the published behaviour and the code
that will serve it are known to disagree, so it is stated rather than left for a
reader to discover. #141 ships `diarize::rank_and_floor` — the gate a meeting
will actually call — and on the identical forty-fragment pass this row
publishes, the two answers are:

| | row 8 (`meeting_matrix::cluster_sanity`) | #141 (`diarize::rank_and_floor`) |
|---|---|---|
| labels | `Plain(Clusters { raw_clusters: 40 })` | `ClusterRanking { surfaced: [], other: 40 }` |
| chips | none | `["Other"]` |
| marker | `diarization_failed` | none |
| transcript | plain, single speaker | diarized, every line "Other" |

`rank_and_floor`'s *never reject* half is right and is exactly what finding #25
asked for. But never reject is not never degrade: a pass in which nothing at all
cleared the floor found no speaker, and rendering that as one "Other" chip
presents noise as attribution. Two assertions keep the disagreement from
outliving this PR. The row's absence tripwire now names the shipping symbols
(`rank_and_floor`, `cluster_track`, `attribute_clusters`) alongside
`cluster_sanity`, so wiring YV126's gate reddens row 8 rather than sailing past a
tripwire aimed at a function nobody will call; and
`the_published_degrade_is_a_requirement_on_yv126s_gate_and_the_two_may_not_diverge`
goes red the day `rank_and_floor` lands with nothing in `src/` applying the
degrade. Exactly one of the two fires, and neither outcome leaves row 8 as it is.

**Not one accuracy number is declared for these rows.** `cluster_sanity` takes
its floor as an argument; `meeting_matrix.rs` holds no threshold constant, and
`matrix_row8_diarize_garbage_clusters` asserts that from both ends — the source
carries no such `const`, and the same cluster set flips verdict under two
different floors. Those numbers are YV126's to measure against `diarize_metrics`
on the eval fixtures. A plausible-looking constant here would be a vendor-blog
threshold entering the tree through the file that renders the published matrix,
which is the worst available door for one.

**Row 13 — the model is missing, or half of it is.** The gate's input ships:
`is_diarize_downloaded` compares the extracted graph against the size the catalog
states, so an interrupted download reads as *missing* rather than as
present-and-corrupt, and `matrix_row13_diarize_model_missing` drives that against
a real file on disk at three lengths (absent, short, exact). What does not exist
is the affordance — `NotetakerStatus` carries no speaker-detection field and no
surface renders one — so the sentence is decided and reaches nobody.
**The size in it is computed, and the plan's number was wrong:** §6 row 13 says
"download 37 MB", written before the assets were vendored; the two entries YV123
pinned are 6,958,444 B + 29,292,684 B = 36.25 MB, so the sentence says **36 MB**,
rounded the same way `ModelSetup.tsx` renders every other catalog size. Passing
`models::diarize_download_bytes()` in is what keeps it true the day a model is
re-pinned. Its other half — *recording is unaffected* — is asserted three ways,
including that `meeting_asr.rs` never reads the diarization catalog as code: the
moment the recording gate consults it, a 36 MB download becomes a precondition
for recording a meeting at all, which is row 12's leak with a different door.

**What YV132 changed in the rules: nothing.** Two assertions in
`tests/matrix_coverage.rs` moved — the list of row ids, and the count of policies
with no owner — and both are inventories of the phase's scope rather than rules,
which is exactly the pair YV105 moved for the same reason. Every tripwire applied
to the three new rows unchanged, and all three had to satisfy them to be
published at all.

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
           --test matrix_row7_diarize_sidecar_wedge \
           --test matrix_row8_diarize_garbage_clusters \
           --test matrix_row13_diarize_model_missing \
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
