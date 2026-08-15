# YV108 — manual verification

The backlog's manual criterion: *"opening a real two-track meeting in the
Meetings UI shows a legible back-and-forth, not two disjoint blocks of text."*

## What was actually run, and what was not

**Not run: a real tapped meeting.** A two-track meeting needs the CoreAudio
process tap running under a granted `NSAudioCaptureUsageDescription` TCC
authorisation, on a signed app bundle, against a live call — none of which
exists in this branch's environment, and none of which CI can produce either. I
did not record one, and I am not going to describe one I did not record. The
automated gate is therefore the whole gate for this item, which is exactly the
posture this backlog specifies for hardware-tap work.

**Run: the shipping component, against both shapes, in a real browser.** The
transcript list is a component (`src/meetings/TranscriptList.tsx`) used by the
Meetings detail in `App.tsx` and by a dev-tooling preview entry, so the two
screenshots beside this file are the shipping markup and the shipping CSS, not a
mock-up of them:

```sh
cd desktop
YAP_DEV_TOOLING=1 npm run build          # adds dist/dev/meeting-transcript-preview.html
npx vite preview --port 4319
# then, headless Chrome, 1400x900:
#   http://localhost:4319/dev/meeting-transcript-preview.html#two-track
#   http://localhost:4319/dev/meeting-transcript-preview.html#mic-only
```

* `two-track-transcript.png` — six turns, Me and Them alternating down ONE
  column of text, timestamps in the left gutter, the speaker in the second. The
  segments are handed to the component **grouped by track** (all the mic's, then
  all the tap's), the shape two transcription passes produce, so the alternation
  in the screenshot is the renderer's ordering and not the input's.
* `mic-only-transcript.png` — a 22-A meeting: every line "Me", the narrower
  speaker gutter it has always had, no "Them" anywhere on the screen.

**Run: the export, end to end through SQLite.** `two-track-export.md` is the
file `Database::meeting_markdown` produced for a two-track meeting on this
branch — captured from the test, not typed by hand. It is the same interleave,
in the same order, as the screenshot.

## What is still owed, and by which item

YV107 (host-time cross-track merge) is not on `main` yet, so today nothing in the
shipped pipeline writes a `track = 1` segment. That does not weaken this item:
YV108 renders whatever ordered, track-attributed segments it is given, and the
gate above feeds it exactly the sequence YV107 is specified to produce. The
end-to-end proof — real capture through real merge through this render, plus a
camera-documented run against a real virtual meeting — is YV109's job, and it is
the item that closes the phase.
