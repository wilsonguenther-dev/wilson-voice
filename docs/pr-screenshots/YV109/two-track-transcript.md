# Fixture (d), decoded and rendered — the human-facing artifact

YV109 changes no UI, so there is no screenshot to take. This is the equivalent:
the marker lines of the transcript the eval gate produced, printed by
`meeting_eval_two_track_ordering_survives_the_clock_mismatch` on 2026-08-15.

Two wavs recorded by two synthetic devices with different crystals and a 750 ms
start offset, decoded independently by the real Parakeet model through the
shipped windowed chunker and the shipped timed seam merge, put back onto one
clock from the journal's own index records, and rendered by
`meetings::render_transcript`:

```
two-track-ordering track 0: measured rate 15999.3600 Hz (-40.0 ppm, declared -40.0), origin -0.0099 s (declared +0.000)
two-track-ordering track 1: measured rate 16004.0000 Hz (+250.0 ppm, declared +250.0), origin +0.7301 s (declared +0.750)
meeting_eval two-track-ordering residual_ms_at_3h mic=9.9 system=19.9 cross_track=10.0
two-track-ordering: residual at the simulated 3 h mark — mic 9.9 ms, system 19.9 ms, CROSS-TRACK 10.0 ms (budget 50 ms)
two-track-ordering: nominal-rate control drifts 3121 ms
two-track-ordering: 13 markers, in spoken order
    [00:00:03] Me: avocado,
    [00:00:07] Them: bramble
    [00:00:12] Me: kettle,
    [00:00:12] Them: custard
    [00:00:18] Them: harpoon
    [00:00:23] Me: marigold,
    [00:00:36] Me: penguin
    [00:00:36] Them: meadow,
    [00:00:44] Them: walrus,
    [00:00:50] Me: sandal
    [00:00:50] Them: turnip,
    [00:01:08] Me: violin,
    [00:01:08] Them: tundra,
```

**The four pairs that share a displayed second are the whole point.** `kettle`
and `custard` were spoken 400 ms apart; `penguin`/`meadow` 500 ms; `sandal`/
`turnip` 300 ms; `violin`/`tundra` 600 ms. All four are Me-first and all four are
closer together than the 750 ms start offset, so a transcript that compared the
two tracks' own clocks — the only thing anything before 22-B could do — prints
every one of them the other way round. That is asserted, not asserted-about:
`two_track_ordering_fixture_is_hard_by_construction` runs the un-rebased render
and requires exactly eight displaced rows.

The `hh:mm:ss` stamps are the shipped `format_offset`, which truncates to the
second, which is why the pairs share a stamp on screen while being ordered
correctly underneath — the sort is on `start_seconds`, not on the string.

The trailing commas are the decoder's punctuation on the carrier sentence ("The
next word is avocado, spoken once."), reproduced verbatim: the gate normalises
before comparing words and prints before normalising, so what is shown here is
what the model actually returned.
