# Wake command: **back to whisper**

Say: **back to whisper** (or paste the resume block below).

## North star
Own local dictation. Replace cloud STT subscriptions. Honest stack: **OpenAI Whisper weights via MLX on-device** — not “Wilson-trained foundation models.” Personalization = vocab/dictionary from history; full LoRA fine-tune is future work.

## Current truth (as of 2026-07-18 / v0.5.1)
- App: `/Applications/Wilson Voice.app` · bundle `com.wilsonguenther.wilson-voice`
- Stack: Tauri 2 + React + Rust · SQLite WAL + FTS5 · warm `asr_worker.py --serve`
- **Must have** `tauri` feature `custom-protocol` or UI is blank white (loads localhost:1420)
- Sign with **Apple Development: Wilson Guenther** so Mic TCC sticks (never ad-hoc if avoidable)
- Modes = Whisper **scales**, not proprietary models (Fast/Balanced/Max)
- ASR paths only under `~/Library/Application Support/WilsonVoice/` (never Desktop)
- **PTT:** primary **fn / Globe hold** via CGEvent tap (`ptt_macos.rs`); optional **fn⌃** and secondary ⌘⇧V
  - Needs Accessibility; set Keyboard → Press 🌐 → **Do Nothing**
  - Carbon cannot bind bare FN (global-hotkey #111)
- **HUD:** float.html pill parks bottom-center of screen-under-cursor; shows on record; hardened floating level + fullScreenAuxiliary (not continuous cursor chase). Full `tauri-nspanel` convert is next polish.
- See `ARCHITECTURE.md`

## Resume agenda (priority order)
1. **Live-test FN hold** + Accessibility + Globe “Do Nothing”
2. **Latency** — p50 hold→clipboard on Fast after warm
3. **HUD** — true `tauri-nspanel` PanelBuilder (research done; harden path shipped)
4. Streaming/partials while holding
5. Developer ID + notarize

## Explicit non-goals for next wake
- Always-on AWS GPU for every ⌘⇧V
- Fake “three Wilson foundation models”
- GraphQL server for a desktop SQLite app

## Repo
`~/Desktop/wilson-voice` · install: `scripts/rebuild_app.sh`
