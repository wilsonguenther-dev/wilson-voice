# yap22-A — phase-closing demo script (offline, on camera)

**Item:** YV99. **Purpose:** replace the epic plan's unverified "22 ships alone,
that's already a product" claim (finding #30) with a recording of it happening.

This is the phase's stated acceptance, verbatim from the backlog: *a 10-minute
meeting completes start-to-finish (record → transcribe → summarize → export)
with **Wi-Fi off**, demonstrated on camera.*

## Status

**Not yet runnable — and that is the honest state of the phase, not a gap in
this document.** The demo needs four pull requests that are open at the time of
writing:

| PR | Item | What the demo needs from it |
|---|---|---|
| #108 | YV91 | The meeting capture session itself (journal, power assertion, 3 h cap wiring) |
| #110 | YV93 | Timed chunked transcription |
| #112 | YV95 | The manual start/stop entry point — ⌃⌘M and the tray item |
| #114 | YV97 | The summary stage |

The machine-checkable half of the "with Wi-Fi off" claim does not wait for them:
`cargo test --test matrix_phase_offline` asserts every module on the meeting
path has no network surface at all, on every commit. Run the script below once
the four PRs are on `main`, attach the recording, and record the numbers in the
results table at the bottom.

**Two matrix rows are not on that list and are not waiting on a queue.** Row 16
— sleep or lid close mid-meeting — has its policy written and tested
(`matrix_row16_sleep_wake`) and its **call site written by nobody**: no branch
registers `NSWorkspaceWillSleepNotification`, so the demo below deliberately
does not include a lid-close step and the phase does not claim that failure is
handled. Row `5b` is the same shape: `meeting_matrix::quality_note` computes the
sentence a meeting owes the user when the disk cost it audio, and no surface
calls it, so a meeting that dropped writes still says nothing about it. Somebody
has to build both; see `yap22a-error-matrix.md` § *Policy only*. Row 17's 3 h cap
is a different case — its enforcement is real and ships in #108, it is simply
longer than a ten-minute demo can exercise.

**Step 5 below is row 15's coverage, not a bonus.** The matrix publishes that row
as a manual repro pointing at this script, because the failure is two live
processes and no in-process test can start one. Record the result of step 5 in
the results table with the rest.

## Before you start

* An installed build of `main` (a DMG, not `npm run tauri dev` — the demo is
  about the shipped app).
* Models already downloaded: Settings → Model shows the ASR model ready, and one
  prior dictation has run so the Silero VAD cache exists. Model acquisition is
  the app's one first-launch download; every real user is past it too.
* Ten minutes of something to say. A recorded lecture playing in the room is
  fine and is the 22-A target case.
* Screen recording on, with the menu bar visible — the Wi-Fi state has to be in
  frame.

## The script

1. **Show the network is off.** Turn Wi-Fi off in the menu bar on camera. If the
   Mac is on Ethernet, unplug it in frame too. Leave the menu-bar indicator
   visible for the rest of the recording.
2. **Start the meeting** with ⌃⌘M (YV95). On a first-ever meeting the one-time
   capture notice appears (YV96) — read it on camera, close it, and note that it
   did not block the recording starting.
3. **Record for ten minutes.** Do not touch the machine for the first five —
   that is also the smaller sibling of YV91's *60 minutes of zero user input on
   battery* repro, and the pill should still be recording when you come back.
4. **Fire a dictation mid-meeting** (the normal hotkey) around minute six. It
   must complete normally, and the meeting must keep recording with no gap
   (YV91's fan-out).
5. **Row 15, in the same take:** double-click Yap.app in Finder while the
   meeting is still running. The second launch must focus the running window and
   exit; the meeting must be unaffected — check afterwards that the recording
   has no seam at that timestamp, that there is exactly one meeting row, and
   that no second `.in_progress.json` was created in the recovery directory.
   This is the row's coverage of record: no in-process test can start a second
   process, and `matrix_row15_single_instance` only greps the builder chain.
6. **Stop the meeting.** Watch it move through `transcribing` → `summarizing` →
   `complete` in the Meetings tab.
7. **Read the result on camera:** the transcript with monotonic timestamps, the
   summary (≤250 words) and the flat action list, and the meeting's duration.
8. **Export** to Markdown and open the file.
9. **Only then, turn Wi-Fi back on.** Everything above happened with no network.

## What counts as a pass

* The meeting is `complete`, not `partial` or `failed`.
* Transcript timestamps are monotonic and cover the full ten minutes.
* A summary and an action list exist and refer to things that were actually said
  (YV97's V1–V7 validators are the automated version of this check; the demo is
  the human one).
* The exported Markdown opens and reads correctly.
* The dictation fired at step 4 landed in History as a normal dictation.
* The second instance at step 5 changed nothing.
* Wi-Fi was off, on camera, for steps 2–8.

## What counts as a fail worth stopping the phase for

Any of: audio lost at the dictation fan-out; a transcript with non-monotonic or
duplicated seam text; a meeting stuck in `transcribing`; a summary that names a
date or an @-address nobody said; an export that loses segments.

## Results

Fill this in when the demo is run; link the recording.

| Date | Build | Duration | State | Row 15 step | WER (if measured) | Recording |
|---|---|---|---|---|---|---|
| _pending #108/#110/#112/#114_ | | | | | | |
