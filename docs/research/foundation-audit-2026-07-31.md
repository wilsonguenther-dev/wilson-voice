# Foundation Audit — 2026-07-31 (8-agent workflow)

Structured findings: `foundation-audit-2026-07-31.json` (same dir). Narrative synthesis:
`~/Obsidian/Wilson-Brain/Notes/Yap-Foundation-Audit-2026-07-31.md`. Donor repo:
`~/oss/handy` (cjpais/Handy v0.9.4).

## Headline

The fresh-Mac "infinite loading" install failure is architectural: the .app bundles NO ASR
runtime (no Python, no model — tauri.conf.json has no resources/externalBin), the python
resolver accepts the non-functional /usr/bin/python3 CLT shim so the UI claims "Ready", and
onboarding calibration's `busy` state is only cleared by a success event while the Rust error
path emits nothing the UI listens to (Onboarding.tsx:83-128, lib.rs:641-645). Compounding:
sync main-thread `pip install` with no timeout, and a lazy 1.6 GB HF model download inside the
first transcribe call with an untimed one-shot fallback (asr.rs:399-401).

## Decision

Kill the Python/MLX sidecar. Port Handy's fully-embedded stack: transcribe-cpp (GGUF + Metal),
compiled-in model catalog with pinned revisions + sha256, resumable verified downloader with
progress events, warm-engine TranscriptionManager, onboarding model-download gate. Ship zero
models in the bundle. Encoded as CI/CD batch **yap11** (YV30–YV34) in `scripts/cicd-loop.mjs`.

Follow-up batches: yap12 latency (persistent cpal stream, load-once VAD — Silero currently
fails to load on EVERY recording (155× in yap.log) and falls back to energy VAD; inline DSP;
kill fixed sleeps; receipt-sequenced paste), yap13 robustness (settings salvage — one bad field
currently resets ALL settings; single-instance; autostart; Secure-Input watchdog; updater chain
is broken end-to-end + auto-installs without consent; CSP; signing-identity hygiene), yap14
Wispr parity (command mode, snippets, auto-learning dictionary, cursor-context awareness,
cleanup levels — full portable list in the JSON under `wispr-changelog`).

## Keep-ours (do not regress during ports)

Yappy companion/personality, local Insights analytics, smart per-app dictation modes,
filler/backtrack cleanup, FTS history. Handy has none of these.
