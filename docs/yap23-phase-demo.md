# yap23 — phase-closing demo: IRL fixture → named-speaker transcript

**Item:** YV134. **Purpose:** the one place the whole diarization chain is
stated end to end, with every number that has been measured and every number
that has not — and with the honest answer to "does yap23 close?".

The phase's stated acceptance, verbatim from the backlog: *a synthetic IRL
(in-person, single mic, multi-speaker) fixture runs the complete chain — capture
→ `kind='in_person'` branch → full clustering → rank+floor → enrollment
match/new-voice prompt → correction if needed → speaker-attributed transcript →
evidence-linked, speaker-attributed next actions — and produces a transcript with
every segment correctly attributed to a named speaker.* It mirrors
`yap22a-phase-demo.md` and `yap22b-phase-demo.md` rather than inventing a
different bar.

---

## Status: yap23 does NOT close yet, and this is why

Read this before anything else in the file.

The chain above has ten stages. **Six of the backlog's fifteen items are merged
on `main`; the six that produce a speaker's identity are not.** There is no
inference backend in `yap-diarize` (YV122), so there is no clustering and no
embedding; there is no `speaker_profiles` table (YV128), so there is no
enrollment; there is no matcher (YV129) and no correction UX (YV130). **The
mechanism that turns a voice into a name does not exist in this repository
today.** No number in this file, and no test in this PR, should be read as
saying otherwise.

That is not a document's promise — it is enforced.
`cargo test --test meeting_eval yap23_phase_close_names_the_stages_that_are_not_shipped`
asserts each missing stage absent against the mechanism (the sidecar's declared
dependencies, the DDL `db::migrate` actually runs, and the type of the only
`SpeakerSource` the app can construct). The day a stage lands, that test goes red
and names what has to change here.

| Stage | Item | Merged? | What exists today |
|---|---|---|---|
| Eval harness (DER/JER/EER, RTTM fixtures) | YV120 | **yes** (#135) | `diarize_metrics.rs`, fixtures (e)/(f), all gates `None` |
| Sidecar scaffold, wire protocol, pool | YV121 | **yes** (#136) | `yap-diarize` + `diarize.rs`, zero model bytes |
| sherpa-onnx, pyannote-seg-3.0 + CAM++ | YV122 | **no** | `load_backend` returns `no_backend` unconditionally |
| Model vendoring, archive extraction, supply chain | YV123 | **yes** (#138) | `DiarizeCatalogModel`, pinned digests, `extract_tar_bz2` |
| Anti-alias EER validation arm | YV124 | **no** | needs embeddings, which need YV122 |
| `meetings.kind` + IRL-first branch | YV125 | **yes** (#140) | migration 4, `diarization_target`, `UNCLUSTERED_SPEAKER_LABEL` |
| Clustering + rank/floor | YV126 | **no** | no `cluster_index` column, no clustering call |
| Overlap honesty | YV127 | **no** | — |
| `speaker_profiles` schema | YV128 | **no** | schema stops at migration 4 |
| Enrollment matching | YV129 | **no** | — |
| Correction UX | YV130 | **no** | — |
| Cross-device drift | YV131 | **no** | — |
| Matrix rows 7, 8, 13 | YV132 | **yes** (#147) | all three rows say the app cannot reach them |
| Evidence-linked speaker-aware actions | YV133 | **yes** (#148) | `SegmentSpeaker`, `SpeakerSource`, `SummaryItem::speaker` |
| Phase-closing E2E | YV134 | **this PR** | fixture (g) + the E2E below + this file |

---

## What YV134 does ship

### 1. Fixture (g) — `irl-close-4-enrolled`

Built the way fixtures (e) and (f) were (YV120): synthetic `say` audio, mundane
invented sentences with no names/digits/addresses, hand-assembled so the RTTM
ground truth is **exact by construction** rather than annotated. It lives with
the rest of the corpus at `~/yap-eval-corpus/meetings/`, outside the repo, hashed
into `tests/fixtures/meeting_eval_manifest.json` + `.sha256`.

What makes it fixture (g) rather than a second fixture (e) is enrollment ground
truth:

| | value |
|---|---|
| speakers | 4 (`spk_a` … `spk_d`) |
| turns | 16, never the same voice twice in a row |
| duration | 70.6 s |
| overlap | **none** — inside pyannote-seg-3.0's ceiling, on purpose |
| peak speakers / 10 s window | 3 (at the ceiling, not over it) |
| enrolled | 3 — `spk_a` "Avery", `spk_b` "Bao", `spk_c` "Cleo" |
| **not** enrolled | 1 — `spk_d`, no name and no clip |
| enrollment clips | `enroll-spk_a.wav` 5.6 s, `enroll-spk_b.wav` 6.8 s, `enroll-spk_c.wav` 7.7 s |

Two design points that are load-bearing rather than decoration:

* **The enrollment clips are HELD OUT.** No sentence in them appears in any turn
  of the meeting, and the E2E decodes each clip and each meeting line and asserts
  non-containment both ways. Enrolling on audio that is also in the meeting makes
  the match a lookup of a clip against itself — that scores a memcmp, not a
  speaker embedding, and would report an EER near zero on a model that had
  learned nothing.
* **One voice is deliberately unenrolled.** A closing fixture where everyone is
  known cannot fail a matcher that says yes to everyone, which is exactly the
  behaviour a loosely-tuned enrollment threshold produces.

Unlike fixture (f), (g) is deliberately *inside* the mechanism ceiling. A
phase-closing demo has to run the chain the app is meant to run; a demo staged on
the case the segmentation model is known to fail would be measuring the ceiling
and calling it the feature. Fixture (f) still exists to measure that ceiling.

### 2. The phase-closing E2E

```sh
cargo test --test meeting_eval yap23_e2e_irl_named_speaker_transcript -- --nocapture
```

It drives the chain in two halves, and the seam between them is the point of the
test rather than a hole in it.

**Upstream — real, shipped, exercised.**
Fixture (g)'s audio is decoded by the **shipped ASR** (Parakeet Unified EN 0.6B,
through the same `--transcribe-file` path every other corpus gate uses), one turn
at a time, into real `MeetingSegment` rows. The shipped `diarization_target`
takes the `in_person` branch. The shipped `render_transcript` labels the result.
The **shipped diarization sidecar is spawned and asked to diarize the fixture**,
and its refusal (`no_models`) is asserted — which is what proves no clustering
ran, so nothing downstream can be mistaken for a diarizer's output.

**Downstream — real, shipped, driven from ground truth.**
Fixture (g)'s own RTTM plus its `enrolled_name` metadata stand in for YV126 +
YV129 — a perfect diarizer and a perfect matcher — and the result is pushed
through the shipped `summarize_segments_with` → `SummaryItem::speaker` rail YV133
built. That half is entirely real code; only its *input* is staged, and the
staging is one function (`support::EnrolledSpeakers::with_new_voice`) which
YV129's matcher replaces.

The four criteria are asserted against the downstream half:

| # | criterion | how it is asserted |
|---|---|---|
| (a) | every segment carries a non-generic label | every line is `Named(_)` or the explicit `New — unnamed` state; never `Speaker` |
| (b) | pre-enrolled voices are correctly resolved | each turn's label equals its own profile — **and**, independently, the labelling is a bijection over the four voices, so two people collapsed onto one profile fails even though every label is a real name |
| (c) | the deliberately-new voice is flagged new | `spk_d` is `Anonymous("New — unnamed")`, never `Named` |
| (d) | an evidence-linked action carries a resolved name | `SummaryItem::speaker` is a name from enrollment, resolved through the evidence id the model cited |

`New — unnamed` is a `SegmentSpeaker::Anonymous` label, never `Named`. The
difference from a bare `Speaker N` is that it is attached to **one** voice the
enrolled ones were told apart from, which is what makes "name this person" a
button rather than a wish.

**Read the `Named`/`Anonymous` split correctly before building on it.** It is a
type-level seam that the shipped summarize rail *flattens*, not a boundary
anything enforces. `summarize.rs` builds its evidence table with an
unconditional `l.speaker.as_ref().map(|s| s.label().to_string())` — there is no
`is_named()` filter on that path — and `SummaryItem::speaker` is a bare
`Option<String>`. An `Anonymous("New — unnamed")` label therefore lands in an
action item's owner field byte-identically to a `Named` label of the same text
and renders the same way. What keeps it from reading as somebody's name is the
**string content**, not the variant; the proof is in this item's own E2E, whose
criterion-(d) loop has to allow `owner == NEW_VOICE_LABEL` explicitly to pass.
Returning `Anonymous` is still the honest thing for a speaker source to do — it
must never claim an identity it does not have — but what it buys today is a
readable contract, not a guard.

**TODO(YV129/YV130).** The enforcement is owed by whichever of those items first
makes the unmatched-voice state user-visible: either filter the evidence table
on `is_named()`, or give `SummaryItem::speaker` the enum instead of a `String`.
YV134 deliberately does not do it — it would change YV133's shipped behaviour,
which is outside this item's scope.

**With the corpus absent** — CI, a fresh clone — the test prints
`meeting eval corpus not found at ~/yap-eval-corpus/meetings, skipping` and
passes, the same posture every corpus-gated test in the file has had since YV90.

---

## The numbers, measured and unmeasured, in one place

This is the section the backlog's eval-first sequencing exists to produce. **A
cell that says `unmeasured` is not an omission — it is the point.** Every
threshold in this epic must be tuned against this harness; none may be copied
from a vendor blog (the plan's own 0.70/0.55 cosine bands and sherpa's 0.5
clustering default are quoted from OpenWhispr and have never been measured on
this pipeline).

### Measured

| metric | fixture | value | where |
|---|---|---|---|
| DER, ground truth vs itself | (e), (f), (g) | 0.0000 | `meeting_eval_diarization_metrics_score_the_real_fixtures` |
| JER, ground truth vs itself | (e), (f), (g) | 0.0000 | same |
| DER, one-cluster baseline | (e) `room-3-near-field` | **0.6384** | same |
| JER, one-cluster baseline | (e) | **0.8795** | same |
| DER, one-cluster baseline | (f) `classroom-6-far-field` | **0.7218** | same |
| JER, one-cluster baseline | (f) | **0.9459** | same |
| DER, one-cluster baseline | (g) `irl-close-4-enrolled` | **0.7273** | same |
| JER, one-cluster baseline | (g) | **0.9318** | same |
| speaker F0 spread | (e) | 103.9 / 170.2 / 216.2 Hz, closest pair 1.270× | `assert_rttm_fits_the_audio` |
| speaker F0 spread | (f) | 77.7 / 105.3 / 170.2 / 210.5 / 250.0 / 296.3 Hz, closest pair 1.185× | same |
| speaker F0 spread | (g) | 170.2 / 103.9 / 210.5 / 70.2 Hz, closest pair **1.237×** | same |
| transcript WER (shipped ASR) | (a) `lecture-15min` | 0.0042 on the shipped VAD-cut/timed arm; gate 0.02 | `meeting_eval_lecture_wer_is_under_the_gate` |

The one-cluster baseline is the number a diarizer has to beat: every reference
turn kept, all relabelled to one speaker. On fixture (e) its DER is derivable on
paper (nothing missed, nothing invented, so the error is exactly the speech
belonging to the two speakers that were not the mapped one) and the test asserts
the measured value equals the derivation — which is what proves the harness is
scoring the fixture rather than a hand-built interval list.

### Unmeasured, with the item that measures it

| metric | fixture | gate constant | measured by |
|---|---|---|---|
| DER under real clustering | (e) | `ROOM_3_DER_GATE = None` | **YV126** |
| JER under real clustering | (e) | `ROOM_3_JER_GATE = None` | **YV126** |
| DER under real clustering | (f) | `CLASSROOM_6_DER_GATE = None` | **YV126** |
| JER under real clustering | (f) | `CLASSROOM_6_JER_GATE = None` | **YV126** |
| DER under real clustering | (g) | `IRL_CLOSE_DER_GATE = None` | **YV126** |
| JER under real clustering | (g) | `IRL_CLOSE_JER_GATE = None` | **YV126** |
| enrollment EER | (g) | `IRL_CLOSE_ENROLLMENT_EER_GATE = None` | **YV124** / **YV129** |
| anti-alias resampler EER delta | (c) `device-change` | — | **YV124** |
| clustering distance threshold | (e) | — | **YV126** (a `CosineDistance`) |
| enrollment match / new-voice bands | (g) | — | **YV129** (a `CosineSimilarity`) |

`meeting_eval_diarization_gates_are_unmeasured_until_there_is_output_to_gate`
asserts every one of those constants is still `None`, so a placeholder cannot be
quietly promoted to a gate: setting one requires editing that test, and editing
it means writing down where the number came from.

Note the phase-closing fixture ships **ungated too**. A closing item is the most
tempting place in a backlog to write a number down so the phase looks finished.
There is still no diarizer, so a number here would be the same guess wearing a
demo's clothes.

---

## The manual, camera-documented take: BLOCKED

YV109 set the bar for this phase's human half: a real recording, on camera, run
through the full pipeline. YV134's equivalent is *a real in-person conversation
(2–3 people, one microphone, a normal room) producing a legible, correctly
speaker-attributed transcript*.

**It cannot be recorded yet, and no substitute is offered.** A camera take is
only worth something if the thing on camera is the shipped mechanism. Recording a
real conversation today would produce a transcript in which every line reads
`Speaker` — which is exactly what YV125 shipped it to say, and is the correct
behaviour, but it is not the demo. Staging it with the ground-truth stand-in the
automated test uses would be a video of a fixture, which is worse than no video.

**Prerequisites, in order:** YV122 (a real backend) → YV123's models installed →
YV124 (the EER checkpoint, before any threshold is tuned) → YV126 (clustering, and
the first measured DER on fixture (e)) → YV128/YV129 (profiles and matching).
YV129 is the first item after which this take is worth filming.

When it is filmed, it belongs here, in the shape `yap22b-phase-demo.md` uses:

| Take | Setup | Expected | Result |
|---|---|---|---|
| 1 | 3 people, one room, built-in mic, `kind = in_person`, 2 pre-enrolled + 1 new | every line named or explicitly new; the new voice prompts | _pending YV129_ |
| 2 | the same recording after correcting one label | the correction persists and re-attributes that voice's other turns | _pending YV130_ |
| 3 | Wi-Fi off for the whole take | identical output; nothing leaves the machine | _pending YV129_ |

---

## How to re-run everything in this file

```sh
# the phase-closing E2E (corpus present)
cargo test --test meeting_eval yap23_e2e_irl_named_speaker_transcript -- --nocapture

# the gate that enforces this file's Status section
cargo test --test meeting_eval yap23_phase_close_names_the_stages_that_are_not_shipped

# every measured number above
cargo test --test meeting_eval -- --nocapture

# regrow fixture (g) and re-hash the corpus (renders with `say`; changes hashes)
cargo test --test meeting_eval meeting_eval_generate_irl_close -- --ignored --nocapture

# check the corpus by hand, without cargo
(cd ~/yap-eval-corpus/meetings && shasum -a 256 -c \
   desktop/src-tauri/tests/fixtures/meeting_eval_manifest.sha256)
```
