# Yap — Architecture Decision (locked 2026-07-18, re-measured by Y6-E)

**App version:** `0.9.2` — `desktop/src-tauri/tauri.conf.json`; `tests/docs_match_the_app.rs`
fails when this line and the config disagree, so a release bumps it with the config.

## Product

Local-first hold-to-talk dictation and meeting notes for macOS. Replace cloud STT subscriptions.
**Honest stack language:** open ASR weights (Parakeet 0.6B by default; Nemotron 0.6B
and Whisper tiny selectable) run on-device by an in-process GGUF engine
(`transcribe-cpp`, Metal) — not proprietary “Wilson foundation models.” Model size
is the speed/accuracy dial.

## Decisions (do not re-litigate without a measured reason)

| Decision | Choice | Why |
|----------|--------|-----|
| Shell | Tauri 2 + React + Rust | Native tray, hotkey, Accessibility, small surface |
| Production embed | `custom-protocol` feature **required** | Without it: blank white UI (loads localhost:1420) |
| Signing | Apple Development locally; Developer ID + notarization for release | TCC grants survive rebuilds and updates only with a stable identity; ad-hoc re-prompts |
| Mic identity | In-process cpal in the app binary | TCC attributes to bundle id, not a helper process |
| ASR paths | `~/Library/Application Support/WilsonVoice/` only | Desktop path = Files & Folders spam every stop |
| ASR runtime | In-process GGUF engine (`transcribe-cpp`, Metal) kept warm by `TranscriptionManager` | No sidecar to spawn, no IPC, no interpreter to bootstrap on a user's Mac |
| Models | Bundled catalog (`src/catalog.json`), downloaded on demand from the in-app model manager, sha256-pinned | One selector (`nativeModel`); size is the speed/accuracy dial |
| Personalization now | Dictionary + vocab harvest from history | Cheap; no GPU train required |
| Personalization later | Offline LoRA on corrections corpus | Nights only; optional remote worker for train |
| Data plane | SQLite WAL + FTS5 | Single-user local; typed Tauri commands |
| Retrieval | **FTS5 first** | No RAG until dictionary + FTS proven insufficient |
| GraphQL | **No** | Overkill for single-user desktop SQLite |
| Cloud STT hot path | **No** | Local-first, zero-day-minded surface |
| AWS always-on GPU | **No** for every short dictate | Network loses to warm local for 2–8s holds |
| Primary hotkey | **fn⌃ hold** by default (`ptt_binding = "fn_control"`), fn or either selectable (CGEvent tap) | Carbon cannot bind bare fn; Input Monitoring required |
| Secondary hotkey | Optional ⌘⇧V Carbon + optional fn⌃ | Settings toggles |
| HUD | Parked pill (bottom / left / right dock) on a screen that exists; hidden until onboarded | Already an `NSPanel` via `tauri-nspanel` — see FULLSCREEN below |
| WPM | `speech_seconds` from audio duration only | Never use model `asr_seconds` as speaking time |
| Latency metric | Release → clipboard ms (`pipeline_ms`) | North star p50 &lt; 800ms on Fast |

## Data schema (source of truth)

- `transcripts` — text, backend, `asr_seconds`, `speech_seconds`, `pipeline_ms`, `word_count`, `source_app`, `created_at`
- `transcripts_fts` — FTS5 content-sync triggers
- `dictionary` — preferred rewrites + learned tokens
- `daily_stats` — recomputed from transcripts (never trust counters alone)
- `scratchpad`, `settings_kv`
- `license_state` — YP2. Two integers (trial start, highest wall clock ever seen) that CORROBORATE `license.json`. Deliberately its own table so nothing that resets or clears settings can reach it. It holds no verdict: whether the app is licensed is recomputed from the Ed25519 signature on every read.

## Attack surface (zero-day-minded)

Minimize:

- No network STT in the hot path
- No Desktop path execution for Python/worker
- No GraphQL / remote query server
- Audio deleted after successful ASR (optional keep: user export)
- Secrets never in repo or Obsidian
- Licensing (YP2) makes NO network call. yap-lic-00 deleted the launch-time GET of the revocation list (its host was decommissioned); `license.rs` contacts no host. No activation call, no telemetry, no license check-in.
- The app's whole network surface is two calls: the model download (Hugging Face + the catalog mirror for ASR/polish/diarization models; github.com for the pinned Silero VAD file), and the updater check (`plugins.updater.endpoints` in `tauri.conf.json`; `check_updates`, default on, install only on consent). Opening the purchase page hands a URL to the browser; the app itself sends nothing.
- The pinned issuer key in `src/license.rs` is a PUBLIC key; the signing half is root-owned, mode 0400, on the Forge box and is captured into the encrypted off-box backup. `tests/license_gate.rs` fails the build if any signing primitive appears in the shipped half of that module.
- The license gate is narrow ON PURPOSE and enforced by a test that reads `lib.rs`: only starting a NEW dictation can be refused. History, exports, settings and every other surface work forever, licensed or not.

Shipped: Developer ID signing + notarization (v0.6.0 onward), with a signed-app smoke on every release path (`docs/RELEASE.md`).

## Speed stack

```
warm in-process engine (TranscriptionManager)
  + load-once Silero VAD
  + windowed decode for long takes
  + measured pipeline_ms
→ latency

dictionary + (future) LoRA
→ accuracy / jargon
```

## Runtime Dependencies

Nothing here is "found on the machine". Every row is either shipped in the
bundle or supplied by the caller, and the two environment rows below are the
contract every automated launch must honour.

Supplied column vocabulary: **SHIPPED** = inside `Yap.app`; **MANAGED** = downloaded and
sha256-verified by the app itself; **BUILD** = consumed at build time, never at run time;
**CALLER** / **USER** = provided by whoever launches the app / granted by the user.
`tests/docs_match_the_app.rs` fails if a row uses anything else.

| Dependency | Supplied | Lands on disk at | Default when absent | Notes |
|---|---|---|---|---|
| ASR model (GGUF; Parakeet unified EN 0.6B by default) | MANAGED — in-app model manager, resumable, sha256-pinned in `src/catalog.json` | `<state root>/models/` | No transcription until downloaded; onboarding fetches it | NOTHING ships in the DMG (decision D28). Hugging Face, with the catalog's mirror. |
| Polish model (GGUF; Qwen2.5 1.5B Instruct Q4_K_M, 0.5B alternative) | MANAGED — same installer, sha256-pinned | `<state root>/models/` | Polish stage is a no-op; the sidecar is never spawned | ~1.1 GB; optional. |
| Silero VAD model (ONNX v4) | MANAGED — `vad.rs::ensure_model`, sha256-pinned commit URL | `<state root>/models/silero_vad_v4.onnx` | VAD-dependent trimming is skipped | Re-hashed every time a cached copy is picked up (YV45). |
| Diarization models (pyannote segmentation 3.0, WeSpeaker CAM++) | MANAGED — tar.bz2 extracted by the app, both hashes pinned | `<state root>/models/diarize/` | Diarization unavailable (not yet wired to meetings) | |
| `yap-polish` sidecar | SHIPPED — `bundle.externalBin`, `Contents/MacOS/yap-polish` | inside the bundle | none — a cargo build of the app FAILS without the staged binary | Build: `npm run sidecar` (or `cargo build -p yap-polish --release` + copy to `src-tauri/binaries/yap-polish-<triple>`). |
| `yap-diarize` sidecar | SHIPPED — `bundle.externalBin`, `Contents/MacOS/yap-diarize` | inside the bundle | none — same precondition | Statically links sherpa-onnx; see next row. |
| sherpa-onnx prebuilt archive (`sherpa-onnx-v1.13.4-osx-arm64-static-lib.tar.bz2`, 19.5 MB) | BUILD — fetched by `sherpa-onnx-sys`'s build script, linked into `yap-diarize` | `desktop/target/sherpa-onnx-prebuilt/` (build machine only) | The `yap-diarize` build fails offline | See "sherpa fetch at build time" below. Nothing of it is downloaded on a user's Mac. |
| `YAP_DATA_DIR` (env) | CALLER — exported before launch | — | `<Application Support>/WilsonVoice` — the shipped behaviour, unchanged | Relocates the app's ENTIRE state root: SQLite history, `settings.json`, `models/`, `recordings/`, `recovery/`, `meetings/`, `logs/`. Resolved ONCE per process (`src/app_paths.rs`) and then frozen, so it cannot move the history out from under an open SQLite connection. It is an OVERRIDE, never a rename — the default directory name is frozen because renaming it orphans every existing install's history. |
| `--smoke` (argv) | CALLER — passes the flag | — | Normal GUI launch | A launch an automated agent may make. It registers NO global hotkey (no CGEvent PTT tap, no ⌃⌘V / ⌃⌘M / ⌃⌘Z / ⌘⇧V), synthesizes NO ⌘V and no Delete keystroke, and **refuses to start** — exiting non-zero with a named `SMOKE_REFUSED_*` message on stderr — when `YAP_DATA_DIR` is unset or resolves to the default root. It never degrades into using the real data dir. |
| macOS TCC grants (Microphone, Accessibility, Input Monitoring; system audio and Calendar for meetings) | USER — granted in System Settings, keyed to the bundle identifier | TCC database | The app degrades to clipboard-only | The bundle id `com.wilsonguenther.wilson-voice` is irreversible: renaming it resets every grant. |

### sherpa fetch at build time

`yap-diarize` depends on `sherpa-onnx`, whose `-sys` build script downloads
`sherpa-onnx-v1.13.4-osx-arm64-static-lib.tar.bz2` (19.5 MB) from GitHub Releases into
`desktop/target/sherpa-onnx-prebuilt/` whenever `SHERPA_ONNX_LIB_DIR` is unset, choosing the
URL from the sys crate's own version — which is why both halves are pinned exactly
(`yap-diarize/Cargo.toml`, `tests/supply_chain.rs::diarize_sidecar_pins_sherpa_onnx_exactly`).
`.github/workflows/ci.yml` documents this at length; it is the open supply-chain item (YV123).
Two escape hatches remove the network from the build:

- **`SHERPA_ONNX_ARCHIVE_DIR`** — a directory holding the vendored `.tar.bz2`; the build
  script extracts it itself, so a checked-in archive plus one env var is enough.
- **`SHERPA_ONNX_LIB_DIR`** — an already-extracted lib tree (produced and cached elsewhere).

A cold build of the app does not touch this; a cold build of the sidecars does.

**Run contract — no agent launches the app without `YAP_DATA_DIR`.**

The harness isolates working directories, cargo target dirs and preview ports.
It does not isolate the app's state. Two concurrent launches plus the installed
copy would otherwise read and write one SQLite history, one settings store and
one models dir, all register the same global PTT binding, and all synthesize a
paste into whatever window happens to be frontmost. So:

```bash
export YAP_DATA_DIR="$(mktemp -d)/yap-state"
npm run desktop:dev            # or the built binary
# automated / unattended runs additionally pass:
#   wilson-voice --smoke
```

`--smoke` is the enforcement: it is the only launch mode that cannot silently
fall back to the real dictation history, because it exits non-zero instead.

## Platform floor — the weakest Mac this must work on

| | Floor | Source of truth |
|---|---|---|
| Chip | **Floor chip:** `Apple M1 (arm64)` — a fanless 8 GB M1 MacBook Air is the reference machine | `tests/long_take_budget.rs`, `transcription.rs`, `tests/meeting_capture_memory.rs` |
| RAM | **Floor RAM:** `8 GB` | `FLOOR_MACHINE_BYTES` in `tests/long_take_budget.rs`; Y3-G's peak-resident ceiling (2.44 GiB) is asserted against it |
| macOS | **Floor macOS (dictation):** `12.0` | `bundle.macOS.minimumSystemVersion` in `tauri.conf.json` |
| macOS, meeting notes | 14.4 — CoreAudio process taps | `os_version_gate.rs::SYSTEM_AUDIO_MIN` |
| Free disk | ~2.5 GB: app + ASR model (~0.5–0.75 GB) + polish model (~1.1 GB) + diarization/VAD (<0.05 GB) + history and meeting audio headroom | `src/catalog.json` file sizes |

**The config over-promises on one axis, and this table says so rather than hiding it.**
`minimumSystemVersion: "12.0"` lets an Intel Mac on macOS 12 open the DMG, but nothing
in the build targets Intel: `git grep -E 'x86_64-apple|universal-apple' -- .github
desktop/package.json desktop/src-tauri/tauri.conf.json` returns nothing and the only
staged sidecars are `aarch64-apple-darwin`. **The shipped DMG runs on Apple silicon only.**
macOS 12/13 on Apple silicon is the declared floor for dictation but has not been
measured on this loop's machines (all measurements: M4 Pro, macOS 26). Raising
`minimumSystemVersion` to match the arm64 reality, or building universal, is Wilson's call
(`docs/loop/PLAN.md` §4, `docs/loop/DEFERRED.md` #7) — this item documents the floor
and does not change the config. `tests/docs_match_the_app.rs` keeps this table equal to
the config and to the budget constant.

## Explicit non-goals

- Always-on AWS GPU for short dictation
- Fake “three Wilson models”
- GraphQL for local SQLite
- Replacing Kokori (TTS ≠ STT)
- Re-litigating Desktop path / TCC identity rules

## Lifecycle (Y6-D)

What the app does on the events the meeting matrix covers only for meetings.
Each decision is a pure function the running app calls, pinned by
`desktop/src-tauri/tests/lifecycle_e2e.rs`; the physical events themselves are
rows in `docs/RELEASE.md` → LIFECYCLE MANUAL CHECKLIST, because a `cargo test`
process has no window-server session.

| Event | Decision | Where |
|---|---|---|
| Cold launch, no settings file | Pill stays hidden until onboarding is saved (`launch_pill_visible`) | `float_pill.rs`, `lib.rs` setup |
| Sleep mid-take | The dictation consumer of `power::SleepWakeHub` parks the take via `cancel_recording` (clip kept in History) | `power.rs`, `lib.rs` |
| Display unplugged | Pill parks on a monitor from `available_monitors()` *now*, clamped inside it (`pill_origin_on_visible_screen`) | `float_pill.rs` |
| Output device swap mid-take | Restore writes to the device that was MUTED (`OutputAudioState.device`, `restore_target`), never to the new default | `sysaudio.rs` |
| Second launch | Socket handoff, then an `flock` on `<data dir>/yap.instance.lock` taken BEFORE `history.db` opens; held → exit 0 | `lifecycle.rs`, `lib.rs::run` |

**FULLSCREEN — named limitation (Y6-D).** ROADMAP.md says a normal `NSWindow`
cannot float above fullscreen apps and names `tauri-nspanel` as the fix. That
premise is stale for the pill: `float_pill.rs` already converts the float
window with `to_panel::<DictatePill>()` (nonactivating, `PanelLevel::Status`,
`canJoinAllSpaces | stationary | fullScreenAuxiliary | ignoresCycle`). Whether
that panel actually appears over a fullscreen app has NOT been measured — the
Y6-D build ran headless and did not drive a fullscreen Space. Status:
**unmeasured**. The answer is the manual row "Pill floats over a FULLSCREEN
app" in `docs/RELEASE.md`; until that row carries a date, a machine and a
result, nothing may claim fullscreen works.

