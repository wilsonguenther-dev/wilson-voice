# Wilson Voice

**Local offline dictation for macOS** — a Wispr Flow / Superwhisper alternative built for developers who talk to Claude Code, Codex, and Grok all day.

- **ASR:** MLX Whisper `large-v3-turbo` (Apple Silicon) with `whisper-cli` fallback  
- **UX:** Menu bar app + **hold-to-talk** global hotkey (default: **Right ⌥ Option**)  
- **Output:** Clipboard + auto-paste into the frontmost app  
- **Privacy:** Audio never leaves your Mac  
- **Ops:** Rotating logs + JSONL event stream for real-time debugging  

## Why not pure FN?

macOS treats **Fn** as a modifier-only key. Global “hold Fn alone” is unreliable without kernel-level remapping. Defaults:

| Config value | Physical key |
|--------------|--------------|
| `right_option` (**default**) | Right ⌥ |
| `right_command` | Right ⌘ |
| `f18` | F18 (map **Fn → F18** in [Karabiner-Elements](https://karabiner-elements.pqrs.org/)) |
| `f13`–`f20` | Function keys |

To make **Fn** work: Karabiner rule *Fn → F18*, then set `hotkey: f18` in config.

## Install

```bash
cd ~/Desktop/wilson-voice
pip3 install -e .
# already have mlx_whisper + models:
#   mlx-community/whisper-large-v3-turbo
```

### Permissions (required once)

1. **Microphone** — Terminal / Python / Wilson Voice  
2. **Accessibility** — same apps (for paste + global hotkey)  
3. **Input Monitoring** — if prompted for pynput  

System Settings → Privacy & Security.

## Run

```bash
# Menu bar + hotkey
python3 -m wilson_voice
# or after install:
wilson-voice

# CLI (file or timed mic)
wilson-voice-cli --seconds 4
wilson-voice-cli /path/to/audio.wav --intent
```

## Config

`~/Library/Application Support/WilsonVoice/config.yaml`

```yaml
model: mlx-community/whisper-large-v3-turbo
language: en
hotkey: right_option
auto_paste: true
auto_copy: true
polish: true
preferred_mic_substrings:
  - Wilson G
  - MacBook Pro Microphone
```

## Logs (extreme diagnostics)

`~/Library/Logs/WilsonVoice/`

| File | Purpose |
|------|---------|
| `wilson-voice.log` | Human-readable rotating log |
| `events.jsonl` | One JSON event per line (`hotkey_press`, `asr_ok`, `exception`, …) |

Menu → **Open logs**, or:

```bash
tail -f ~/Library/Logs/WilsonVoice/wilson-voice.log
tail -f ~/Library/Logs/WilsonVoice/events.jsonl
```

## Architecture (peers studied)

| Project | Stack | Borrowed ideas |
|---------|-------|----------------|
| [VoiceInk](https://github.com/Beingpax/VoiceInk) | Swift + whisper.cpp + Parakeet | Push-to-talk, local models, personal dictionary patterns |
| [Handy](https://github.com/cjpais/Handy) | Tauri/Rust + Whisper/Parakeet + VAD | Subprocess isolation, paste into focused app, tray |
| FreeFlow / Hex | Mac dictation | Hotkey UX, ANE/Parakeet speed |

**Wilson Voice language choice:** Python — so we call **your already-working MLX stack** without re-wrapping Metal. ASR runs in a **subprocess** so model crashes cannot kill the tray.

## Tests

```bash
cd ~/Desktop/wilson-voice
python3 -m pytest tests/ -q
python3 scripts/qa_battery.py   # ~50 automated checks + live ASR samples
```

## License

MIT
