# Meeting demo — proving Track B on a real call

**Item:** YV110 · wire `start_system_tap` into the live meeting flow.
**Audience:** whoever is holding the Mac. This script is the half of YV110 that
CI cannot run, and it is written so that someone who was not in the session can
follow it.

## Why this is manual, and what exactly is unproven without it

Everything in the two-track path is tested in CI over the `TapPlatform` seam —
the decision (`syscapture::track_b_plan`), the two-track session, the tap's
format reaching track 1, the drain into the journal, the teardown order at
stop, and the badge a mic-only meeting carries. See
`desktop/src-tauri/tests/meeting_track_b_wiring.rs` and the matrix rows 1/2
tests.

Three things in that path need hardware and a human, and no test in this
repository claims them:

1. **CoreAudio actually delivers.** `AudioHardwareCreateProcessTap` and its
   aggregate device only exist on macOS 14.4+, and the fake platform proves the
   *call order*, never that the OS returns audio.
2. **TCC is keyed to the code-signing identity.** An unsigned `cargo run` build
   may never show the permission alert at all — the failure DGR Labs describes
   as "we wasted half an hour wondering why no dialog appeared". **Run this on a
   signed, notarized build**, or the result means nothing.
3. **The audio is the other people on the call**, not Yap's own output. The tap
   is global-with-self-excluded; only a real call with a real remote voice shows
   that the exclusion works and the exclusion is not inverted.

## Before you start

- macOS **14.4 or later** (check: `sw_vers -productVersion`). On anything older
  this demo is not applicable and the app is expected to record mic-only — that
  is matrix row 12, and step 7 below is how you check it.
- A **signed, notarized** Yap build (`docs/` release flow), not `npm run tauri
  dev`.
- Zoom, Google Meet or Teams, and a second person or a second device to be the
  far end of the call.
- Screen recording on, camera on you. The whole point of this take is that it
  is watchable evidence.

## The script

1. **Reset the permission, so the alert is real.** In a terminal:

   ```
   tccutil reset AudioCapture consulting.drivia.yap
   ```

   (If the bundle id differs on your build, read it from
   `desktop/src-tauri/tauri.conf.json`.) Quit and reopen Yap afterwards.

2. **Run the setup step.** Yap → Settings → **Set up meeting recording**. macOS
   shows its System Audio Recording alert *here*, with Yap's sentence already on
   screen explaining what is about to be asked. Allow it.
   *Pass:* the alert appears during this step and not later. That is YV102's
   whole reason for existing — an alert that arrives mid-Zoom-join steals focus
   and a denial there is permanent.

3. **Join the call** and let the other person talk for a few seconds before you
   start recording, so there is audio the tap could have missed.

4. **Start the meeting** from any entry point — ⌃⌘M, the menu-bar item, the
   pill, or the Meetings tab's button. All four go through the same control
   plane; pick a different one each time you re-run this.
   *Pass:* recording starts with **no second prompt and no extra step**. The
   pill shows the clock and **no "mic only" badge**.

5. **Talk, both ways, for at least 60 seconds.** You speak; they speak; overlap
   once on purpose.
   *Pass:* macOS shows the purple/orange recording indicator in the menu bar for
   the whole meeting.

6. **Stop the meeting**, and open it in the Meetings tab.
   *Pass, all four:*
   - two WAVs exist for the meeting (the row carries `mic_wav_path` **and**
     `sys_wav_path`);
   - the transcript interleaves **Me** and **Them** in the order the
     conversation actually happened (YV108);
   - the meeting has **no** system-audio badge or note;
   - Settings' setup step now reads as granted — the meeting's own
     ever-delivered-a-non-zero-sample discriminator wrote that back.

7. **The honest-degrade half — do not skip it.** Repeat steps 3–6 with the
   permission denied:

   ```
   tccutil reset AudioCapture consulting.drivia.yap
   ```

   then reopen Yap and **decline** the alert in the setup step (or simply never
   run the step).
   *Pass:* the meeting still records. The pill carries the **mic only** badge,
   the recording bar in the main window carries the full sentence naming System
   Settings, exactly one WAV lands, and the meeting is **never aborted**. That
   is matrix row 1.

8. **The mid-meeting loss (optional, and the hardest to stage).** With a
   two-track meeting running, revoke System Audio Recording in System Settings →
   Privacy & Security. Leave the meeting running for **five minutes**.
   *Pass:* the meeting keeps recording the microphone; after the watchdog's
   three-attempt budget the pill's banner changes to say the system-audio track
   stopped and the mic is still recording; the meeting is **never stopped**; the
   finished meeting row carries a `tap_rebuilds` JSON log. That is matrix row 2.
   *Known limitation, by design in YV110:* the rebuild attempts are decided and
   logged, **not executed** — a rebuilt tap rebases its `host_ns` epoch and
   YV107's cross-track merge reads that field, so executing the seven-step
   rebuild is a separate item with a clock-continuity question in it. If the tap
   recovers on its own the watchdog closes the attempt and the meeting carries
   on with both tracks.

## Recording the result

Put the take (or the four screenshots from steps 4, 6, 7 and 8) on the PR, and
say which macOS version and which meeting app were used. A pass that does not
name the OS version is not a pass: this entire feature is gated on 14.4.
