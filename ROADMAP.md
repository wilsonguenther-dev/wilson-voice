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

1. **Warm ASR daemon + speed profiles** (shipped 2026-07-17) — biggest latency win without AWS  
2. **Streaming partials** while holding (chunked / VAD)  
3. **tauri-nspanel** real Dictate pill for fullscreen  
4. **MLX fine-tune path**: corrections corpus → offline LoRA (local nights; AWS only if dataset is huge)  
5. **Developer ID + notarization** so TCC survives updates  
6. Optional **cloud STT backend** (Deepgram) if local median still &gt; 800ms — prefer this over raw EC2  
7. **iOS companion** (history) — not system-wide paste  

## Explicit non-goals right now
- Azure STT  
- Always-on AWS GPU for every short dictate (wrong cost/latency for hold-to-talk)  
- Replacing Kokori  
- Kernel extensions / SIP off  
- Fake Desktop `.app` wrappers  

See **STRATEGY.md** for the full local-vs-AWS decision.
