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
  (`SUMMARY_VERBATIM_CEILING`), with no groundedness floor.
- **C**: arm E with a Qwen2.5-3B q4 model file, measured only (`YAP_EVAL_ARM_C_MODEL`), never
  shipped; must stay at or under 2.2 GB peak RSS.

The BYOM arm is removed: the yap-byom-* items own BYOM.

Honest fallback copy shipped with E: no Summary passes, so no paragraph and a "Topics" section with
the timestamped labels; an action that fails the item gates is quoted under "Lines that sounded like
commitments" with its time and track, never filed under "Action items"; every notes card ends with
"Written on this Mac by a small model. Check owners against the transcript."

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

For reference, the last measured run before the guards and before this item (2026-10-27, the
pipeline A started from, in docs/BUDGETS.md): action recall 0.20, precision 0.50, decision recall
1.00, verbatim 0.35, duplicates 16, max items per segment 3, owner accuracy 0.50, 47.9 s wall.

DECISION: ship arm E (items first, Summary last) as the default notes strategy (`NOTES_STRATEGY =
NotesStrategy::E`), no second local model by default, no hosted model, no change to the
polish_model setting; the integrator's measured rows above confirm it or overturn it.

## Follow-up items

- yap-audit-notes-strategy-call-bleed: run the same arms on call-bleed-2voice through the AEC
  (yap24-NT11) transcript, not only the planted transcript fixture.
- yap-audit-notes-strategy-arm-c: provision the Qwen2.5-3B q4 file to a COPY location and record
  arm C's row and peak RSS with the ASR engine unloaded.
- yap-audit-notes-strategy-human-31min: the human half, numbers only: verbatim ratio, duplicate
  count, max items per segment and owner counts from scripts/eval/compare-reference.py --notes on a
  COPY of the 31-minute meeting, pasted in the PR body.
