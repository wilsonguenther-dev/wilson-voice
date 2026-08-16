# YV131 — AS-norm: the tuning transcript

Every number here was measured on this pipeline, with the model the app actually
downloads. None is quoted from a paper or a vendor post. The raw per-trial
scores are committed beside this file in `yap23-asnorm-measurement.json`, and
`cargo test --test as_norm_cross_condition_measured` recomputes the headline
figures through YV120's `enrollment_eer` rather than trusting this document.

## Headline

Held out on LibriSpeech `dev-other` — 33 speakers the design was never tuned
against — through a simulated headset-to-laptop channel change:

| | raw cosine | AS-norm | change |
|---|---|---|---|
| **Equal error rate** | **15.15%** | **12.12%** | **−20.0% relative** |

AS-norm is ahead in all ten measured split × channel cells.

## The setup

| | |
|---|---|
| Embedder | `wespeaker_en_voxceleb_CAM++`, sha256 `c46fad10…`, the catalog.json entry the app downloads |
| Extractor | `sherpa-onnx 1.13.4` — the exact pin `desktop/yap-diarize/Cargo.toml` carries |
| Embedding width | **512**, measured. The plan guessed 192; YV122 measured 512 and this agrees |
| Cohort | LibriSpeech `test-clean`, 40 speakers × 2 conditions = 80 entries, 163,840 bytes |
| Design chosen on | LibriSpeech `dev-clean`, 40 speakers |
| Design reported on | LibriSpeech `dev-other`, 33 speakers |
| Segment length | 30 s enrollment, 30 s test — YV126's cluster floor |
| Licence | CC-BY-4.0. Only 512-float embeddings ship; no audio is redistributed |

The three speaker sets are disjoint. That is the point: a cohort sharing
speakers with the targets is not measuring what it claims to, and a design
chosen and reported on one split is not measured, it is fitted.

## The channel ladder

"Cross-device" is not one thing, so a claim about it is meaningless without the
severity attached. All four B channels are **simulated** filters applied to real
speech — defined in `scripts/yv131-build-impostor-cohort.py::CHANNELS`. No
AirPods were recorded and no volunteer was asked to speak.

| Channel | dev-clean raw → AS-norm | dev-other (held out) raw → AS-norm |
|---|---|---|
| A clean (control) | 3.75% → **2.66%** | 6.01% → **4.55%** |
| B1 headset→laptop, mild | 12.50% → **10.27%** | 15.15% → **12.12%** |
| B2 across-desk, moderate | 21.25% → **19.95%** | 23.96% → **22.73%** |
| B3 far-field room | 30.99% → **27.50%** | 32.17% → **31.96%** |
| B4 telephone band | 31.09% → **28.89%** | 35.18% → **35.09%** |

Read the bottom two rows as carefully as the top. **The benefit shrinks to
almost nothing once the channel is destructive** — 0.7% and 0.3% relative on the
held-out split. AS-norm corrects for a condition shift; it cannot recover
identity information the channel has removed. B1 is the case Wilson asked about
and the one this item is justified by. B3/B4 are published so the reader can see
where the technique stops helping.

## What did not work, and is not shipped

**A synthetic cohort from macOS `say` voices.** Built first, then rejected on
measurement. CAM++ cannot separate synthetic voices at all:

| Cohort/target material | clean same-condition EER |
|---|---|
| macOS `say`, 5 held-out English voices | **35.6%** |
| LibriSpeech, real speakers, identical harness | **2.2%** |

Two different `say` voices score cosine **0.64** where two different humans score
0.51. Apple's Siri-era voices (Eddy / Reed / Rocko / Grandpa / Grandma / Shelley)
embed at **0.91–0.997** to each other across their US and UK variants — they are
variations on one synthetic speaker, not different speakers, so six of them in a
cohort is one voice with six votes. A cohort has to model "some stranger", and
TTS output does not land where strangers land.

**A design chosen and reported on the same split.** The first pass reported
+15.3% relative on `dev-clean` with K=40. Scored on a held-out split for the
first time, the same design came out at **−3%** — worse than raw. That result is
why the tune/report split exists in this item at all, why
`the_tune_and_report_splits_are_different_corpora` is a test, and why the K that
ships is 20 rather than the number that looked best on the tuning split.

## Two things this measurement taught the implementation

**1. Only the enrollment side can reorder a ranking.** Within one cluster every
candidate is scored against the *same* test embedding, so the test-side
normalization term is identical across them — a monotone affine map of the raw
score, which cannot change who wins. Candidate order is changed only by the
enrollment-side term, which corrects *hubness*: a profile enrolled on the same
device as the recording sits close to every stranger recorded on that device,
and scores high for reasons that have nothing to do with being the right person.
Both sides still ship — the test side is what keeps scores comparable across
clusters and meetings — but the acceptance test is built on the half that
actually does the work.

**2. CAM++ embeddings below ~20 s are unstable in a way that is not monotone in
duration.** Measured on `dev-clean`, cosine of a segment to its own speaker's
reference:

| segment | 10 s | 13 s | 14 s | 15 s | 16 s | 17 s | 20 s | 30 s |
|---|---|---|---|---|---|---|---|---|
| cos to reference | 0.93 | 0.64 | 0.94 | 0.66 | 0.95 | 0.72 | 0.98 | 0.97 |

The same audio, cut at different lengths, swings between 0.64 and 0.98 — this is
content-dependent, not a smooth duration effect (the extractor was confirmed
deterministic and stateless first). Everything here is therefore measured at
30 s, which is also YV126's cluster floor. Tuning a normalization on top of
embeddings that unstable would have been tuning the window, not the voice.

## Reproducing

```bash
pip install 'sherpa-onnx==1.13.4' numpy soundfile
curl -O https://www.openslr.org/resources/12/{dev-clean,dev-other,test-clean}.tar.gz
# CAM++ from the catalog.json mirror, digest-checked by the script itself
python3 scripts/yv131-build-impostor-cohort.py \
    --model /path/to/wespeaker_en_voxceleb_CAM++.onnx \
    --librispeech /path/to/LibriSpeech
cd desktop/src-tauri && cargo test --test as_norm_cross_condition_measured -- --nocapture
```

## Still not measured

A **real** cross-device recording — one human, one laptop microphone, one pair of
AirPods — has not been run. Every B channel above is a filter, and a filter is a
model of a device change, not a device change. That remains the honest gap in
this item, and it is the one thing that would turn "the mechanism helps under a
simulated shift" into "the mechanism helps Wilson". It needs hardware and a
person, neither of which a test binary has.
