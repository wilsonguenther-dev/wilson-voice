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
explicitly — do not dump the transcripts table into this repo.
