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
