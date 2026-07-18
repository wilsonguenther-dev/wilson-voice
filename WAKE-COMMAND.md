# Wake command: **back to whisper**

Say: **back to whisper** (or paste the resume block below).

## North star
Own local dictation. Replace cloud STT subscriptions. Honest stack: **OpenAI Whisper weights via MLX on-device** — not “Wilson-trained foundation models.” Personalization = vocab/dictionary from history; full LoRA fine-tune is future work.

## Current truth (as of 2026-07-18 / v0.5.0)
- App: `/Applications/Wilson Voice.app` · bundle `com.wilsonguenther.wilson-voice`
- Stack: Tauri 2 + React + Rust · SQLite WAL + FTS5 · warm `asr_worker.py --serve`
- **Must have** `tauri` feature `custom-protocol` or UI is blank white (loads localhost:1420)
- Sign with **Apple Development: Wilson Guenther** so Mic TCC sticks (never ad-hoc if avoidable)
- Modes = Whisper **scales**, not proprietary models:
  - Fast → `mlx-community/whisper-small-mlx` (~244M)
  - Balanced → `mlx-community/whisper-large-v3-turbo` (~809M) DEFAULT
  - Max → `mlx-community/whisper-large-v3-mlx` (~1.5B)
- ASR paths only under `~/Library/Application Support/WilsonVoice/` (never Desktop)
- **Shipped v0.5:** pipeline_ms hold→clipboard telemetry; speech_seconds from WAV duration; WPM excludes asr fallback; source_app; export; ModelHolder preload + temp=0 decode; ARCHITECTURE.md locked
- See `ARCHITECTURE.md` — FTS before RAG; no GraphQL; local-first

## Resume agenda (priority order)
1. **Latency** — measure p50 on device after warm; Fast under Metal load; streaming/partials next
2. **Mic UX** — zero re-prompt after Allow; keep stable codesign; never reintroduce ad-hoc spam
3. **SQL / insights** — mostly fixed (speech-only WPM, daily_stats, streaks); verify live after dictate
4. **Data collection** — schema clean; wav deleted post-ASR; export in Settings
5. **Retrieval later** — FTS5 first; RAG only if dictionary + FTS insufficient
6. **Architecture** — locked in ARCHITECTURE.md (do not re-open)
7. **HUD** — real NSPanel pill (not glitchy webview chase)
8. **Honest product language** — never claim “my Whisper” when base is OpenAI Whisper + MLX

## Explicit non-goals for next wake
- Always-on AWS GPU for every ⌘⇧V
- Fake “three Wilson foundation models”
- GraphQL server for a desktop SQLite app

## Repo
`~/Desktop/wilson-voice` · install: `scripts/rebuild_app.sh`
