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
The one overlap is one frame. The 6.64 s row is a word row stretched over a pause (one reason
`split_utterances` carries a cap fallback instead of trusting row
lengths).

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
(`c46fad10…`). The store landed with yap-1106-voiceprints (see the dated
section below); `tests/supply_chain.rs`
(`the_voiceprint_embedder_digest_is_the_pinned_embedding_model`) compares it
against this pins block.

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

## 2026-11-07 — yap-1106-diarize-wire: the pass is wired (no new package)

No crate or model changes: the Pick above is what runs. `meeting_diarize::diarize_meeting`
(called from `meeting_pipeline::run_meeting_job` after the transcript is final and before
notes) hands the right track to the vendored sidecar through `diarize::cluster_track`:
the mic track for in-person and unknown meetings, the call track for a virtual meeting
(its mic stays "You"). Rows are attributed from the ASR word times kept in the per-track
resume ledger with `meeting_asr::build_speaker_turns` (section 2), and a row whose words
belong to two speakers is split at the switch (`Database::split_segment_by_speaker`).

Two caller-side numbers, both PROVISIONAL and both ported from the hedgehog analyzer's
design (DIARIZE.md "Pipeline"; diarize.py `_cluster` and `_w_spk`), not measured on
Yap's corpus yet — the PENDING measurements in the table above still decide them:

- cosine DISTANCE cut `PROVISIONAL_CLUSTER_DISTANCE = 0.75` (env `YAP_EVAL_CLUSTER_DISTANCE`, evals only);
- shortest turn worth an embedding `PROVISIONAL_SHORTEST_TURN = 1.5 s` (env `YAP_EVAL_MIN_TURN_MS`, evals only).

Not ported in this item (later analyzer items own them): folding clusters under 1.6 s
into the nearest centroid (`_merge_small`), single-window smoothing (`_mode_smooth`),
enrolled voiceprints, the talk-time summary, and overlap flags.

## 2026-11-07 — yap-1106-voiceprints: saved voiceprints (no new package)

No crate or model changes. Enrolled speaker profiles (a re-port of the parked
`feat/yv128` / `feat/yv129` designs, and the "enrolled speaker profiles" idea of
the hedgehog analyzer's DIARIZE.md "Pipeline") live in `speaker_profiles.rs`
and migration 11 (`speaker_profiles`, `speaker_samples`, plus
`meeting_segments.speaker_profile_id` / `speaker_name`).

- A profile's `embedder_digest` is the catalog's pinned sha256 of
  `wespeaker-en-voxceleb-campplus` (the pins block above), read at runtime via
  `speaker_profiles::pinned_embedder_digest`. Profiles of another digest or
  dimension are never compared: the matcher answers "re-enroll".
- The centroid is the normalised mean of normalised turn embeddings. Only
  turns attributed to one cluster are embedded, inside the word-exact bounds of
  section 2 minus `TURN_EMBED_GUARD_SECONDS` (0.2 s) at each end.
- Two cosine-SIMILARITY bands, both PROVISIONAL until #152's scored population
  is measured: `auto` = `PROVISIONAL_AUTO_SIMILARITY = 0.70` (the one shared
  0.70), `ask` = `PROVISIONAL_ASK_SIMILARITY = 0.55`; eval-only overrides
  `YAP_EVAL_VOICEPRINT_AUTO` / `YAP_EVAL_VOICEPRINT_ASK`;
  `MatchBands::tuned` derives them from a genuine/impostor population
  (`ask` = the EER threshold, `auto` = the lowest threshold within a FAR cap).
- Everything stays on this Mac ("Leaves this Mac: No").

## 2026-11-07 — yap-1106-who-is-this: the "Who was this?" cards (no new package)

No crate or model changes; the pins above are untouched, so
`tests/supply_chain.rs` keeps asserting the same digests.

- The full-clustering pass now keeps each cluster's voice
  (`diarize::cluster_voices`: the L2-normalised mean of the cluster's
  L2-normalised turn embeddings, plus its talk seconds) in migration 12's
  `meeting_cluster_voices`, stamped with the pinned embedder digest
  (`speaker_profiles::pinned_embedder_digest`). This is the hedgehog
  analyzer's DIARIZE.md "Pipeline" idea of one saved centroid per person,
  fed from the turns the pass already embedded; nothing new is embedded.
- Cards: clusters with at least 20 s of speech that match no saved voice,
  five at most, most speaking time first, never "You"
  (`speaker_profiles::pick_voice_cards`). `ask`/`auto`-band matches read
  "Same person as Alex?" with the reason ("Probably Alex — voice match 0.82").
- The clip is at most 3 s cut from the start of the cluster's LONGEST turn
  (`speaker_profiles::clearest_clip`); turns are the word-exact rows of
  section 2, so a clip never crosses a speaker switch. Played from the kept
  audio; once the 7-day retention purged it, the card says so and still
  names from the line.
- Save / That's me / "Yes, same person" relabel every row of the cluster in
  that meeting (whole turns) and add the cluster voice as a sample when one
  was kept (`Database::name_meeting_cluster`). "Not sure" and "Ignore this
  voice" keep nothing (`Database::dismiss_meeting_cluster`). "Forget all
  voices" and deleting the meeting clear the kept cluster voices.
- Everything stays on this Mac ("Leaves this Mac: No").

## 2026-11-07 — yap-1106-auto-label-and-correct: auto-labels and corrections (no new package)

No crate or model changes; the pins above are untouched, so `tests/supply_chain.rs`
keeps asserting the same digests.

- Future meetings: after the clustering pass keeps each cluster's voice,
  `Database::auto_label_meeting` names every `auto`-band cluster (the shared
  provisional 0.70) with the saved voice's name — one cluster per person per
  track (the best match), never a line a person already named or corrected.
  This is the hedgehog analyzer's DIARIZE.md "Pipeline" idea of one saved
  centroid per known person, applied to new meetings.
- Corrections (a speaker chip's Rename / This is someone else / Merge with… /
  Split this speaker) are pure reducers over the meeting's lines
  (`speaker_profiles::apply_correction`). Lines are the word-exact turns of
  section 2, so a correction moves whole turns and never leaves a word with the
  old speaker. Each corrected line carries the correction's lock token
  (migration 13, `meeting_segments.speaker_locked`).
- A correction that moves a whole speaker feeds the voiceprints: the cluster's
  kept voice becomes a positive sample of the new name and a negative sample
  (`speaker_negative_samples`) of the old one. A match that a negative sample
  sits at least as close to as the profile's own centroid is not trusted for
  auto-labelling (`speaker_profiles::vetoed_by_negatives`). Partial moves and
  splits have no voice of their own and feed nothing.
- Undo lasts 10 s and asks the lock (the YV130 rule): only lines still carrying
  that correction's token are restored, so a later correction — even to the
  same name — stands.
- The Markdown export and both notes paths read the corrected names
  (`meetings::render_markdown_named`, `render_transcript_named`).
- Everything stays on this Mac ("Leaves this Mac: No").
