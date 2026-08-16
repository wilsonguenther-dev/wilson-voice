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

Captured with headless Chrome at 2× device scale against the built bundle
(`http://localhost:<port>/dev/meeting-transcript-preview.html#<scene>`), macOS
26.5.2 / arm64.

## What each screenshot shows

**`mic-only.png` — the caveat, where clustering is the mechanism.** Three
microphone lines, each labelled `Speaker` (YV125's un-clustered label), and under
them, aligned to the text column rather than to the timestamp gutter:
*"Speech during overlapping talk is attributed to only one speaker."*

**`blank-tap.png` — the case a `kind` alone would get wrong.** The user said this
was a call. The tap attached and recorded nothing but silence, so the microphone
carried whatever was in the room, `is_two_track` is false, the branch is
`ClusterTrackA`, and the caveat appears. A gate written against `kind == "virtual"`
instead of against the diarization target would have withheld it here.

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
