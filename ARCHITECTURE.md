# Wilson Voice — Architecture Decision (locked 2026-07-18)

## Product

Local-first hold-to-talk dictation for macOS. Replace cloud STT subscriptions.
**Honest stack language:** OpenAI Whisper weights via MLX on-device — not three
proprietary “Wilson foundation models.” Modes = scale/speed profiles.

## Decisions (do not re-litigate without a measured reason)

| Decision | Choice | Why |
|----------|--------|-----|
| Shell | Tauri 2 + React + Rust | Native tray, hotkey, Accessibility, small surface |
| Production embed | `custom-protocol` feature **required** | Without it: blank white UI (loads localhost:1420) |
| Signing | Apple Development: Wilson Guenther | Mic TCC sticks across rebuilds; ad-hoc re-prompts |
| Mic identity | In-process cpal in the app binary | TCC attributes to bundle id, not a helper process |
| ASR paths | `~/Library/Application Support/WilsonVoice/` only | Desktop path = Files & Folders spam every stop |
| ASR runtime | In-process GGUF engine (`transcribe-cpp`, Metal) kept warm by `TranscriptionManager` | No sidecar to spawn, no IPC, no venv to bootstrap on a user's Mac |
| Models | Bundled catalog, downloaded on demand from the in-app model manager | One selector (`nativeModel`); size is the speed/accuracy dial |
| Personalization now | Dictionary + vocab harvest from history | Cheap; no GPU train required |
| Personalization later | Offline LoRA on corrections corpus | Nights only; optional remote worker for train |
| Data plane | SQLite WAL + FTS5 | Single-user local; typed Tauri commands |
| Retrieval | **FTS5 first** | No RAG until dictionary + FTS proven insufficient |
| GraphQL | **No** | Overkill for single-user desktop SQLite |
| Cloud STT hot path | **No** | Local-first, zero-day-minded surface |
| AWS always-on GPU | **No** for every short dictate | Network loses to warm local for 2–8s holds |
| Primary hotkey | **fn / Globe hold** (CGEvent tap) | Carbon cannot bind bare FN; Accessibility required |
| Secondary hotkey | Optional ⌘⇧V Carbon + optional fn⌃ | Settings toggles |
| HUD | Parked bottom-center pill; show on record | Continuous cursor chase OFF; full `tauri-nspanel` next |
| WPM | `speech_seconds` from audio duration only | Never use model `asr_seconds` as speaking time |
| Latency metric | Release → clipboard ms (`pipeline_ms`) | North star p50 &lt; 800ms on Fast |

## Data schema (source of truth)

- `transcripts` — text, backend, `asr_seconds`, `speech_seconds`, `pipeline_ms`, `word_count`, `source_app`, `created_at`
- `transcripts_fts` — FTS5 content-sync triggers
- `dictionary` — preferred rewrites + learned tokens
- `daily_stats` — recomputed from transcripts (never trust counters alone)
- `scratchpad`, `settings_kv`
- `license_state` — YP2. Two integers (trial start, highest wall clock ever seen) that CORROBORATE `license.json`. Deliberately its own table so nothing that resets or clears settings can reach it. It holds no verdict: whether the app is licensed is recomputed from the Ed25519 signature on every read.

## Attack surface (zero-day-minded)

Minimize:

- No network STT in the hot path
- No Desktop path execution for Python/worker
- No GraphQL / remote query server
- Audio deleted after successful ASR (optional keep: user export)
- Secrets never in repo or Obsidian
- Licensing (YP2) adds exactly one host to the app's network surface: a background GET of the issuer's public revocation list. No activation call, no telemetry, no license check-in. Every failure of that fetch is a no-op — an offline Mac keeps the entitlement it had.
- The pinned issuer key in `src/license.rs` is a PUBLIC key; the signing half is root-owned, mode 0400, on the Forge box and is captured into the encrypted off-box backup. `tests/license_gate.rs` fails the build if any signing primitive appears in the shipped half of that module.
- The license gate is narrow ON PURPOSE and enforced by a test that reads `lib.rs`: only starting a NEW dictation can be refused. History, exports, settings and every other surface work forever, licensed or not.

Optional later: Developer ID + notarization so TCC survives distribution updates.

## Speed stack

```
warm daemon + ModelHolder cache
  + temperature=0 decode
  + Fast profile under Metal pressure
  + measured pipeline_ms
→ latency

dictionary + (future) LoRA
→ accuracy / jargon
```

## Runtime Dependencies

Nothing here is "found on the machine". Every row is either shipped in the
bundle or supplied by the caller, and the two environment rows below are the
contract every automated launch must honour.

| Dependency | How it is supplied | Default when absent | Notes |
|---|---|---|---|
| `YAP_DATA_DIR` (env) | Caller exports it before launch | `<Application Support>/WilsonVoice` — the shipped behaviour, unchanged | Relocates the app's ENTIRE state root: SQLite history, `settings.json`, `models/`, `recordings/`, `recovery/`, `meetings/`, `logs/`. Resolved ONCE per process (`src/app_paths.rs`) and then frozen, so it cannot move the history out from under an open SQLite connection. It is an OVERRIDE, never a rename — the default directory name is frozen because renaming it orphans every existing install's history. |
| `--smoke` (argv) | Caller passes the flag | Normal GUI launch | A launch an automated agent may make. It registers NO global hotkey (no CGEvent PTT tap, no ⌃⌘V / ⌃⌘M / ⌃⌘Z / ⌘⇧V), synthesizes NO ⌘V and no Delete keystroke, and **refuses to start** — exiting non-zero with a named `SMOKE_REFUSED_*` message on stderr — when `YAP_DATA_DIR` is unset or resolves to the default root. It never degrades into using the real data dir. |
| ASR / diarization models (GGUF, ONNX) | Downloaded once, sha256-verified, into `<state root>/models` | none — first run fetches | Moves with `YAP_DATA_DIR`, so two concurrent launches do not share one multi-gigabyte dir. |
| `yap-polish`, `yap-diarize` sidecars | Bundled via `bundle.externalBin`, staged at `src-tauri/binaries/<name>-<triple>` | none — a cargo build of the app FAILS without them | Build with `cargo build -p yap-polish --release` (and `-p yap-diarize`), then copy into `binaries/`. |
| macOS TCC grants (Microphone, Accessibility, Input Monitoring) | Granted by the user, keyed to the bundle identifier | none — the app degrades to clipboard-only | The bundle id is irreversible: renaming it resets all three grants. |

**Run contract — no agent launches the app without `YAP_DATA_DIR`.**

The harness isolates working directories, cargo target dirs and preview ports.
It does not isolate the app's state. Two concurrent launches plus the installed
copy would otherwise read and write one SQLite history, one settings store and
one models dir, all register the same global PTT binding, and all synthesize a
paste into whatever window happens to be frontmost. So:

```bash
export YAP_DATA_DIR="$(mktemp -d)/yap-state"
npm run desktop:dev            # or the built binary
# automated / unattended runs additionally pass:
#   wilson-voice --smoke
```

`--smoke` is the enforcement: it is the only launch mode that cannot silently
fall back to the real dictation history, because it exits non-zero instead.

## Explicit non-goals

- Always-on AWS GPU for short dictation
- Fake “three Wilson models”
- GraphQL for local SQLite
- Replacing Kokori (TTS ≠ STT)
- Re-litigating Desktop path / TCC identity rules
