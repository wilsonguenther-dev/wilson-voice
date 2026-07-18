# Wilson Voice — Stabilization Plan

**Status:** Phases 0–4 **scaffold done** (v0.4.x). Product polish is Phase 5+ — see `STRATEGY.md`.

## Phase checklist

| Phase | Goal | Status |
|-------|------|--------|
| 0 Hygiene | Single `/Applications` app; no DMG; no Desktop wrappers | Done |
| 1 Permissions | AX + mic probes; Settings UI; open Privacy panes | Done |
| 2 Hotkeys | ⌘⇧V hold on main thread after setup | Done |
| 3 Record + paste | cpal mic + main-thread paste; clipboard always | Done |
| 4 Product UI | Home / Permissions / Insights / Dictionary / Scratchpad | Done (v1) |
| 5 Speed | Warm ASR daemon + Fast/Balanced/Max + pipeline_ms | v0.5 partial (streaming still open) |
| 6 HUD | Real NSPanel pill (fullscreen-safe) | Not done |
| 7 Personalize | Correction loop + offline MLX LoRA | Not done |
| 8 Sign | Developer ID + notarize (TCC survives update) | Not done |

## Verified on machine

- Process launches: setup complete, no deadlock (`nextEvent` idle)
- Hotkey log: `registered hold-to-talk ⌘⇧V`
- Install path: **only** `/Applications/Wilson Voice.app`
- Bundle id: `com.wilsonguenther.wilson-voice`
- Version: **0.4.0**
- No Wilson Voice volume in `/Volumes`

## User must still do once (TCC)

System Settings → Privacy & Security — enable for **Wilson Voice** (not Python):

1. **Microphone** (first Record may prompt)
2. **Accessibility** (auto-paste Cmd+V)
3. Optional later: Input Monitoring (future hold-modifier)

Use in-app **Permissions** tab to re-check and open panes.

## Architecture (locked)

- Tauri 2 + React + Rust tray
- SQLite WAL + FTS5
- MLX Whisper sidecar (`.venv` + `python/asr_worker.py`)
- Carbon hotkey via `tauri-plugin-global-shortcut` (⌘⇧V)
- Paste via enigo only if `AXIsProcessTrusted()`

## Rebuild

```bash
~/Desktop/wilson-voice/scripts/rebuild_app.sh
```
