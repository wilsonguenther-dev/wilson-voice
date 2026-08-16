# YV131 — AS-norm: the tuning transcript

Every number here was measured on this pipeline, with the model the app actually
downloads. None is quoted from a paper or a vendor post. The **primitives** every
trial was computed from are committed beside this file in
`yap23-asnorm-measurement.json` — speaker ids, per-side cohort statistics, raw
cosines — and `cargo test --test as_norm_cross_condition_measured` recomputes
every figure below by pushing them back through the **shipped**
`speaker_asnorm::as_norm_score` and YV120's `eer_sweep`. Change the formula and
these numbers move and the tests go red.

That is a deliberate correction. The first version of this document committed
finished per-trial *scores*, which meant the shipped arithmetic could be changed
— a whole term deleted from it — with every test still green, because the
published numbers had been computed by a Python script that still had the old
one.

## Headline

Held out on LibriSpeech `dev-other` **+** `test-other` — 66 speakers the design
was never tuned against — through a simulated headset-to-laptop channel change:

| | raw cosine | AS-norm | change |
|---|---|---|---|
| **Equal error rate** | **18.20%** | **14.06%** | **−4.14 pp (−22.7% relative)** |

**Speaker-level bootstrap 95% CI on the reduction: [0.25, 7.99] pp**
(2,000 resamples of the 66 speakers, P(Δ ≤ 0) = 1.9%). The interval excludes
zero. It is wide, and it is the honest width: the independent unit here is the
speaker, not the trial, and a trial-level interval on the same data would be
narrower and wrong.

Two report subsets rather than one for exactly that reason. On `dev-other` alone
— 33 speakers — the same measurement gave a 3.57 pp reduction with a 95% CI of
**[−1.01, +8.79] pp**: a real point estimate the arm was too small to separate
from zero. The response to a wide interval is a bigger arm, not a rounder
number, so LibriSpeech's second held-out "other" subset was pooled in.

**And the shipped band's operating point on that arm: FAR 8.81%, FRR 18.16%.**
An EER is a property of a distribution; a shipped band is a property of a
decision, and this item ships one, so both are published.

## What ships

| | | chosen on |
|---|---|---|
| Cohort | LibriSpeech `test-clean`, 40 speakers × 2 conditions, **78 entries after the distinctness gate**, 159,744 bytes | design sweep |
| Normalization | **enrollment-side only** — `(s − μₑ) / σₑ` | design sweep |
| K | **40** | design sweep |
| Distinctness gate | **0.9335** cosine | q0.999 of the `dev-clean` cross-speaker distribution |
| Admission band | **1.2229** in AS-norm units | equal-error crossing on `dev-clean` \| B1 |

Every one of those five lives in the cohort manifest
(`desktop/src-tauri/assets/yv131-impostor-cohort.json`) with the split and the
rule that produced it, not in the source. `cargo test --test
as_norm_bands_are_measured_not_quoted` fails the build if a tuned number appears
as a code constant, and `as_norm_cohort_is_provenanced` fails it if any of them
loses its transcript.

## The setup

| | |
|---|---|
| Embedder | `wespeaker_en_voxceleb_CAM++`, sha256 `c46fad10…`, the catalog.json entry the app downloads |
| Extractor | Python `sherpa-onnx==1.13.4`, the version `scripts/yv131-build-impostor-cohort.py` refuses to run without (it compares `sherpa_onnx.__version__` and aborts) |
| Embedding width | **512**, read off the pinned model itself — see below |
| Cohort | LibriSpeech `test-clean`, 40 speakers |
| Design chosen on | LibriSpeech `dev-clean`, 40 speakers |
| Design reported on | LibriSpeech `dev-other` + `test-other`, 66 speakers, 391 genuine / 25,415 impostor trials |
| Segment length | 30 s enrolment, 30 s test — YV126's cluster floor |
| Licence | CC-BY-4.0. Only 512-float embeddings ship; no audio is redistributed |

The four speaker sets are disjoint. That is the point: a cohort sharing speakers
with the targets is not measuring what it claims to, and a design chosen and
reported on one split is not measured, it is fitted.

### The embedding width, and where the 192 came from

The whole 160 KB asset is 78 rows of exactly this width, so it is worth saying
where the number comes from. It comes from the model file, reproducibly:

```bash
curl -L -o campp.onnx \
  "https://huggingface.co/wilsonguenther/yap-diarize-models/resolve/c0f5026b16bf2cac9b5f9e6e2a36da6c6a8628ec/wespeaker_en_voxceleb_CAM%2B%2B.onnx"
shasum -a 256 campp.onnx   # c46fad10b5f81e1aa4a60c162714208577093655076c5450f8c469e522ec54ef
python3 -c "import onnx; m=onnx.load('campp.onnx'); \
print([(k.key,k.value) for k in m.metadata_props]); \
print([(o.name,[d.dim_param or d.dim_value for d in o.type.tensor_type.shape.dim]) for o in m.graph.output])"
```

Output: metadata `output_dim = 512`, graph output `embs ['B', 512]`. The digest
is the one `src/catalog.json` pins and the one
`scripts/yv131-build-impostor-cohort.py` re-verifies before it embeds anything,
so this is the file the app downloads and not a lookalike.

**The 192 in audit finding #19 does not describe this file.** Four comments in
the tree still assert it (`src/diarize_protocol.rs`, `src/diarize.rs`,
`yap-diarize/src/main.rs`, `tests/diarize_sidecar_pool.rs`); this revision
corrects them and says why in place. The mechanism those comments defend — the
sidecar reports `embedding_dim` at load time and no Rust constant names a width
— is unaffected and still right; it is only the illustrative number that was
wrong, which is precisely the drift that would have silently degraded every
ranking to raw cosine through `CohortError::DimMismatch`.

This document previously cited "YV122 measured 512" and "the exact pin
`desktop/yap-diarize/Cargo.toml` carries". Neither is true in this tree: YV122
(#137) is unmerged, that manifest declares `serde` and `serde_json` only, and
`tests/supply_chain.rs` actively asserts the sidecar carries no inference crate
yet. The pin that governs *this* asset is the Python one in the build script.

## The design sweep — including the half that did not survive it

Cohort variant × normalization form × K, every cell scored on the tuning split.
The full table is in the JSON; the best K for each combination:

| cohort | form | best tuning-split EER |
|---|---|---|
| clean+bandlimited | **enrollment-only** | **11.67%** (K=40) ← **ships** |
| clean+bandlimited+headset | symmetric | 11.99% (K=80) |
| clean+bandlimited | symmetric | 12.03% (K=60) |
| clean+bandlimited+headset | enrollment-only | 12.47% (K=70) |
| clean+bandlimited | test-only | 13.75% (K=60) |
| clean+bandlimited+headset | test-only | 14.15% (K=100) |

The selection rule was stated before the split was scored: argmin EER, ties to
fewer cohort conditions, then the simpler form, then the smaller K.

**The literature form of AS-norm is symmetric, and this ships one side of it.**
That is the substantive change in this revision, and it came out of an
adversarial review rather than out of taste. The test-side term had *no
observable consequence anywhere in the shipped code*: within one cluster every
candidate is scored against the same test embedding, so the term is identical
across them — a monotone affine map of the raw score, which cannot reorder
anything — and deleting it from `src/` left every test green. A reviewer
demonstrated that by mutation.

The available responses were to find an assertion the term could fail, or to
find out whether it belonged at all. Sweeping it against the harness answered
the second question: it loses, on the tuning split, in both cohort variants, at
every K. The epic plan asks for the test side by name ("the impostor-score
distribution for that specific recording condition") — and the plan is a
hypothesis, which is what the harness is for.

The three-condition cohort variant exists because the same review pointed out
that a cohort spanning only {clean, telephone-band} gives the adaptive top-K
nothing condition-appropriate to select for a mild headset shift. Adding a
headset-shaped condition was measured too. It also lost — which is consistent
with the shipped form: with the test side gone, the top-K is selected against the
**enrolment** centroid, so the cohort's condition coverage is chosen relative to
the enrolment condition and has no channel to track on the test side anyway.

## The channel ladder, and what AS-norm costs when there is no shift

"Cross-device" is not one thing, so a claim about it is meaningless without the
severity attached. All four B channels are **simulated** filters applied to real
speech — defined in `scripts/yv131-build-impostor-cohort.py::CHANNELS`. No
AirPods were recorded and no volunteer was asked to speak.

| Channel | tune `dev-clean` raw → AS-norm | held out raw → AS-norm | band FAR / FRR (held out) |
|---|---|---|---|
| A clean (control) | 8.85% → 9.58% | 10.00% → **11.25%** | 8.33% / 13.30% |
| B1 headset→laptop, mild | 15.46% → **11.67%** | 18.20% → **14.06%** | 8.81% / 18.16% |
| B2 across-desk, moderate | 23.66% → **21.99%** | 25.58% → **24.30%** | 6.67% / 49.62% |
| B3 far-field room | 32.08% → **30.83%** | 32.23% → **30.69%** | 3.93% / 80.56% |
| B4 telephone band | 32.08% → **31.25%** | 35.04% → **33.50%** | 2.44% / 91.82% |

Read the first row and the last two as carefully as the second.

**AS-norm loses on the matched-condition control**, by 1.25 points on the
held-out split. There is no condition shift to correct for there, and dividing
by an estimated standard deviation adds variance for nothing. An earlier draft
of this document claimed AS-norm was "ahead in all ten split × channel cells";
that was true of an arm half this size and it is not true here. The trade is
real and it is the right trade for this app — the case Wilson asked about is
someone changing devices, not someone using the same one twice — but it is a
trade, and the test now pins it as one: `the_full_ladder_is_committed_not_only_the_flattering_rung`
requires a win on every *shifted* channel and expects the control to regress.

**The benefit also shrinks as the channel gets destructive** — B3 and B4 recover
1.5 points where B1 recovers 4.1. The recalibration below cannot recover identity
information the channel has removed. The band's FRR at B4 is 91.82%, which is not
a working feature at that severity, and saying so here is cheaper than a user
discovering it.

## What the shipped form does, and what it does not

The measured numbers above are not in dispute. The *explanation* attached to them
in the previous revision was wrong, and it is the explanation a maintainer would
act on, so it is corrected here rather than quietly dropped.

`as_norm_score(raw, enrollment)` reads the raw cosine and the **enrolment**
centroid's top-K cohort statistics. **Nothing about the live recording enters the
normalization.** For a fixed profile `μₑ` and `σₑ` are constants, so the shipped
decision is exactly

```text
accept  ⟺  cos ≥ μₑ + 1.2229 · σₑ
```

— a per-**profile** absolute cosine band, identical for every microphone. The
cohort strangers are LibriSpeech rows scored against the enrolment centroid;
their scores do not move when the cluster's device moves. The previous revision
said the band was "expressed relative to strangers recorded on that device" and
that the score answers "a question the microphone cannot skew"; both described
the **test-side** term that the design sweep measured and deleted, and neither is
true of what ships. The shipped decision is condition-blind.

**So why do the cross-condition numbers move?** Because what is removed is the
per-profile (**hubness**) offset. Some enrolled centroids sit in a dense part of
the embedding space and score high against everybody, so one fixed cosine number
is a different strictness for each enrolled person. Dividing that offset out
makes one band mean the same thing across enrolled people, and the FRR *spread*
between a matched channel and a shifted one narrows because inter-speaker score
variance has left the decision — not because the score follows the channel. The
benefit is per-speaker offset calibration; condition tracking is not shipped and
would require the test-side term back, which would have to beat
`yap23-asnorm-measurement.json` first.

The ranking side is a genuine second effect and survives intact: within one
cluster the raw cosine is shared, but each candidate carries its own `μₑ`/`σₑ`,
so normalization reorders candidates that a raw cosine ordered by their offsets.

## The cross-device claim, as a decision rather than a score

The spec's third acceptance criterion is that a profile enrolled on a laptop
microphone is *offered*, not silently missed as `New`, when the same person
appears on AirPods. **That criterion is OPEN.** It is written as a manual check
against a real recording; what is measured below is four simulated filters on
LibriSpeech, and no cross-device recording has been made. It also cannot be
satisfied yet for a structural reason: `speaker_asnorm` has no caller outside
test binaries, so no enrolment and no `Suggested` prompt exists end to end (see
"Not measured" below). Scores do not deliver the criterion; a band does — and the
band is measured here, on simulated channels, three ways on the held-out arm by
`cargo test --test as_norm_admits_across_conditions`.

**One band, two conditions.** Holding one band fixed across the clean control
and the shifted channel:

| | FRR clean | FRR shifted | spread |
|---|---|---|---|
| cosine band (tuned the same way, same split) | 14.3% | 35.8% | **21.5 pp** |
| normalized band | 13.3% | 18.2% | **4.9 pp** |

That spread is what a user experiences as "it knows me at my desk but not on the
couch".

**At a matched false-accept rate.** "Lower FRR" alone proves nothing — any band
can miss fewer true speakers by admitting more strangers. So the cosine band is
handed *exactly* the normalized band's false-accept budget, chosen with
knowledge of the held-out arm, which the normalized band was not allowed:

| band | FAR | FRR |
|---|---|---|
| cosine, tuned on `dev-clean` \| B1 (0.843) | 6.19% | 35.81% |
| cosine, matched to the normalized FAR (0.824) | 8.81% | 29.16% |
| **normalized (1.223)** | **8.81%** | **18.16%** |

**Counted.** Of 391 genuine cross-condition trials, **73 that a fixed cosine
band misses are admitted by the normalized band**, against 4 that go the other
way. Those 73 are the spec's sentence: the same person, a different microphone,
offered instead of missed.

## The distinctness gate

The first draft rejected two cohort speakers as "the same voice" at a hand-picked
0.97, while the cohort it shipped contained a pair at 0.9536 — inside the
0.91–0.997 band the same PR used to condemn a rejected synthetic cohort. A gate
nobody measured cannot adjudicate that.

The gate is now the **99.9th percentile of the cross-speaker cosine distribution
measured on `dev-clean`** — a split the cohort does not draw from — and rows
above it are dropped. Two were (speakers 8455 and 7176, one condition each),
leaving 78 entries with a worst cross-speaker pair of 0.9328.

| distribution (dev-clean, 30 s segments) | median | p99 | p99.9 | max | min |
|---|---|---|---|---|---|
| cross-speaker (3,120 pairs) | 0.637 | 0.901 | **0.933** | 0.953 | 0.109 |
| same speaker, two disjoint recordings (40) | 0.974 | 0.993 | — | 0.994 | **0.907** |

**These two distributions overlap**, and the manifest publishes both tails so
that it is visible rather than implied: the closest a voice came to itself
(0.907) is *below* the gate. No cosine gate anywhere can both admit every
genuinely distinct pair and reject every duplicate under this embedder. The gate
sits between the two medians, which is the strongest true claim available, and
it is a heuristic against a construction error — not a proof of distinctness.

## What did not work, and what was withdrawn

**A synthetic cohort of macOS `say` voices.** Rejected, and the reason has been
re-measured because the original reason did not reproduce.

The decision here is not "can CAM++ tell TTS voices apart" — it is "does a
cohort of TTS voices normalize *human* trials as well as a cohort of humans".
So: swap the cohort, change nothing else, score the held-out arm. Size-matched,
same K:

| cohort | entries | K | held-out AS-norm EER |
|---|---|---|---|
| synthetic (macOS `say`, 8 voices) | 24 | 24 | **16.11%** |
| human, size-matched | 24 | 24 | **13.32%** |
| human, shipped | 78 | 40 | 14.06% |
| *(no cohort — raw cosine)* | — | — | 18.20% |

The synthetic cohort is 2.8 points worse than a human one of the same size, and
that is the load-bearing evidence.

**Two claims from the first draft are withdrawn.** It reported a 35.6% vs 2.2%
EER comparison over `say` voices; that arm is degenerate — TTS synthesis is
deterministic, so two segments of one `say` voice are far more alike than two
recordings of one human, and an equal error rate over them measures the
synthesizer's repeatability, not speaker separability. It also claimed "two
different `say` voices score cosine 0.64 where two different humans score 0.51".
The shipped cohort's own clean cross-speaker distribution contradicts that:
**mean 0.662, median 0.684, max 0.933**, with 57% of pairs above 0.64. The
measured synthetic cross-voice distribution is mean 0.549, median 0.544 — *lower*
than the human one, the opposite of what the sentence claimed.

**A design chosen and reported on the same split.** The very first pass reported
+15.3% relative on `dev-clean` with K=40, chosen there. Scored on a held-out
split for the first time it came out at **−3%** — worse than raw. That is why
the tune/report split exists in this item at all and why
`the_tune_and_report_splits_are_different_corpora` is a test.

**A tuned K with no tuning transcript.** The draft that followed shipped K=20 as
a hard-coded module constant, with the script header claiming it "was chosen on
dev-clean" and this document claiming it shipped "rather than the number that
looked best on the tuning split". Both cannot be true. There is now a `--sweep-k`
mode, the sweep is committed, and
`the_design_sweep_is_committed_and_the_shipped_design_is_its_argmin` fails if the
shipped design is not the transcript's argmin.

## CAM++ below ~20 s is unstable, non-monotonically in duration

Measured on `dev-clean`, cosine of a segment to its own speaker's reference:

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
pip install 'sherpa-onnx==1.13.4' numpy scipy soundfile
# LibriSpeech dev-clean, dev-other, test-clean, test-other from openslr.org/12
# CAM++ from the catalog.json mirror, digest-checked by the script itself
python3 scripts/yv131-build-impostor-cohort.py \
    --model /path/to/wespeaker_en_voxceleb_CAM++.onnx \
    --librispeech /path/to/LibriSpeech \
    --cache /tmp/yv131-cache \
    --tts-baseline
cd desktop/src-tauri && cargo test --test as_norm_cross_condition_measured -- --nocapture
```

The script re-verifies the model digest against `catalog.json` before it will
run, refuses to run under any sherpa-onnx but the pinned one, and writes both the
cohort asset and this document's JSON. `--cache` keeps the split embeddings on
disk so a re-sweep does not re-embed a corpus that has not moved.

## Still not measured

A **real** cross-device recording — one human, one laptop microphone, one pair of
AirPods — has not been run. Every B channel above is a filter, and a filter is a
model of a device change, not a device change. That remains the honest gap in
this item, and it is the one thing that would turn "the mechanism helps under a
simulated shift" into "the mechanism helps Wilson". It needs hardware and a
person, neither of which a test binary has.

The second gap is scale: 66 held-out speakers is enough to separate this effect
from zero and not enough to characterise it finely. The confidence interval on
the headline spans 0.25 to 7.99 percentage points, which is a factor of thirty.
Anyone reading "22.7% relative" as a precise quantity is reading it wrong.

The third gap is that **none of this is wired to anything**. Outside test
binaries the only reference to `speaker_asnorm` in the tree is
`pub mod speaker_asnorm;` in `lib.rs`. The spec's integration point,
`speaker_profiles.rs::match_cluster` (YV129, "extended, not forked"), does not
exist here — YV126/128/129/130 are unmerged and `main` is at YV125. So this item
ships a measured scoring component, not a user-visible behaviour, and spec
acceptance 3 stays OPEN until enrolment exists and a real recording is run
against it.
