# Yap — Research & Roadmap

Measured at commit `0c5a0f3` (app version 0.9.2, `desktop/src-tauri/tauri.conf.json`).
Every row below names the file that makes it true; if a row and the code disagree,
the code is right and this file is the bug (`desktop/src-tauri/tests/docs_match_the_app.rs`
fails the build on the claims it can check).

## What works today (0.9.2)

| Layer | Implementation | Where |
|-------|----------------|-------|
| Shell | Tauri 2 + React + Rust, one `/Applications/Yap.app`, Dock + menu-bar tray | `tauri.conf.json`, `lib.rs` |
| Hotkey | Hold **fn⌃** (CGEvent tap, `ptt_binding = "fn_control"`); `fn`, `fn / fn⌃`, a dictate-toggle chord, a meeting chord and Esc-to-cancel are configurable; the Carbon ⌘⇧V chord is opt-in (`keep_cmd_shift_v`, off) | `ptt_macos.rs`, `hotkeys.rs`, `hotkey_table.rs` |
| Mic | In-process **cpal**, so TCC attributes the mic to the app's bundle id | `record.rs` |
| VAD | Silero v4 via `vad-rs`, loaded once and kept warm | `vad.rs` (YV36) |
| ASR | In-process GGUF engine (`transcribe-cpp`, Metal) — the ONLY transcriber. Default model Parakeet unified EN 0.6B; Nemotron streaming 0.6B and Whisper tiny are the alternatives in the catalog | `transcription.rs`, `asr_engine.rs`, `src/catalog.json` |
| Long takes | Decoded in windows with seam dedupe; progress events per window | `dictation.rs` (Y3-B), `transcribe_progress.rs` (Y3-C) |
| Cleanup | Rule-based cleanup levels (default `medium`) + optional local-LLM polish in the `yap-polish` sidecar | `dictation.rs`, `polish.rs` (YV10, YV60, Y4-E) |
| Paste | Clipboard + synthesized ⌘V on the **main thread**; secure-input aware | `paste.rs`, `secure_input.rs` |
| Data | SQLite WAL + FTS5 in `~/Library/Application Support/WilsonVoice/` | `db.rs`, `app_paths.rs` |
| Pill | NSPanel (`tauri-nspanel`) over full-screen apps, all Spaces, docked bottom by default | `float_pill.rs` |
| Notetaker | Meeting capture (mic + CoreAudio process tap, two tracks never pre-mixed, macOS 14.4+), timed transcription, summaries in `yap-polish`, calendar read-only | `meeting.rs`, `syscapture.rs`, `meeting_pipeline.rs`, `summarize.rs`, `calendar.rs` |
| Licensing | Offline Ed25519 license, 14-day trial, gate on new dictation only; no network call | `license.rs` (YP2, yap-lic-00) |
| Updates | `tauri-plugin-updater`, two endpoints, notify-then-install on consent | `tauri.conf.json`, `lib.rs` (YV44, UPD-A) |

Not shipped, deliberately: speaker diarization. The `yap-diarize` sidecar is in the
bundle and its handshake is tested, but no meeting is handed to it yet
(`meeting_matrix.rs` header; Y11 gates it).

## Shipped (moved out of "next")

| Was | Shipped as |
|-----|-----------|
| Warm ASR engine + speed profiles | YV31 `TranscriptionManager` warm engine; YV34 native engine becomes the only ASR path (the interpreter-hosted engine was deleted) |
| Streaming partials / chunked decode | YV93 VAD-cut timed transcription; Y3-B windowed long takes; Y3-C progress events |
| `tauri-nspanel` pill for full-screen apps | `float_pill.rs` NSPanel, main-thread dispatch (#8) |
| Developer ID signing + notarization | v0.6.0 notarized DMG; yap-p0-release-smoke signed-app smoke on every release path (#242) |
| DMG + auto-updater | YV8, YV44 consent, YV82 keypair, UPD-A two-endpoint feed |
| Local-LLM cleanup | YV10 levels; YV60 `yap-polish` sidecar (Qwen2.5 GGUF); Y4-E long-form chunked polish |
| Dictionary, snippets, command mode | YV47–YV52 batch (YV48 snippets, YV49 command mode) |
| Notetaker | yap22/yap24 series (two-track capture, sleep survival NT10, untranscribed-audio retention NT1, pipeline guards NT2) |
| Paid license | YP2 offline license core; yap-lic-00 removed the last licensing network call |

## Research notes (how other apps do it)

### Floating “Dictate” pill / HUD
- **Wispr Flow / VoiceInk / Superwhisper**: menubar + optional floating HUD; not a second full app.
- **macOS constraint** ([Tauri #11488](https://github.com/tauri-apps/tauri/issues/11488)): normal `NSWindow` cannot float above *fullscreen* apps. Real solution is **`NSPanel`** with `.nonactivatingPanel`, `collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]`, high window level. (Shipped: `float_pill.rs`.)
- **Cursor follow**: poll `CGEventGetLocation` ~80ms and `set_position`. Continuous chase is OFF in Yap (parked bottom-center); avoid fighting the user’s focus (non-activating).

### Permissions (hardest part — OpenWhispr / community consensus)
- Mic: app process must call capture APIs (`requestAccess` / open stream) so TCC lists the **bundle**, not a helper process.
- Accessibility: required for paste/type; **ad-hoc re-sign invalidates trust** → user toggles off/on after each rebuild unless the build carries a stable signing identity (Developer ID for release, Apple Development locally).
- Input Monitoring: only needed for raw modifier-only holds (fn, fn⌃, Right ⌥ alone), not for Carbon chord hotkeys.

### Faster ASR (no Azure)
| Option | Latency | Notes |
|--------|---------|--------|
| **transcribe-cpp GGUF, Parakeet 0.6B** (current) | Very good | Metal, in-process, no interpreter |
| whisper.cpp Metal | Very good | VoiceInk / open-wispr style; same ggml family |
| Interpreter-hosted Whisper on Apple GPU | Good | Yap's engine through v0.5; removed in YV34 because a fresh Mac could not run it |
| Fine-tune / LoRA (offline) | Best for jargon | Train on Drivia/Wilson vocab offline |
| AWS GPU (RunPod/EC2) | Optional burst | Keep for long batch, not daily dictation (privacy + RTT) |

Kokori remains TTS (speech out). Dictation is STT (speech in) — different stack.

### iOS / cross-device pill
- iOS cannot do global system paste into arbitrary apps the way macOS Accessibility allows (sandbox).
- Patterns: Keyboard extension, Share extension, App Intents, or companion that inserts only inside your apps.
- Cross-device “follow me”: Continuity / iCloud private DB of recent transcripts; not a floating iOS HUD over Safari without private APIs.
- Near-term: Mac-first pill; iOS companion history viewer later.

## Next build slices (ordered)

1. **Speaker diarization reaches a meeting** — wire `diarize::pool()` once Y11's eval defects close
2. **Personalization**: corrections corpus → offline LoRA (local nights; AWS only if dataset is huge)
3. **Platform floor decision** — raise `minimumSystemVersion` to the arm64 reality or build universal (ARCHITECTURE.md "Platform floor"; DEFERRED #7)
4. Optional **cloud STT backend** (Deepgram) if local median still &gt; 800ms — prefer this over raw EC2
5. **iOS companion** (history) — not system-wide paste

## Explicit non-goals right now
- Azure STT  
- Always-on AWS GPU for every short dictate (wrong cost/latency for hold-to-talk)  
- Replacing Kokori  
- Kernel extensions / SIP off  
- Fake Desktop `.app` wrappers  

See **STRATEGY.md** for the full local-vs-AWS decision.
