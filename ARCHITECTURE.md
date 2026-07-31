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

## Attack surface (zero-day-minded)

Minimize:

- No network STT in the hot path
- No Desktop path execution for Python/worker
- No GraphQL / remote query server
- Audio deleted after successful ASR (optional keep: user export)
- Secrets never in repo or Obsidian

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

## Explicit non-goals

- Always-on AWS GPU for short dictation
- Fake “three Wilson models”
- GraphQL for local SQLite
- Replacing Kokori (TTS ≠ STT)
- Re-litigating Desktop path / TCC identity rules
