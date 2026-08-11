# Test fixtures

## `crash/wilson-voice-crash.ips`

A macOS crash report for Yap (YV64), sanitized: the incident UUID is zeroed, the
pids are fake, and the thread/image lists are cut to two entries each. The SHAPE
is verbatim — a one-line JSON header followed by a second JSON document — which
is the whole point: `crash::parse_ips_header` / `parse_report` are tested
against the real format, not against a hand-invented one.

## `quick-brown-fox-16k.wav`

**Spoken phrase (verbatim):** *"The quick brown fox jumps over the lazy dog."*

16 kHz mono 16-bit PCM, ~2.5 s, ~84 KB. Generated locally on macOS (no network,
no third-party asset), from the system speech synthesizer:

```sh
say -v Samantha -o /tmp/yv32.aiff "The quick brown fox jumps over the lazy dog."
afconvert -f WAVE -d LEI16@16000 -c 1 /tmp/yv32.aiff quick-brown-fox-16k.wav
```

It backs the headless end-to-end gate (YV32) — the one command that proves the
real pipeline (wav → 16 kHz mono samples → embedded GGUF engine → text) works:

```sh
cd desktop/src-tauri
cargo run --release -- --transcribe-file tests/fixtures/quick-brown-fox-16k.wav
```

The printed text must contain the phrase words (e.g. `quick`, `fox`). The first
run downloads the catalog's smallest model (Whisper Tiny, ~46 MB, sha256
verified) into Application Support; later runs are offline.

## `meeting_eval_manifest.json` + `.sha256` — the meeting eval corpus (YV90)

The accuracy backstop for the yap22-A notetaker, landed BEFORE any capture or
transcription code so that every accuracy claim downstream has a number to check
against. The harness is `tests/meeting_eval.rs`; two numeric gates, both derived
from the epic plan's finding #16 ("no evaluation harness anywhere in the plan …
acceptance criteria are unfalsifiable"):

| gate | fixture | assertion |
|------|---------|-----------|
| WER | `lecture-15min` | `wer <= 0.15` on a ~15-minute single-speaker lecture — the stated 22-A target case (a student in a lecture) |
| merge | `lecture-15min` | `no_anchor_seams == 0` over all 32 seams, and an insertion rate `<= 0.005` — an unmerged seam emits its overlap twice, which is an insertion against an exact reference and nothing else |
| seam ordering | `seam-stress` | `duplicated_word_count == 0`, `dropped_word_count <= 1` for the marker words placed inside the chunker's overlap regions, and `timestamps.is_sorted()` over the chunked decode's segment start times |
| seam drift | `seam-stress` | the merged chunked transcript is within `0.02` WER of a single continuous decode of the same audio, and lost at most `MAX_TAIL_TRIM + MAX_HEAD_SKIP` words per seam |

**Why the marker counters are not the gate, only part of it.** They score the
five declared marker words, because those are the only words KNOWN to sit in an
overlap region. That makes them necessary and not sufficient: a merge that eats
ordinary words while sparing the markers passes them. Measured, not argued —
patching the merge to delete every third token except the markers left
`duplicated=0 dropped=0` while the chunked transcript drifted 0.3326 WER from the
continuous decode, 157 real words gone. So the drift the harness was already
computing is now asserted (`drift_within_budget`, with its own corpus-free
negative control `seam_drift_gate_catches_a_marker_preserving_word_eater`), and
the merge reports what it did at each seam so the LECTURE — which has no marker
words at all — is checked too. Both probes are in
`docs/pr-screenshots/YV90/meeting-eval-run.txt`, applied, run and reverted.

**Where a "chunk boundary" actually is, and why it is derived rather than
typed.** The windows are `CHUNK_SECONDS` = 30 s wide and hop by
`CHUNK_SECONDS - CHUNK_OVERLAP_SECONDS` = 28 s, so they run 0–30, 28–58, 56–86,
84–114, 112–142, 140–…, and the only regions TWO windows both contain are
`[k*28, k*28+2]`. The first cut of this fixture centred each marker sentence on
`k * 30 s` instead, which put four of the five markers in the interior of a
single window — and a marker only one window sees is counted identically by a
correct merge, a duplicating merge and a merge that eats real words. The gate was
80% vacuous: the exact failure mode the plan's finding #16 describes, reproduced
inside the harness built to prevent it. The fixture now derives its boundaries
from those two constants, places the marker WORD (not the sentence) at the
midpoint of the overlap region — the carrier sentence is synthesized in three
pieces so the word can be positioned to the sample — and records each word's span
in `meta.json`. Two guards run before the counts are scored: a static one that
re-derives the seams and asserts every marker span lies inside one, and a dynamic
one that asserts every scored marker was decoded in at least two windows,
failing with *"marker X is inside a single window — the seam gate would be
vacuous"*. Both are shown failing, on purpose, in `docs/pr-screenshots/YV90/`.

Measured on 2026-08-11, Parakeet Unified EN 0.6B (Metal): **WER 0.0048** on the
904.7 s lecture (15 errors over 3117 reference words — 15 substitutions, 0
deletions, **0 insertions**) across 33 windows and 32 seams, every one of which
found its anchor; and on the 162.0 s seam fixture **0 duplicated / 0 dropped**
marker words, all five decoded in exactly 2 of the 6 windows, with the merged
chunked transcript differing from a single continuous decode of the same audio by
one substitution (drift WER 0.0021 over 475 words). 0.15 stays as the committed
WER gate: it is the placeholder baseline the backlog specifies, to be replaced
with a tuned number once YV93 lands the real chunker.

The lecture number moved from 0.0151 to 0.0048 when `MAX_TAIL_TRIM` went from 2
to the overlap's own word budget (`OVERLAP_TOKEN_BUDGET` = `ceil(2 s * 175 wpm /
60)` = 6). At 2, three of the 32 lecture seams found no anchor and appended the
whole incoming window — 32 insertions, the overlap said twice at 112 s, 252 s and
504 s. The cause is not a half-cut word: the outgoing window ends mid-sentence
and the model finishes the sentence its own way ("…before the break i want to
**look at the material**" against the next window's "…before the break i want to
**leave you with a question**"), so the genuine anchor sits several tokens back
from the end. Trimming that far is safe by construction — the trimmed tokens are
inside the overlap, which the incoming window re-supplies from the anchor onward
— and it is bounded, which is what `MergeReport` reports and the gates assert.

The lecture is scored on a **windowed** decode — 30 s windows, 2 s overlap,
merged at the seams — not on one pass over the whole file, and that is a
measurement rather than a preference: a single-pass headless decode of the
904.7 s fixture climbed to ~5.1 GB RSS, drove the machine into swap and made no
decoding progress (8.7 s of CPU in 59 s of wall clock) before it was killed. So
the WER number includes seam cost, which is the number worth gating on for a
meeting, and it is the first measured support for the plan's windowed-only ASR
decision (finding #11).

A third fixture, `device-change`, is generated here and consumed by YV92: a short
recording whose input format changes mid-way (48 kHz built-in mic → 24 kHz, what
AirPods report), with both native-rate halves kept beside the resampled 16 kHz
track so the resampler can be driven at the rate the device actually declared.

**The audio is NOT in this repo, and the audio is not anybody's speech.** Same
rule as the gate corpus below: real dictation never enters a public repo, and a
meeting recording is strictly worse — it carries other people's voices. Every
fixture is rendered on the machine by the macOS speech synthesizer (`say -v
Samantha -r 175` → `afconvert`) from invented, mundane sentences with no names,
no digits and no addresses. That has a second benefit worth stating: the
reference transcript is EXACT by construction (it is the text that was spoken),
so a WER number measures the decoder rather than a human's guess at what was
said.

The corpus lives at `~/yap-eval-corpus/meetings/` — durable, outside the repo and
outside any scratch directory. What is committed is the generator (a `#[ignore]`
writer in `tests/meeting_eval.rs`, the same pattern the gate corpus uses), each
fixture's `meta.json`, and the two manifests. `say` output is stable for a given
macOS + voice build and NOT across them, so regenerating on another machine
legitimately changes every hash: re-run the writer and commit the diff.

```sh
cd desktop/src-tauri
# grow the corpus (~2 minutes of `say`), then hash it into both manifests
cargo test --test meeting_eval meeting_eval_generate_corpus -- --ignored --nocapture
# regrow ONLY the seam fixture — the one tied to the chunk geometry — and re-hash
cargo test --test meeting_eval meeting_eval_generate_seam_stress -- --ignored --nocapture
# re-hash an existing corpus without regenerating the audio
cargo test --test meeting_eval meeting_eval_write_manifest -- --ignored --nocapture
# run the gates (decodes through the app's own headless engine — 26 s warm, minutes cold)
cargo test --test meeting_eval -- --nocapture
```

Two manifests, one list. `meeting_eval_manifest.json` is the structured one the
harness reads (sizes, hashes, fixture ids, the synthesis settings the corpus was
grown with); `meeting_eval_manifest.sha256` is the same list in the format
`shasum -a 256 -c` reads, because a JSON document cannot also be a checksum file
— every line of one would have to start with a bare hex digest, which no JSON
line can. `meeting_eval_manifest_is_committed_and_names_three_fixtures` asserts
the two agree, so they cannot drift. To verify the corpus by hand, with no cargo
involved:

```sh
cd ~/yap-eval-corpus/meetings
shasum -a 256 -c "$OLDPWD/desktop/src-tauri/tests/fixtures/meeting_eval_manifest.sha256"
```

With the corpus absent — CI, a fresh clone, anyone else's machine — every
corpus-gated test prints `meeting eval corpus not found at
~/yap-eval-corpus/meetings, skipping` and passes. The metric tests do not need
the corpus and always run, including the negative controls that keep the gates
falsifiable: `seam_dedupe_never_deletes_real_words` builds a "dedupe" that
satisfies the naive *no duplicated words* criterion by deleting the overlap
outright, and proves the gate fails it, while
`seam_regions_are_the_only_places_two_windows_overlap` does the arithmetic that
placed the markers — a span inside `[k*28, k*28+2]` is in two windows, one on
`k*30` is in one — so the prose above cannot drift from `CHUNK_SECONDS` /
`CHUNK_OVERLAP_SECONDS` again without a red test.

## `gate/*.jsonl` — the public-safe hallucination-gate corpus (YV76)

The committed corpus for `dictation::degenerate_cutoff` is SYNTHETIC, and that is
a permanent decision, not a placeholder: the raw dictation corpus below stays out
of this public repo for good (YV76). Nothing under `gate/` is anybody's speech —
every case is generated or hand-written from mundane invented sentences, and
`gate_corpus_holds_no_private_dictation` enforces it in CI (ASCII only, no `@`,
no links, no digit runs, and no non-`.jsonl` file may appear in the directory at
all, so a raw dump cannot be dropped in beside the cases).

Two files, 53 cases, four failure classes with the expected outcome on each:

| class | expected | what it reproduces |
|-------|----------|--------------------|
| `real_speech` | `keep` | long unpunctuated run-on cadence — the YV66 false positive (`global_ttr_below_cliff` marks the takes whose WHOLE-take ratio is under the old 0.3 cliff) |
| `legit_repetition` | `keep` | "very very long", emphatics, a quoted chorus — repetition a person actually produces |
| `degenerate` | `reject` | stuck-decoder output: identical-token runs, looped phrases, the glued `SERV-SERV-SERV` token |
| `mixed` | `truncate` | a degenerate tail after real speech — the prefix must survive |

`generated-classes.jsonl` is written by the generator in `tests/gate_corpus.rs`
(`generated_cases`), so the corpus can be regrown and diffed:

```sh
cd desktop/src-tauri
cargo test gate_corpus_regenerate_generated_fixture -- --ignored   # rewrite
cargo test gate_corpus_                                            # check
```

`handwritten-innocuous.jsonl` holds the shapes a generator makes stilted. The
suite asserts PROPERTIES, not per-case goldens: no real-speech case is ever
discarded (`gate_corpus_never_discards_real_speech_class`), and every degenerate
case is truncated or rejected with its audio kept
(`gate_corpus_degenerate_class_truncated_or_rejected_with_audio`, which also pins
the live gate's whole-take arm to `TakeOutcome::Rejected` — YV67's "a rejection
keeps the wav and becomes a Retry row").

The complementary check is local-only and is the one below: the synthetic corpus
proves the behaviour on every class, and only the real transcripts prove the gate
does not misfire on how one particular person talks. After exporting the corpus
with the command in the next section, run

```sh
YAP_PRIVATE_CORPUS=/tmp/transcripts_corpus.txt \
  cargo test gate_corpus_private_corpus -- --ignored --nocapture
```

which prints one line per firing take — line number, token count, cutoff,
verdict — and never a word of the transcript itself.

## `transcripts_corpus.txt` — deliberately NOT committed

YV66 was specified with a real-corpus regression fixture here: one raw transcript
per line, 300+ lines, taken from `select raw_text from transcripts` in the local
Yap database. **It is not in this repo, on purpose.**

This repository is public, and the transcripts table is a verbatim record of
everything its owner has ever dictated. Reviewing the real 356-row corpus turned
up third parties named alongside criminal and sexual allegations, an API-key
handoff, named sales prospects, customer and contract detail, and a lot of
ordinary private speech. No regex scrub makes free-form dictation safe to
publish, and hand-vetting drops the corpus below the 300-line floor anyway — of
356 rows, only ~290 survive even a crude PII filter. So the fixture stays local
and the gate is proved against it locally instead.

The corpus DOES back the YV66 numbers in the PR, and the check is one command to
reproduce on any machine that has the database. Export it:

```sh
sqlite3 "$HOME/Library/Application Support/WilsonVoice/wilson_voice.db" \
  "select replace(replace(raw_text, char(10), ' '), char(13), ' ')
     from transcripts
    where raw_text is not null and length(trim(raw_text)) > 0;" \
  > /tmp/transcripts_corpus.txt
```

Then run `dictation::degenerate_cutoff` over every line and confirm that the only
lines it fires on are genuinely degenerate ASR output. On the 356-row corpus as
of YV66 it fires on exactly four:

| line | tokens | cutoff | what it is |
|------|--------|--------|------------|
| 2 | 219 | 0 | `"we'll move forward and"` looped ~50× — whole take is garbage |
| 5 | 1 | 0 | `WPM-SERV-SERV-SERV…` glued loop (rule 3) |
| 119 | 408 | 257 | good prose, then `"or Instagram"` ×45 — tail cut, prefix kept |
| 157 | 457 | 258 | good prose, then `"and she was a good day"` ×20 — tail cut |

The other 352 transcripts are untouched, including the 933-token take whose
whole-take type/token ratio (0.3022) cleared the old cliff by 0.002. Any
committed replacement for this fixture must be synthesised or consented to
explicitly — do not dump the transcripts table into this repo. YV76 settled that
question: the synthetic `gate/` corpus above is the committed one, permanently,
and this export stays a local check.
