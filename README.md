# Wilson Voice

**Product-grade local dictation for macOS** — a Wispr Flow / Superwhisper alternative.

Not a pile of Python scripts. A real desktop app:

- **Native shell:** Tauri 2 + React + Rust (`com.wilsonguenther.wilson-voice`)
- **Menu bar tray** + **floating Dictate pill** (Wispr-style always-on-top control)
- **SQLite WAL + FTS5** for transcript history, dictionary, insights, scratchpad
- **Local ASR:** MLX Whisper (`large-v3-turbo`) via Python sidecar — audio never leaves your Mac
- **OS paste:** clipboard + simulated ⌘V into the frontmost app
- **UI:** Home · Insights · Dictionary · Scratchpad · Settings

See [PRODUCT.md](./PRODUCT.md) for architecture research (Handy, VoiceInk, OpenWhispr, Muesli, sflow).

## Install (desktop app)

```bash
cd ~/Desktop/wilson-voice/desktop
npm install
# one-time: arm64 venv for ASR
cd .. && python3.13 -m venv .venv && .venv/bin/pip install mlx-whisper
cd desktop && npm run desktop:build
open "src-tauri/target/release/bundle/macos/Wilson Voice.app"
# or copy to Applications:
# cp -R "src-tauri/target/release/bundle/macos/Wilson Voice.app" /Applications/
```

### Permissions (once)

System Settings → Privacy & Security — grant **Wilson Voice** (not “Python”):

1. **Microphone**
2. **Accessibility** (paste)
3. **Input Monitoring** (global hotkeys)

## Use

| Action | How |
|--------|-----|
| Dictate | Hold **⌥Space** or **⌘⇧V**, or click floating **Dictate** / tray |
| History | Home tab — search is FTS5 over SQLite |
| Re-paste | Copy / Paste on any card |
| Dictionary | Add terms so ASR rewrites jargon correctly |
| Insights | Words, WPM, streak, 7-day chart |
| Quit | Tray → Quit (closing the window hides to tray) |

## Data

```
~/Library/Application Support/WilsonVoice/
  wilson_voice.db      # SQLite (WAL)
  settings.json
  recordings/          # temp WAVs
```

## Dev

```bash
cd ~/Desktop/wilson-voice/desktop
npm run desktop:dev
```

## Legacy Python package

`wilson_voice/` remains as the ASR library / CLI used by the desktop sidecar (`python/asr_worker.py`). The **product UI is the Tauri app**, not `python -m wilson_voice`.

## License

MIT — see [LICENSE](./LICENSE).
