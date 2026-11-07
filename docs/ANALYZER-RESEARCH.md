# Analyzer research — word timestamps, speaker turns, diarization and embeddings

Binding for the analyzer chain (Wilson, 2026-11-06 18:05Z, ENGINEERING MUSTS (b)).
Later items FOLLOW the pick below. An item that needs a new package or model
appends a dated section with the reason and the pin, and updates
`desktop/src-tauri/tests/supply_chain.rs` in the same PR. Never upgrade or swap
a model or crate without updating this doc and that test: the test reads the
`pins` block at the bottom of this file and fails when it and the manifests or
the model catalog disagree.

Written 2026-11-07 by yap-audit-utterance-segments (section 1 and 2, the parts
this item owns). Sections 3–4 are the survey frame; the quality and speed
measurements on the repo's diarize fixtures are owned by yap-1106-diarize-wire
and are marked PENDING where they have not been taken. Nothing below is a
number that was not measured or read from a committed file.

## 1. Word timestamps

**Which engine path gives word times.** `asr_engine::transcribe_timed`
(desktop/src-tauri/src/asr_engine.rs) asks `transcribe-cpp` for
`TimestampKind::Auto` ("richest this family supports") and keeps
`TranscriptResult.words`. Each native row is `Word { t0_ms, t1_ms, seg_index,
first_token, n_tokens, text }` (transcribe-cpp 0.1.3, `src/result.rs`): a WHOLE
word that the native side assembled from its tokens, so a word row is never a
sub-word piece. `TimedKind` records the granularity the decode actually came
back with; on the shipped Parakeet build it is `token` (the 2026-10-26 audit of
meeting c6187806: 62 of 62 track-0 chunks carried spans, `timestampKind token`).
`TimedTranscript::best_spans` prefers the word rows and only falls back to
segment rows when a model produced no words.

The meeting chunker shifts the rows onto the meeting timeline
(`TimedSpan::shifted`) and keeps them in the resume ledger
(`<id>.tN.asr-progress.json`, `chunks[].spans`). These are the times every
speaker turn is built from (section 2). Window and chunk times are never used
for a turn bound, and nothing is interpolated from character offsets.

**Measured on the fixtures** (read from the committed timings-only ledger
`desktop/src-tauri/tests/fixtures/meeting-utterances/call-bleed-2voice.t1.asr-progress.json`,
the NT0 synthetic corpus, system track; 10 windows, 416 word rows):

| quantity | value |
|---|---|
| shortest word row | 0.08 s |
| median word row | 0.08 s |
| longest word row | 6.64 s |
| median gap between consecutive words in a window | 0.08 s |
| rows with zero or negative length | 0 |
| consecutive rows that overlap | 1 (by 0.08 s) |

Every time is a multiple of 0.08 s: the encoder's 80 ms frame is the
resolution of these word times, and a turn bound inherits that resolution.
The one overlap is one frame. The 6.64 s row is a word stretched over a pause,
which is why `split_utterances` has a cap fallback rather than trusting row
lengths.

**Accuracy against a reference: PENDING.** The NT0 corpus carries no
word-level reference times, so the error of these rows against ground truth has
not been measured. INTEGRATOR: forced-align one corpus file (or hand-mark ten
word onsets in `call-bleed-2voice.t1.wav`) and append the median and p95 onset
error here.

**How they are pinned.** `transcribe-cpp` is declared `0.1.3` (a caret range)
in desktop/src-tauri/Cargo.toml, and `Cargo.lock` resolves it to exactly
`0.1.3`; the supply-chain test asserts the lockfile version equals the `locked`
line in the pins block, so a lockfile bump that changes the word-time source
fails a test instead of passing silently. The ASR model file itself is pinned by
its catalog sha256 (models.rs), outside this doc.

## 2. Speaker turns from word times (the rule)

`meeting_asr::build_speaker_turns(track, words, segments)`:

- A turn is a run of whole word rows on one track that all went to the same
  speaker. A word is never split.
- Each word goes to the diarization segment that covers the word's MIDPOINT.
  When two segments cover it, the one with the larger overlap. When the
  midpoint falls in a gap between segments, the segment with the larger
  overlap with the word; when the word touches no segment, the nearest one.
- Consequence, true by construction: no turn contains a word whose midpoint
  lies in another speaker's segment. A switch to another speaker never bleeds a
  word across the boundary.
- A turn's start and end are its first word's start and its last word's end,
  exactly.
- Unattributed segments (no cluster) are each their own speaker: two
  neighbouring unknowns may be two people, so they never merge.
- Clips, embeddings and relabels use these bounds. Embeddings use
  `SpeakerTurn::embedding_window`, which skips `TURN_EMBED_GUARD_SECONDS`
  (0.2 s) nearest each boundary.
- Log line: `meeting_asr turns built n=… words=… boundary_words_reassigned=… track=…`,
  where `boundary_words_reassigned` counts words that overlap two speakers'
  segments (a switch inside the word).

Proven by `desktop/src-tauri/tests/speaker_turns_word_times.rs` over
`tests/fixtures/speaker-turns/two-voice-interleave.json` (A/B alternating every
2–4 s, a one-word interjection, two switches inside a word, a trailing word
past the last segment): zero cross-attributed words, every bound equal to its
word time. The real-audio variant is `#[ignore]`d until a diarization run with
the models is available.

## 3. Diarization and speaker-embedding options (survey frame)

Hard constraint, from desktop/yap-diarize/Cargo.toml: **never a second
onnxruntime linked into the same binary.** `wilson-voice` already links an
onnxruntime through `vad-rs`/`ort`; `sherpa-onnx-sys` statically links its own.
That is why diarization lives in the `yap-diarize` sidecar. `ort` is acceptable
only in a separate process; candle is pure Rust and links nothing native.

Sources (official; not re-fetched in this pass, so model-card numbers are not
transcribed here):

- sherpa-onnx releases and docs: https://github.com/k2-fsa/sherpa-onnx/releases ,
  https://k2-fsa.github.io/sherpa/onnx/speaker-diarization/index.html
- segmentation models: https://github.com/k2-fsa/sherpa-onnx/releases/tag/speaker-segmentation-models ,
  model card https://huggingface.co/pyannote/segmentation-3.0
- embedding models (WeSpeaker, 3D-Speaker, NeMo exports in one zoo; the tag is
  spelled this way upstream): https://github.com/k2-fsa/sherpa-onnx/releases/tag/speaker-recongition-models
- WeSpeaker: https://github.com/wenet-e2e/wespeaker ; 3D-Speaker: https://github.com/modelscope/3D-Speaker ;
  NeMo TitaNet: https://catalog.ngc.nvidia.com/orgs/nvidia/teams/nemo/models/titanet_large
- candle: https://github.com/huggingface/candle ; ort: https://github.com/pykeio/ort

| option | role | link risk | size | license | measured on repo fixtures |
|---|---|---|---|---|---|
| pyannote segmentation 3.0 via sherpa-onnx =1.13.4 (vendored) | segmentation | none (sidecar) | 6,958,444 B archive | MIT (catalog) | PENDING (yap-1106-diarize-wire) |
| WeSpeaker CAM++ en VoxCeleb via sherpa-onnx (vendored) | embedding | none (sidecar) | 29,292,684 B | CC-BY-4.0 weights, Apache-2.0 toolkit (catalog) | PENDING |
| WeSpeaker ResNet34 via sherpa-onnx | embedding | none (same sidecar) | not vendored | per model card | PENDING |
| 3D-Speaker CAM++ / ERes2Net via sherpa-onnx | embedding | none (same sidecar) | not vendored | per model card | PENDING |
| NeMo TitaNet export via sherpa-onnx | embedding | none (same sidecar) | not vendored | per model card | PENDING |
| candle ECAPA-TDNN / WavLM-SV port | embedding | none (pure Rust) | not vendored | per model card | PENDING |
| `ort` + any ONNX model | either | only out of process | — | per model card | PENDING |

## Pick

Keep the vendored pair: pyannote segmentation 3.0 and WeSpeaker CAM++ (English,
VoxCeleb), run through `sherpa-onnx =1.13.4` inside the `yap-diarize` sidecar.
Why: it is already vendored to a Wilson-owned mirror at a pinned revision with
compiled-in digests, it is the only option with zero new link risk and zero new
download, both licenses permit commercial use, and the alternatives in the same
zoo are drop-in swaps through the same sidecar API when the PENDING
measurements show one is better. A swap is a new dated section here plus a
changed pins block, never a silent catalog edit.

The voiceprint `embedder_digest` stores the embedding model's sha256 below
(`c46fad10…`). That store is not on main yet; when it lands, its test must
compare against this pins block.

## Pinned versions

The block below is machine-read by `tests/supply_chain.rs`
(`analyzer_research_pins_match_the_manifests_and_the_catalog`). Format: one pin
per line, `crate <name> =<x.y.z>` (exact pin in desktop/yap-diarize/Cargo.toml),
`locked <name> <x.y.z>` (Cargo.lock resolution), `model <catalog id> <field> <value>`.

```pins
crate sherpa-onnx =1.13.4
crate sherpa-onnx-sys =1.13.4
locked transcribe-cpp 0.1.3
model sherpa-onnx-pyannote-segmentation-3-0 sha256 24615ee884c897d9d2ba09bb4d30da6bb1b15e685065962db5b02e76e4996488
model sherpa-onnx-pyannote-segmentation-3-0 extracted_sha256 220ad67ca923bef2fa91f2390c786097bf305bceb5e261d4af67b38e938e1079
model sherpa-onnx-pyannote-segmentation-3-0 revision c0f5026b16bf2cac9b5f9e6e2a36da6c6a8628ec
model wespeaker-en-voxceleb-campplus sha256 c46fad10b5f81e1aa4a60c162714208577093655076c5450f8c469e522ec54ef
model wespeaker-en-voxceleb-campplus revision c0f5026b16bf2cac9b5f9e6e2a36da6c6a8628ec
```
