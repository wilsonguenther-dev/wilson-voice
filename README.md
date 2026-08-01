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

## One app, one sidecar

Wilson Voice is a single Tauri app (`desktop/`): a menu-bar–resident window
(Home / Insights / Dictionary / Settings) plus an always-on-top Dictate pill.
On-device ASR runs **inside the app binary** — a native GGUF engine
(`transcribe-cpp`, Metal) driven by `src-tauri/src/transcription.rs`. There is
no interpreter and no virtual environment: audio is decoded in-process (no
`ffmpeg` binary) and the speech model is downloaded once from the in-app model
manager.

The one helper process is **`yap-polish`** (`desktop/yap-polish/`, YV60): the
optional local-LLM cleanup stage, bundled as a Tauri sidecar and spoken to over
newline-delimited JSON on stdio — no port, no listener, no network. It is a
separate binary for a concrete reason: the app already statically links
`transcribe-cpp`'s vendored `ggml`, and `llama-cpp-2` vendors its own, so the
two cannot share a link line. The split also makes the polish deadline *hard* —
a late answer is dropped and the child killed. Its model is **not** bundled;
with none installed the sidecar is never spawned and dictation behaves exactly
as it does without it.

`desktop/` is a Cargo workspace (`src-tauri` + `yap-polish`), and
`bundle.externalBin` makes the staged sidecar a precondition of building the
app, so build it first:

```bash
cd ~/Desktop/wilson-voice/desktop
npm run sidecar     # cargo build -p yap-polish --release, staged under its triple
```

`npm run desktop:dev` and `npm run desktop:build` already run it for you.

> The former standalone `wilson_voice/` rumps/pynput menu-bar app was removed in
> favour of this single Tauri app (recoverable from git history). Nothing in the
> shipping product imports it.

## License

MIT — see [LICENSE](./LICENSE).
