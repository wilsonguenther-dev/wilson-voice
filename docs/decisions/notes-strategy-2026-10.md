# Notes strategy: items first, Summary last (yap-audit-notes-strategy)

Written 2026-11-08T03:12:13Z by loop three coder B2. Panel 3 (2026-10-26) approved arm E with changes as the
advisory choice; the numbers below confirm or overturn it.

## The problem

The 0.80 content-word groundedness floor (`GROUNDEDNESS_FLOOR`, raw 4+ character tokens, no
stemming) rejected paraphrase ("discussed", "agreed", "planning" never appear in a transcript:
38 / 15 / 22 `v3_ungrounded` rejections per real meeting) and kept verbatim copies. A 1.5B model
classifies short spans well under a grammar; it does not write abstractive prose over 1,400 noisy
tokens reliably.

## The arms (all in `desktop/src-tauri/src/summarize.rs`, `NotesStrategy`)

- **A**: today's pipeline plus the yap-audit-notes-guards fixes: MAP (narrative + items), REDUCE,
  0.80 floor. Kept runnable for the eval and for the arm-A contract tests.
- **B**: A with a paraphrase-tolerant grounding check (`lemma_gates`): digits, contacts and names
  stay hard-gated, the rest matches at lemma level (`LEMMA_GROUNDEDNESS_FLOOR` = 0.6).
- **E** (shipped, `NOTES_STRATEGY`): MAP keeps the grammar items (actions, decisions, questions,
  each tied to a segment id) and replaces the narrative with a 3-8 word topic label per chunk
  (`map_grammar_items_first`, timestamped with the chunk's first line). No REDUCE fold. Items are
  grounded against their CITED segment +-1 (`ground_items_in_window`). The Summary is ONE pass whose
  input is only the surviving items plus the topic labels (`items_first_summary_input`), gated by
  hard digit / name / contact checks and a 4-gram verbatim ceiling of 0.5
  (`SUMMARY_VERBATIM_CEILING`), judged after forgiving ONE lifted 4-gram
  (`SUMMARY_VERBATIM_SLACK_NGRAMS`, `items_first_verbatim_ratio`), with no groundedness floor.
- **C**: arm E with a Qwen2.5-3B q4 model file, measured only (`YAP_EVAL_ARM_C_MODEL`), never
  shipped; must stay at or under 2.2 GB peak RSS.

The BYOM arm is removed: the yap-byom-* items own BYOM.

Honest fallback copy shipped with E: no Summary passes, so no paragraph and a "Topics" section with
the timestamped labels; an action that fails the item gates is quoted under "Lines that sounded like
commitments" with its time and track, never filed under "Action items"; every notes card ends with
"Written on this Mac by a small model. Check owners against the transcript."

## Bounce fix (2026-11-08T21:41:12Z, loop three coder B1)

Batch 1 bounced the item: under arm E the guards' clean planted fixture
(`tests/summary_audit_guards.rs`, `a_clean_planted_fixture_still_yields_its_planted_actions`) lost
its Summary. The paragraph, "The Liberty University pilot starts with two hundred students and Jeisil
will send the signed contract.", is a real paraphrase that joins two items, but 7 of its 13 distinct
4-grams occur in the transcript (raw 0.54): it has to name the meeting ("the Liberty University
pilot") and the items it is written from are the transcript's own lines. On a one-sentence Summary a
single 4-gram moves the ratio 7.7 points, so a flat 0.5 refused it as a re-transcript. The gate now
forgives one lifted 4-gram before it applies the ceiling: that paragraph judges 6 of 13 (0.46) and is
kept; two transcript lines pasted together (13 of 16) still judge 12 of 16 and are refused; over a
real 40-word paragraph the allowance is worth at most 0.04, over 100 words 0.01. The eval keeps
reporting the RAW ratio (`support::eval_metrics::summary_verbatim_ratio`), so the table below is not
softened by the allowance. Test: `summary_items_first::the_summary_gate_forgives_one_lifted_phrase_and_no_more`.

## The numbers (planted 15-minute fixture of yap24-NT4)

Gates: action recall >= 0.8, action precision >= 0.8, decision recall >= 0.5, owner accuracy
>= 0.8 or blank, Summary 4-gram verbatim <= 0.5, cross-section duplicates 0, items per segment <= 2.

NOT MEASURED YET. The coder that wrote this file may not compile or run models (loop rule, one
24 GB Mac). The integrator runs the eval below once and replaces the four placeholder rows with the
`NOTES-ARMS-TABLE` rows it prints (numbers with a decimal point). No number here is invented.

```
cd desktop/src-tauri
YAP_DATA_DIR="$(mktemp -d)/yap-state" YAP_EVAL_REQUIRE=1 \
  YAP_MODEL_DIR="$HOME/code/wilson-voice-loop/cache/models" \
  cargo test --features custom-protocol --test meeting_notes_recall_eval -- --ignored --nocapture --test-threads=1
```

| Arm | Action recall | Action precision | Decision recall | Owner accuracy | Summary 4-gram verbatim | Cross-section dups | Max items per segment | Wall s | Peak RSS MB |
|---|---|---|---|---|---|---|---|---|---|
| A | pending integrator run | | | | | | | | |
| B | pending integrator run | | | | | | | | |
| C | pending (needs YAP_EVAL_ARM_C_MODEL) | | | | | | | | |
| E | pending integrator run | | | | | | | | |

The same arms on `call-bleed-2voice` through the shipped input path (the second table the eval
prints, `NOTES-ARMS-TABLE call-bleed-2voice (AEC-cleaned ASR)`, from the ignored test
`call_bleed_arms_over_the_aec_cleaned_transcript`): the AEC pre-pass (yap24-NT11) cleans the far
side out of the microphone, the shipped decoder transcribes both tracks on the shipped windows,
`chunk_segments` makes the rows the app would store, and every arm writes notes from those rows,
scored against the commitments and decisions planted in the corpus `meta.json` (keyword match;
ASR rows never share a planted start). The ASR runs in a child process that has exited before any
notes pass starts, so peak RSS is the notes pass with the ASR engine unloaded. The NT4 gates bind
the shipped arm on this table too.

| Arm (call-bleed-2voice) | Action recall | Action precision | Decision recall | Owner accuracy | Summary 4-gram verbatim | Cross-section dups | Max items per segment | Wall s | Peak RSS MB |
|---|---|---|---|---|---|---|---|---|---|
| A (call-bleed) | pending integrator run | | | | | | | | |
| B (call-bleed) | pending integrator run | | | | | | | | |
| C (call-bleed) | pending (needs YAP_EVAL_ARM_C_MODEL) | | | | | | | | |
| E (call-bleed) | pending integrator run | | | | | | | | |

For reference, the last measured run before the guards and before this item (2026-10-27, the
pipeline A started from, in docs/BUDGETS.md): action recall 0.20, precision 0.50, decision recall
1.00, verbatim 0.35, duplicates 16, max items per segment 3, owner accuracy 0.50, 47.9 s wall.

DECISION: ship arm E (items first, Summary last) as the default notes strategy (`NOTES_STRATEGY =
NotesStrategy::E`), no second local model by default, no hosted model, no change to the
polish_model setting; the integrator's measured rows above confirm it or overturn it.

## Follow-up items

- yap-audit-notes-strategy-arm-c: provision the Qwen2.5-3B q4 file to a COPY location and record
  arm C's row and peak RSS with the ASR engine unloaded.
- yap-audit-notes-strategy-human-31min: only if the batch PR lands without the human-half numbers
  below; the test and the commands exist, the run needs the integrator on this Mac.

## Human half (numbers only, never text)

The ignored test `human_half_real_meeting_copy_numbers_only` runs the shipped arm on a COPY of the
31-minute meeting (stored rows, echo-suppressed rows excluded), prints one `HUMAN-HALF` line of
numbers (verbatim ratio, duplicates, max items per segment, owner counts Me / Them / Someone here /
blank / other) and stores the notes in the copy, so `scripts/eval/compare-reference.py --notes`
reads the same notes. Wilson's data dir is read only through the copy; the copy is deleted after.

```
T="$(mktemp -d)"; cp "$HOME/Library/Application Support/WilsonVoice/wilson_voice.db"* "$T/"
ID="$(sqlite3 "$T/wilson_voice.db" "SELECT id FROM meetings WHERE duration_seconds BETWEEN 1800 AND 1920 ORDER BY started_at DESC LIMIT 1")"
cd desktop/src-tauri
YAP_DATA_DIR="$(mktemp -d)/yap-state" YAP_MODEL_DIR="$HOME/code/wilson-voice-loop/cache/models" \
  YAP_EVAL_MEETING_DB="$T/wilson_voice.db" YAP_EVAL_MEETING_ID="$ID" \
  cargo test --features custom-protocol --test meeting_notes_recall_eval -- --ignored --nocapture \
  --test-threads=1 human_half_real_meeting_copy_numbers_only
python3 ../../scripts/eval/compare-reference.py --notes "$T/wilson_voice.db" "$ID"
rm -rf "$T"
```

The numbers go in the batch PR body.
