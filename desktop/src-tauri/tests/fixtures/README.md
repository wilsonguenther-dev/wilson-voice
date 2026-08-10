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
