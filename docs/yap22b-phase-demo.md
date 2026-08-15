# yap22-B — phase-closing demo script (a real tapped call, on camera)

**Item:** YV109. **Purpose:** the human half of 22-B's closing claim. The
machine half runs on every commit and needs no hardware; this is the half that
needs a real meeting, a real tap and a real TCC grant.

This is the phase's stated acceptance, verbatim from the backlog: *a real meeting
recorded with mic + a real tapped application (Zoom, Meet or Teams) completes end
to end and produces a legible interleaved transcript, Wi-Fi off for the
transcribe/merge/render portion.* It mirrors `yap22a-phase-demo.md` rather than
inventing a different bar.

## Status

**Still not runnable, and this document says exactly what is missing** — the
same posture YV105 took for the matrix rows whose call sites had not landed.
Publishing a script for a capability the app cannot reach would be four
paragraphs that read like a feature and describe nothing.

Every PR this document was waiting on has now landed. What is missing is no
longer a pull request: it is a **production caller**. Nothing in a shipped build
starts a two-track meeting — `syscapture::virtual_meeting_config` and
`meeting::fan_out_tap_block` are reachable only from tests — so the tap never
opens outside the test suite and there is nothing yet to point a camera at.
Whoever wires that up is the one who can finally run this, and the "Results"
table below stays empty until they do.

| PR | Item | What the demo needs from it |
|---|---|---|
| merged | YV100 (#123) | `syscapture.rs` — the process tap itself, feeding track 1 of the journal |
| merged | YV102 (#125) | The TCC pre-warm step in Settings, so the grant can be given before a call starts |
| merged | YV107 (#130) | The host-time cross-track merge — without it the two tracks have no common clock and "interleaved" has no meaning |
| merged | YV101 | The macOS 14.4 runtime gate |
| merged | YV106 | The two-track session and schema migration 3 |
| merged | YV108 | The Me/Them render and the Markdown export |

**The machine-checkable half runs today, on every commit, with no hardware:**

```sh
cd desktop/src-tauri
cargo test --test two_track_phase_e2e -- --nocapture
```

A synthetic two-source, deliberately clock-offset capture goes start to finish —
record both tracks → per-track transcription → merge by host time → render
Me/Them — and comes out as one transcript with zero out-of-order spans. Its
negative control runs the same chain with the rebase removed and asserts the
conversation comes out wrong, so a green run means something. What it cannot do
is prove a CoreAudio tap ever delivered a frame; that is what the script below is
for.

**The corpus half** adds real synthesized speech and the real decoder to the same
chain, on a fixture built so the clocks genuinely disagree:

```sh
cargo test --test meeting_eval meeting_eval_two_track_ordering -- --nocapture
```

## Before you start

* An installed build (a DMG, not `npm run tauri dev`) that actually STARTS a
  two-track meeting — see the note at the top; every merged piece is present in
  a build from `main`, and nothing calls them.
* macOS 14.4 or later. Below that the system-audio track is refused by design
  and this demo is not runnable — the mic-only meeting still is, which is 22-A's
  demo, not this one.
* System Audio Recording granted, given through Settings BEFORE the call starts
  (YV102). Granting it mid-call is a legitimate second take; it is not this one.
* Models already downloaded, and one prior dictation run so the VAD cache exists.
* A real call with at least one other person on it — Zoom, Google Meet or Teams.
  A recording of a call is not a substitute: the point is that the tap picks up
  what the OTHER end is playing out of this Mac's output device.
* **Bluetooth output on, if you have AirPods.** That is the configuration OS-2's
  evidence names, and the one where the two clocks are furthest apart.
* Screen recording on with the menu bar visible — the Wi-Fi state has to be in
  frame.

## The script

1. **Show the tap is granted.** Settings → the system-audio step, on camera,
   showing granted rather than "not determined".
2. **Join the call** and let the other person talk first, so the first thing on
   the recording is Them and not Me.
3. **Start the meeting** with ⌃⌘M. Note on camera that the pill shows recording.
4. **Hold a real back-and-forth for at least ten minutes**, with at least three
   places where you talk over each other — those are the moments the merge is
   for, and the ones a reader will check first.
5. **Change the output device mid-call** (AirPods in or out) around minute six.
   The tap's aggregate device is rebuilt (YV103/YV104); the recording must not
   stop and must not gap. Say the time out loud when you do it so it can be
   found in the transcript.
6. **Turn Wi-Fi off** — after the call ends, before stopping the meeting. Leave
   the indicator in frame for the rest of the recording.
7. **Stop the meeting.** Watch `transcribing` → `summarizing` → `complete`.
8. **Read the result on camera:** one interleaved transcript, Me and Them
   alternating in the order things were said, with the overlapping exchanges
   from step 4 in the right order.
9. **Export to Markdown** and open the file: the same order, the same labels.
10. **Only then, turn Wi-Fi back on.**

## What counts as a pass

* The meeting is `complete`, and `sys_wav_path` is set — there really are two
  tracks, not one track and a hopeful label.
* The transcript is ONE list, interleaved, not two blocks.
* Every one of the step-4 overlaps reads in the order it happened. This is the
  claim; if a reader has to reconstruct who answered whom, the phase has not
  closed.
* The device change at step 5 left no gap and no seam of duplicated text.
* The exported Markdown reads the same way the screen did.
* Wi-Fi was off, on camera, for steps 7–9.

## What counts as a fail worth stopping the phase for

Any of: a "Them" line that is obviously the user's own voice (the tap picked up
the mic through the speakers is a real thing — say so rather than shipping it);
Me and Them visibly out of order across an overlap; a tap that delivered only
silence for the whole call (that is matrix row #1/#2 or OS-4's all-zero bug, and
the watchdog's log in `tap_rebuilds` says which); a meeting where the merge put
the two tracks minutes apart.

## Results

Fill this in when the demo is run; link the recording.

| Date | Build | App tapped | Duration | Output device change | Overlaps in order | Recording |
|---|---|---|---|---|---|---|
| _not yet run — every PR has landed; waiting on a production caller that starts the tap for a meeting_ | | | | | | |
