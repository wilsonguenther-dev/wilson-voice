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

**At-rest storage:** transcripts and history live in a plain local **SQLite**
database (`wilson_voice.db`); the app does not encrypt it itself and relies on
macOS **FileVault** full-disk encryption for at-rest protection — enable FileVault
if the transcript history is sensitive.

## Dev

```bash
cd ~/Desktop/wilson-voice/desktop
npm run desktop:dev
```

### Build & signing

`tauri.conf.json` commits **ad-hoc signing** (`bundle.macOS.signingIdentity: "-"`),
so `npm run tauri build` works on any machine — no certificate, no keychain, and
the `Entitlements.plist` still gets applied to the bundle. An ad-hoc signature
changes on every rebuild, so macOS re-asks for Microphone / Accessibility /
Input Monitoring after each local build; that is expected for a dev build.

Real distribution signing comes from the environment and is never committed:

```bash
export APPLE_SIGNING_IDENTITY="Developer ID Application: … (TEAMID)"
npm run tauri build
```

`APPLE_SIGNING_IDENTITY` overrides the value in `tauri.conf.json`, so the release
workflow (which also sets `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID` for
notarization) signs with the Developer ID cert without any config change.

## One app, no sidecar

Wilson Voice is a single Tauri app (`desktop/`): a menu-bar–resident window
(Home / Insights / Dictionary / Settings) plus an always-on-top Dictate pill.
On-device ASR runs **inside the app binary** — a native GGUF engine
(`transcribe-cpp`, Metal) driven by `src-tauri/src/transcription.rs`. There is
no interpreter, no virtual environment, and no helper process: audio is decoded
in-process (no `ffmpeg` binary) and the speech model is downloaded once from the
in-app model manager.

> The former standalone `wilson_voice/` rumps/pynput menu-bar app was removed in
> favour of this single Tauri app (recoverable from git history). Nothing in the
> shipping product imports it.

## License

MIT — see [LICENSE](./LICENSE).
