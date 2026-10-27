# Yap

**Product-grade local dictation for macOS** — a Wispr Flow / Superwhisper alternative,
plus a meeting notetaker. Audio is transcribed on your Mac.

A real desktop app:

- **Native shell:** Tauri 2 + React + Rust, one app (`Yap.app`), menu-bar tray + Dock
- **Floating Dictate pill:** an NSPanel that floats over full-screen apps on every Space
- **Local ASR:** an in-process GGUF engine (`transcribe-cpp`, Metal) — no interpreter, no helper process
- **SQLite WAL + FTS5** for transcript history, dictionary, snippets, insights, scratchpad, meetings
- **OS paste:** clipboard + simulated ⌘V into the frontmost app
- **UI:** Home · Insights · Dictionary · Scratchpad · Meetings · Settings

See [PRODUCT.md](./PRODUCT.md) for the feature list, [ARCHITECTURE.md](./ARCHITECTURE.md)
for decisions, runtime dependencies and the platform floor, and [ROADMAP.md](./ROADMAP.md)
for what shipped and what is next.

## The two names that did NOT change — and must not

The product was called **Wilson Voice** before it was called **Yap**. Two identifiers
still carry the old name, **on purpose**, and renaming either one is irreversible
damage, not a tidy-up:

- **Bundle identifier `com.wilsonguenther.wilson-voice`.** macOS keys the
  Microphone, Accessibility and Input Monitoring grants (TCC) to the bundle id.
  Renaming it makes every existing install a brand-new app to macOS: all three
  grants are lost and dictation stops until the user re-grants them by hand.
- **Data directory `~/Library/Application Support/WilsonVoice/`.** It holds the
  SQLite history (`wilson_voice.db`), settings, models and meeting audio.
  Renaming it orphans every existing user's history — the app would start empty
  and the old data would sit unreferenced on disk.

The display name (`productName`), window titles and every user-visible string say
Yap. These two do not, and that is correct.

## Install (desktop app)

Download the notarized DMG from the site, or build it:

```bash
cd desktop
npm ci
npm run desktop:build      # builds both sidecars, the frontend and the app + DMG
open "src-tauri/target/release/bundle/macos/Yap.app"
```

On first run the in-app model manager downloads the speech model (sha256-verified);
nothing else needs installing. See ARCHITECTURE.md → Runtime Dependencies.

### Permissions (once)

System Settings → Privacy & Security — grant **Yap**:

1. **Microphone**
2. **Accessibility** (paste)
3. **Input Monitoring** (the fn-key hold)

Meeting notes additionally ask for system-audio capture (macOS 14.4+) and, optionally,
read-only Calendar access.

## Use

**Default dictation key:** hold **fn⌃** (`ptt_binding = "fn_control"`).

| Action | How |
|--------|-----|
| Dictate | Hold the dictation key (fn, fn⌃ or either — Settings), or click the floating **Dictate** pill / tray. The Carbon ⌘⇧V chord is optional and off by default |
| Cancel | Esc while a take is live |
| History | Home tab — search is FTS5 over SQLite |
| Re-paste | Copy / Paste on any card |
| Dictionary | Add terms so ASR rewrites jargon correctly |
| Snippets | Spoken trigger phrases expand to saved text |
| Meetings | Record a call or an in-person meeting; transcript + summary stay local |
| Insights | Words, WPM, streak, charts |
| Quit | Tray → Quit (closing the window hides to tray) |

## Data

```
~/Library/Application Support/WilsonVoice/     # frozen name — see above
  wilson_voice.db      # SQLite (WAL)
  settings.json
  models/              # downloaded ASR / polish / VAD / diarization models
  recordings/          # temp WAVs
  meetings/            # meeting audio (kept until its transcript exists)
  logs/
```

`YAP_DATA_DIR` relocates the whole root for test and agent launches (ARCHITECTURE.md);
it is an override, never a rename.

**At-rest storage:** transcripts and history live in a plain local **SQLite**
database (`wilson_voice.db`); the app does not encrypt it itself and relies on
macOS **FileVault** full-disk encryption for at-rest protection — enable FileVault
if the transcript history is sensitive.

## Dev

```bash
cd desktop
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

## One app, two sidecars

Yap is a single Tauri app (`desktop/`): a menu-bar–resident window
(Home / Insights / Dictionary / Scratchpad / Meetings / Settings) plus an always-on-top Dictate pill.
On-device ASR runs **inside the app binary** — a native GGUF engine
(`transcribe-cpp`, Metal) driven by `src-tauri/src/transcription.rs`. There is
no interpreter and no virtual environment: audio is decoded in-process
(no `ffmpeg` binary) and the speech model is downloaded once from the in-app
model manager.

There are two helper processes, both bundled as Tauri sidecars and both spoken
to over newline-delimited JSON on stdio — no port, no listener, no network.

**`yap-polish`** (`desktop/yap-polish/`, YV60) is the optional local-LLM cleanup
stage. It is a separate binary for a concrete reason: the app already statically
links `transcribe-cpp`'s vendored `ggml`, and `llama-cpp-2` vendors its own, so
the two cannot share a link line. The split also makes the polish deadline
*hard* — a late answer is dropped and the child killed. Its model is **not**
bundled; with none installed the sidecar is never spawned and dictation behaves
exactly as it does without it.

**`yap-diarize`** (`desktop/yap-diarize/`, YV121/YV122) is the speaker-diarization
stage, out of process for the identical reason one level down: the app already
links onnxruntime statically through `vad-rs`/`ort`, and `sherpa-onnx` links its
own vendored copy. It takes no model on argv — models arrive as a `load_models`
request — so its readiness handshake announces a *process*, not a model. It is
bundled and its handshake is tested, but no meeting is handed to it yet.

`desktop/` is a Cargo workspace (`src-tauri` + `yap-polish` + `yap-diarize`),
and `bundle.externalBin` makes the staged sidecars a precondition of building
the app, so build them first:

```bash
cd desktop
npm run sidecar     # builds both sidecars --release, staged under the host triple
```

`npm run desktop:dev` and `npm run desktop:build` already run it for you.

> The former standalone `wilson_voice/` rumps/pynput menu-bar app was removed in
> favour of this single Tauri app (recoverable from git history). Nothing in the
> shipping product imports it.

## License

Yap is a **paid, proprietary** app. It was open source through the **v0.7.0
release** (tag `v0.7.0`, commit `8c8740f`): that release and every release
before it stay under the Apache License 2.0 in [LICENSE](./LICENSE) for
everyone who received them, permanently.

Everything committed **after** that release — including the licensing, trial
and payment code — is proprietary and is **not** covered by the `LICENSE` file.
Cloning this repository does not grant you rights over it. See
[NOTICE](./NOTICE) for the cut-off and [TERMS.md](./TERMS.md) for the terms
that do apply.
