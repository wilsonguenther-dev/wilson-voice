# Wake command: **back to whisper**

Say: **back to whisper** (or paste the resume block below).

## North star
Own local dictation. Replace cloud STT subscriptions. Honest stack: **OpenAI Whisper weights via MLX on-device** — not “Wilson-trained foundation models.” Personalization = vocab/dictionary from history; full LoRA fine-tune is future work.

## Current truth (as of last session)
- App: `/Applications/Wilson Voice.app` · bundle `com.wilsonguenther.wilson-voice`
- Stack: Tauri 2 + React + Rust · SQLite WAL + FTS5 · warm `asr_worker.py --serve`
- **Must have** `tauri` feature `custom-protocol` or UI is blank white (loads localhost:1420)
- Sign with **Apple Development: Wilson Guenther** so Mic TCC sticks (never ad-hoc if avoidable)
- Modes = Whisper **scales**, not proprietary models:
  - Fast → `mlx-community/whisper-small-mlx` (~244M)
  - Balanced → `mlx-community/whisper-large-v3-turbo` (~809M) DEFAULT
  - Max → `mlx-community/whisper-large-v3-mlx` (~1.5B)
- ASR paths only under `~/Library/Application Support/WilsonVoice/` (never Desktop)

## Resume agenda (priority order)
1. **Latency** — warm daemon reliability; Fast profile default under load; streaming/partials; measure p50 hold→clipboard
2. **Mic UX** — zero re-prompt after Allow; keep stable codesign; never reintroduce ad-hoc spam
3. **SQL / insights math** — speech_seconds WPM, daily_stats recompute, streak/today correctness; real-time refresh; no bad heuristics
4. **Data collection** — clean transcript schema, speech duration, source app, export; no silent drift
5. **Retrieval later** — FTS5 first (already partial); decide RAG only if dictionary + FTS insufficient; GraphQL is overkill for single-user local
6. **Architecture decision** — modern zero-day-minded path: local-first, minimal attack surface, no cloud STT in hot path, optional offline LoRA only
7. **HUD** — real NSPanel pill (not glitchy webview chase)
8. **Honest product language** — never claim “my Whisper” when base is OpenAI Whisper + MLX

## Explicit non-goals for next wake
- Always-on AWS GPU for every ⌘⇧V
- Fake “three Wilson foundation models”
- GraphQL server for a desktop SQLite app

## Repo
`~/Desktop/wilson-voice` · install: `scripts/rebuild_app.sh`
