# Wilson Voice — Research & Roadmap

Progress as of 2026-07-17: dictation UI works, SQLite history works, Accessibility granted for Wilson Voice.app, local MLX Whisper transcribes.

## What works today (v0.4.1)

| Layer | Implementation |
|-------|----------------|
| Shell | Tauri 2 + React, single `/Applications` app |
| Hotkey | Carbon `⌘⇧V` hold (Tauri global-shortcut) |
| Mic | In-process **cpal** (TCC identity = Wilson Voice) |
| ASR | MLX Whisper large-v3-turbo on Apple Silicon |
| Paste | CGEvent ⌘V on **main thread** |
| Data | SQLite WAL + FTS5 |
| Pill | Floating always-on-top webview, cursor follow, all Spaces |

## Research notes (how other apps do it)

### Floating “Dictate” pill / HUD
- **Wispr Flow / VoiceInk / Superwhisper**: menubar + optional floating HUD; not a second full app.
- **macOS constraint** ([Tauri #11488](https://github.com/tauri-apps/tauri/issues/11488)): normal `NSWindow` cannot float above *fullscreen* apps. Real solution is **`NSPanel`** with `.nonactivatingPanel`, `collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]`, high window level.
- **Tauri path**: current always-on-top webview is enough for normal desktops/multi-monitor. For fullscreen-over-app quality → [`tauri-nspanel`](https://github.com/ahkohd/tauri-nspanel) next.
- **Cursor follow**: poll `CGEventGetLocation` ~80ms and `set_position` (we do this). Avoid fighting the user’s focus (non-activating).

### Permissions (hardest part — OpenWhispr / community consensus)
- Mic: app process must call capture APIs (`requestAccess` / open stream) so TCC lists the **bundle**, not Python/ffmpeg.
- Accessibility: required for paste/type; **ad-hoc re-sign invalidates trust** → user toggles off/on after each rebuild until Developer ID signing.
- Input Monitoring: only needed for raw modifier-only holds (Right ⌥ alone), not for ⌘⇧V Carbon hotkeys.

### Faster ASR (no Azure)
| Option | Latency | Notes |
|--------|---------|--------|
| **MLX Whisper turbo** (current) | Good | Metal GPU on M-series |
| MLX whisper-small / medium | Faster | Trade accuracy |
| **whisper.cpp** Metal | Very good | VoiceInk / open-wispr style |
| Fine-tune / LoRA on MLX | Best for jargon | Train on Drivia/Wilson vocab offline |
| AWS GPU (RunPod/EC2) | Optional burst | Keep for long batch, not daily dictation (privacy + RTT) |

Kokori remains TTS (speech out). Dictation is STT (speech in) — different stack.

### iOS / cross-device pill
- iOS cannot do global system paste into arbitrary apps the way macOS Accessibility allows (sandbox).
- Patterns: Keyboard extension, Share extension, App Intents, or companion that inserts only inside your apps.
- Cross-device “follow me”: Continuity / iCloud private DB of recent transcripts; not a floating iOS HUD over Safari without private APIs.
- Near-term: Mac-first pill; iOS companion history viewer later.

## Next build slices (ordered)

1. **tauri-nspanel** float for fullscreen apps  
2. **ASR speed**: default profile turbo; one-click “fast” = small; optional whisper.cpp sidecar  
3. **MLX fine-tune path**: dataset of Wilson/Drivia terms → LoRA → local weights folder  
4. **Developer ID + notarization** so TCC survives updates  
5. **iOS companion** (history + push) — not system-wide paste  

## Explicit non-goals right now
- Azure STT  
- Replacing Kokori  
- Kernel extensions / SIP off  
- Fake Desktop `.app` wrappers  
