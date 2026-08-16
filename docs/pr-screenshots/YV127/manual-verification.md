# YV127 — manual verification

> **Acceptance criterion 2 (manual):** "the caveat sentence renders in the
> Meetings detail UI for a `full_clustering`-mode meeting and does **not** render
> for a `virtual`+tap meeting where Track A never went through clustering (the
> caveat only applies where clustering ran)."

Verified against the **shipping component**, not a mock. `dev/meeting-transcript-preview.html`
(YV108's dev-tooling entry, `YAP_DEV_TOOLING=1 npm run build`) mounts the real
`<TranscriptList>` inside the app's own chrome, so a screenshot of it is a
screenshot of the Meetings detail. Its three existing scenes happen to be exactly
the two sides of this criterion, which is why no fourth scene was added:

| screenshot | scene | meeting | diarization target | caveat |
|---|---|---|---|---|
| `mic-only.png` | `#mic-only` | `kind = unknown`, one recorded track | `ClusterTrackA` | **shown** |
| `blank-tap.png` | `#blank-tap` | `kind = virtual`, tap recorded only silence | `ClusterTrackA` | **shown** |
| `two-track.png` | `#two-track` | `kind = virtual`, tap recorded the far side | `MicIsMe` | **not shown** |

The `meeting` column is **read off the picture, not off this caption**: the
harness strip at the bottom right of every screenshot prints `kind=<the value
the component was handed>` next to the scene buttons. That readout exists
because of a defect in the first version of this document — it captioned
`blank-tap.png` as `kind = virtual` while the preview was passing `unknown`
(`kind={twoTrack ? "virtual" : "unknown"}`), which made the row describe a
meeting no screenshot had been taken of. The preview now passes the kind the
scene means (`scene === "mic-only" ? "unknown" : "virtual"`), and the value is
in the frame so a caption can never drift from it again.

Captured with headless Chrome at 2× device scale against the built bundle
(`http://localhost:<port>/dev/meeting-transcript-preview.html#<scene>`), macOS
26.5.2 / arm64.

## What each screenshot shows

**`mic-only.png` — the caveat, where clustering is the mechanism.** Three
microphone lines, each labelled `Speaker` (YV125's un-clustered label), and under
them, aligned to the text column rather than to the timestamp gutter:
*"Speech during overlapping talk is attributed to only one speaker."*

**`blank-tap.png` — the case a `kind` alone would get wrong.** The user said this
was a call — the harness strip reads `kind=virtual`. The tap attached and
recorded nothing but silence, so the microphone carried whatever was in the
room, `is_two_track` is false, the branch is `ClusterTrackA`, and the caveat
appears. A gate written against `kind == "virtual"` instead of against the
diarization target would have withheld it here.

That last sentence is a claim about a counterfactual, so it was **run**, not
asserted. With `showsOverlapCaveat`'s first line replaced by the naive gate
(`if (kind === "virtual") return false;`) and everything else untouched, the
`#blank-tap` screenshot loses the caveat — sha256 `45f17c49…`, against
`f74656d1…` for the shipped one. Under the *pre-fix* preview (the same naive
gate, but `blank-tap` still rendering at `kind = "unknown"`) the screenshot
came back **byte-identical to the correct one**, `f74656d1…`: the old picture
could not see the bug it was cited as evidence against. Full transcript in
`non-vacuous-mutations.txt` (M10, M10b).

The same case is now pinned by a test rather than only by a picture — a row in
`describe("showsOverlapCaveat")`'s truth table with `kind = "virtual"` and two
`SYSTEM_TRACK` spans carrying the same whitespace the preview's `BLANK_TAP`
uses (`"   "`, `"\n\t "`), asserting `true`. The truth table's other `virtual →
true` row has no second-track rows at all, which is a different input; M11 in
the mutation record shows the new row is the only one in that table that
catches an `isTwoTrack` that stops ignoring blank spans.

**`two-track.png` — the exception.** `Me` / `Them` interleaved, because the other
participants really were recorded on their own track. Track A was never clustered,
so there is no attribution decision for a caveat to qualify, and there is no
caveat. This is the "does not render" half of the criterion.

## What this does NOT show

Clustering. YV126 is not on `main` at the time of this item (PR #141 open), so
every microphone line in `mic-only.png` and `blank-tap.png` reads `Speaker` — one
label for the whole track, not one per person. The caveat is honest under that
state because it states a limit on attribution and claims no ability to separate
voices; it does not become true when YV126 lands, it becomes load-bearing.
