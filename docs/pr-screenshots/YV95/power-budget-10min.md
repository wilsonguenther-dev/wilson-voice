# YV95 — the OS-12 power budget for recording-with-pill-visible

OS-12's fourth clause is the only acceptance criterion in this item that needs a
machine, a wall clock and root:

> **Acceptance:** a `powermetrics`-measured package-power delta for 10 minutes of
> recording-with-pill-visible stays under a written ceiling, recorded in this
> doc. Without a number this finding is just an opinion.

## What was NOT run

**The `powermetrics` run was not performed for this item, and no measured
milliwatt figure is claimed anywhere in this PR.** `powermetrics` requires root
(`sudo powermetrics …` — it reads the SMC and the CPU power counters), and the
harness that produced this branch runs non-interactively with no way to
authenticate a sudo prompt:

```
$ sudo -n powermetrics --samplers cpu_power -i 1000 -n 1
sudo: a password is required
```

The measurement also needs the *shipped app*, signed and launched with
Microphone permission granted, holding a real meeting for ten minutes with the
pill on screen — i.e. it belongs with YV99's phase-closing on-camera demo, where
a recording engine exists and a real meeting can be run end to end. Same posture
as YV91's 60-minute idle repro (`docs/pr-screenshots/YV91/manual-repro-60min-idle.md`):
the claim is never stronger than the evidence.

## What WAS measured, and is a test

The cost OS-12 identified is not the audio path — a 48 kHz stereo tap is
384 KB/s and the disk write is 64 KB/s, both negligible. It is the **pill's watch
threads and its paint loop, held for up to three hours**. Those are countable,
and the count is asserted in `tests/pill_idle_tick_during_recording.rs`:

| Over a 3-hour meeting | Before | After | Asserted by |
|---|---|---|---|
| hover-watch cursor tests | 144,000 | 10,800 | `a_three_hour_meeting_costs_an_order_of_magnitude_fewer_polls` |
| `run_on_main_thread` hops from the hover watch | 144,000 | 10,800 | same |
| JS-thread wakes for the elapsed clock | 648,000 (a 60 fps canvas clock) | 10,800 (one 1 Hz emit) | `the_shipped_tick_is_one_hertz` |
| recording-pulse frames costing JS | 648,000 (a canvas pulse) | 0 (CSS compositor animation) | markup: the badge is DOM, `float.css` `@keyframes rec-pulse` |
| **Yappy pill: ambient canvas redraws** | **108,000** (10 fps × 3 h) | **0** (canvas parked) | `a_meeting_parks_the_canvas_loop` (`src/pill/live.test.ts`) |

`a_three_hour_meeting_costs_an_order_of_magnitude_fewer_polls` fails if the first
column ever comes back.

**The last row was missing from this table when the item was first written, and
it was the largest number in it.** `framePlan` parked the canvas on
`hidden || reduceMotion` only. A meeting deliberately SHOWS the pill, so a
settled Yappy fell through to the 10 fps ambient tick — a full canvas redraw, a
spring update and a bubble reposition every 100 ms — for the entire meeting,
while the module docs claimed the loop "can stay parked through a three-hour
meeting". The DOM overlay was necessary but not sufficient: an overlay that costs
nothing does not park a loop still redrawing behind it. `meetingRecording` is now
a `framePlan` input and parks it outright once the scene has settled (gated on
`settled`, unlike `hidden`, because this pill is in view — a frame frozen
mid-capsule-close would stay frozen for the whole meeting). A live take during a
meeting is unaffected: the live branch is answered before anything can park.

The default `pill_style` is `classic`, which already settles-and-parks its own
rAF at rest, so this only ever hit users on the Yappy style — a shipped,
user-selectable style, i.e. not a hypothetical.

## The ceiling to gate against

**Proposed ceiling: ≤ 150 mW package-power delta over the same machine idle,
averaged across a 10-minute recording with the pill visible on battery.**

This is a *target*, written down so the first real run has something to pass or
fail — not a measured result. For reference, the machine class OS-12 names (a
fanless M1 Air) idles around 60–100 mW package power with the display on, and a
single P-core at 1% is roughly 30–50 mW; a recorder that costs more than about
one and a half percent of a core with its indicator on screen has regressed into
exactly the state YV81 was written to prevent.

## The exact procedure, for whoever runs it

```bash
# 1. Launch the signed build, grant Microphone, leave the pill visible.
#    Unplug: the finding is about battery, and package power on AC is a
#    different number.

# 2. Baseline — the same machine, same apps, no meeting, 10 minutes:
sudo powermetrics --samplers cpu_power -i 5000 -n 120 \
  | awk '/Combined Power/ {print $NF}' > /tmp/yap-idle.txt

# 3. Start a meeting (⌃⌘M), leave the cursor OFF the pill, and repeat:
sudo powermetrics --samplers cpu_power -i 5000 -n 120 \
  | awk '/Combined Power/ {print $NF}' > /tmp/yap-recording.txt

# 4. The delta is the number that goes in the table below.
python3 - <<'PY'
idle = [float(x) for x in open('/tmp/yap-idle.txt')]
rec  = [float(x) for x in open('/tmp/yap-recording.txt')]
print(f"idle {sum(idle)/len(idle):.0f} mW, recording {sum(rec)/len(rec):.0f} mW, "
      f"delta {sum(rec)/len(rec) - sum(idle)/len(idle):.0f} mW")
PY
```

Do NOT move the cursor onto the pill during the run: hovering is the state that
legitimately steps the watch back up to 75 ms, and a run with the mouse parked on
the capsule measures a different thing.

**Run it once per pill style.** The two styles have different paint loops —
classic is a CSS variable driven by a settle-and-park rAF, Yappy is a canvas
scene — so a single number cannot speak for both, and the Yappy row is the one
the canvas park above is meant to move.

| Date | Machine | macOS | Pill style | Idle (mW) | Recording (mW) | Delta (mW) | Under 150? |
|---|---|---|---|---|---|---|---|
| _not yet run_ | | | classic | | | | |
| _not yet run_ | | | yappy | | | | |
