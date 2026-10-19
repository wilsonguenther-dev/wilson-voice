# Yap test channel — "Yap Test.app"

**What it is.** A way to run any head (main, a branch, a PR, a sha) *before* it
clears the two-seat review, without touching the real, signed
`/Applications/Yap.app`. The test build is a different app as far as macOS is
concerned: bundle id `com.wilsonguenther.wilson-voice.test`, name "Yap Test",
installed at `/Applications/Yap Test.app` (each test build replaces the last
one; the real Yap is never written to).

```bash
cd desktop
npm run test-build                 # origin/main
npm run test-build -- --pr 240     # a PR head
npm run test-build -- --branch feat/x
npm run test-build -- --sha 1a2b3c4
npm run test-build -- --dry-run --pr 240   # resolve + print, build nothing
```

## How Wilson uses it

Quit the real Yap first (menubar → Quit) — both apps register the same global
hotkeys, so only one can own them. Run `npm run test-build -- --pr N`, open
**Yap Test** from /Applications, and try the change. The yellow chip
`TEST BUILD <sha> · <branch>` in the window's top-right corner and on the float
pill says exactly which head you are holding. When done, quit Yap Test and
`open -a Yap` to go back to the real one.

**First launch only — TCC.** macOS ties Microphone, Accessibility and Input
Monitoring grants to the bundle id + signature, so Yap Test asks for its own
grants ONCE (System Settings → Privacy & Security; it shows up as "Yap Test").
Because every test build is signed with the same Developer ID identity as
production, those grants persist across later test builds. If a grant ever
looks stuck, remove "Yap Test" from the list and re-add it.

**Gatekeeper.** Test builds are signed but NOT notarized (local only). The
script strips the quarantine flag, so they normally just open; if macOS ever
says it cannot verify the developer, right-click → Open once.

## What the switch changes

One build-time variable, `YAP_CHANNEL=test`, set by `scripts/test-build.sh`:

| | Yap (production) | Yap Test |
|---|---|---|
| bundle id | `com.wilsonguenther.wilson-voice` | `com.wilsonguenther.wilson-voice.test` |
| app name | Yap | Yap Test |
| state root (history, settings, license, models) | `~/Library/Application Support/WilsonVoice` | `~/Library/Application Support/WilsonVoice-Test` |
| log | `…/WilsonVoice/logs/yap.log` | `…/WilsonVoice-Test/logs/yap.log` |
| updater | on | **off** — no endpoint contacted, no install, no updater artifacts |
| chip | none | `TEST BUILD <sha> · <branch>` |
| hotkeys | defaults | same defaults (quit the real Yap while testing) |

- Rust: `build.rs` turns `YAP_CHANNEL=test` into the `yap_channel_test` cfg;
  `src-tauri/src/channel.rs` derives identifier, product name, data dir, log
  path and updater-enabled from it together (`app_paths::default_root`,
  `perm_truth::BUNDLE_ID`, `check_for_update`/`install_update` read it). It is
  compile-time, so no environment variable can flip an installed production app.
- Bundle: `tauri build --config <overlay>` (merged over `tauri.conf.json`) sets
  `identifier`, `productName`, `bundle.createUpdaterArtifacts: false`,
  `plugins.updater.endpoints: []` and an Info.plist copy whose
  `CFBundleName`/`CFBundleDisplayName` say "Yap Test".
- Frontend: `vite.config.ts` folds the same variables into `__YAP_BUILD__`;
  `src/buildChannel.ts` renders the chip (null on production).
- Sidecars (`yap-polish`, `yap-diarize`) build through `beforeBuildCommand`
  exactly as a release does.
- Models: on the first test install the real Yap's `models/` dir is APFS-cloned
  (`cp -c`, no extra disk) into the test root so nothing re-downloads. History,
  settings and the license/trial state are NOT copied — Yap Test starts fresh.

**Heads that predate this switch** (anything branched before it landed) get the
test-channel commit merged in, inside a throwaway build worktree that is never
pushed. If that merge conflicts the script refuses — rebase the head on main.

Before installing, the script refuses any bundle whose id is not the test id,
whose name is not "Yap Test", or whose binary was not compiled with the test
data dir. Every install appends a line to
`~/Obsidian/Wilson-Brain/Projects/Loop-Logs/yap-test-builds.md`.

Environment overrides: `YAP_TEST_TARGET_DIR` (else `CARGO_TARGET_DIR`, else
`~/.cache/yap-test-channel/target`), `YAP_TEST_WORKTREE`, `YAP_TEST_LOG_MD`.
`--keep` keeps the build worktree (faster rebuilds: node_modules survive).

Tests: `cargo test -p wilson-voice channel::` (the switch moves identifier,
data dir, log path and updater together) and `bash scripts/test-build.test.sh`
(dry-run resolves the head and prints the overlay).
