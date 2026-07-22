# Wilson Voice — End-to-End Audit & Rebuild Plan

_Senior QA + systems + ML audit of the whole app, plus the sourced architecture
decision and the ordered fix roadmap the dev loop executes against._

## TL;DR

- **The app is ~80% right. It was never two competing apps — it is one Tauri app
  (`desktop/`) plus dead legacy Python code (`wilson_voice/`, now removed).**
- **Keep the stack.** Tauri 2 + Rust shell + on-device ASR is correct and proven
  by Handy (the closest open-source twin). A native-Swift rewrite is ~6–10 weeks
  to reach current parity for zero user-visible gain. Do **not** rewrite.
- Every remaining failure is a **specific, named bug**, not an architecture flaw.

## Architecture verdict (researched, sourced)

| Question | Verdict |
|---|---|
| Keep Tauri, or rewrite native Swift / Electron? | **Keep Tauri, consolidate to one app.** Handy ships this exact trifecta (overlay + global key + paste, offline) on Tauri 2. Electron ruled out (webview + ~800 MB RAM class). Native Swift only justified as a *scoped* helper for the pill if it can't float over fullscreen in release. |
| Rust or Python for the hot path? | **Rust shell; Python only for MLX ASR today.** The Python sidecar is the source of the ffmpeg/PATH/IPC fragility; migrate ASR into Rust (`transcribe-rs`) later to ship one binary. |
| Is the ASR model fine-tuned? Better model? | Not fine-tuned (correct — on-device LoRA overfits on scarce data). Better engine = **Parakeet-TDT v3** (2–5× faster than MLX-Whisper-turbo, equal/better EN accuracy, runs in Rust). The cheap high-ROI "learning" is **dictionary → Whisper `initial_prompt` biasing** (+45–60% rare-word recognition) + a post-ASR correction pass. |
| Streaming like Wispr? | Wispr feels instant because it is **cloud-streaming (no offline mode)**. superwhisper (local) transcribes on release — same as us. Honest local UX = warm + small model + paste-on-release. Real partials only via Parakeet RNN-T or Apple SpeechAnalyzer (later). |

## Audit findings by subsystem (severity)

### Rust core — crashes, PTT, pill (`desktop/src-tauri/src/`)
- **CRITICAL** PTT `Start` (hold-arm timer thread) and `Stop` (CGEvent tap thread)
  race with no ordering guard → stuck-recording latch, mic stays hot. *(ptt_macos.rs / lib.rs)*
- **HIGH** `paste_entry` is a **sync** Tauri command → runs on main thread, then
  blocks on a task queued *to* main → 3s deadlock, History paste always fails. *(paste.rs / lib.rs)*
- **HIGH** Float pill recreated at cold start → re-triggers the launch hang a prior
  commit fixed. *(lib.rs setup)*
- **HIGH** Pill mis-positioned on Retina (points treated as pixels). *(float_pill.rs)*
- **HIGH** 900 ms "space keeper" re-derives pill position from the **cursor** and
  re-applies the full style mask every tick → flicker, focus-steal, monitor teleport. *(float_pill.rs)*
- **HIGH** ~280 ms of leading speech clipped every hold (Start delayed before mic opens). *(ptt_macos.rs)*
- **MEDIUM** Clipboard clobbered, never restored. Mic reported "ready" even when
  TCC-denied. Auto-paste declines on a fragile AX role whitelist. Startup `expect()` panics.

### Frontend / IPC (`desktop/src/`)
- **CRITICAL** On-screen Stop dead in hands-free: `manual_toggle` / tray toggle don't
  clear the `hands_free` latch, so `stop_and_transcribe` early-returns — pill/button/tray
  can't stop a hands-free recording. **This is the core "pill isn't working".**
- **HIGH** `refreshAll` depends on `query` and owns listener registration → every
  keystroke tears down + re-adds all listeners and refetches; events dropped in the gap.
- **HIGH** No error handling on mutating `invoke`s (paste/save/delete) → silent failures.
- **MEDIUM** Pill conflates drag-region + click; waveform freezes on steady input.
- **GOOD** IPC contract is 100% consistent (zero command/arg/event mismatches).

### ASR / ML (`python/asr_worker.py`, `asr.rs`)
- **CRITICAL** `mlx_whisper` decodes via the **ffmpeg binary**; sidecar spawned without
  PATH → breaks in a Finder-launched `.app`. **FIXED** (ndarray decode path).
- **CRITICAL (goal)** "Learning" is a no-op: harvest stores `preferred == term`, seeds
  store `preferred = NULL`, dictionary never fed to Whisper. **Partially fixed** (worker
  now accepts `prompt`/`vocab`); Rust wiring + harvest redesign pending.
- **HIGH** Daemon read has no timeout + holds a global mutex across the blocking read →
  one hung worker (first-run model download, stall) hangs all dictation forever.
- **MEDIUM** Naive un-filtered resampler; stdout protocol has no framing; default
  profile Balanced (~1–3 s) misses the <800 ms Fast target.

### Data / SQL (`db.rs`) — the "rescue SQL"
- **Data layer is sound**: FTS5 synced, migrations idempotent+safe, bound params, correct
  insights math. **The rescue/repair path is absent.**
- **HIGH** No `wal_checkpoint` anywhere + quit via `app.exit(0)` skips the connection
  destructor → 3.6 MB WAL never checkpoints (4 KB `.db` looks empty → partial-backup data loss).
- **HIGH** DB open is a hard `.expect()` panic — no `integrity_check`, `busy_timeout`, or repair.

### Build / TCC / drift
- **CRITICAL (latent)** `scripts/install_launch_agent.sh` auto-launched the **legacy**
  Python app at login under the Tauri bundle id (TCC = "Python", mic/hotkey conflict). **REMOVED.**
- **HIGH** Ad-hoc signing fallback silently breaks TCC persistence. **HARDENED** (fails unless forced).
- Current app reads `settings.json` (`fn_control`); `config.yaml` (`right_option`) was inert
  legacy residue. Bundle id consistent, `custom-protocol` on, `NSMicrophoneUsageDescription` present.

## The Wispr interaction spec to match (build target)

- PTT default **`Fn`** (fallback `Ctrl+Opt`); hold = talk; **double-tap / `Fn+Space` = hands-free lock**; `Esc` cancels.
- One `CFRunLoop` owns tap + all timing (no cross-thread Start/Stop); **record-then-discard** (no clipped leading word); ~40 ms Fn debounce; re-enable on `tapDisabledBy*`.
- Pill = native `NSPanel`: `.nonactivatingPanel | .borderless` set **once**, level `.statusBar`, collectionBehavior `.canJoinAllSpaces | .fullScreenAuxiliary | .stationary`, `canBecomeKey=false`. Bottom-center via `visibleFrame`×`backingScaleFactor`; reposition on space/screen **notifications**, never a timer. States: idle → listening (bars + ping) → transcribing → done (✓).
- Paste: gate on **Accessibility trust only** (drop role whitelist); save pasteboard → write → ⌘V → **restore**; optional AX `kAXSelectedText` fast path. Ship Developer-ID.
- Shell: `NSStatusItem` + main window + pill in one process; `.accessory` default, toggle to `.regular` when Settings opens.
- Learning: auto-add to dictionary by watching post-paste edits; feed dictionary into `initial_prompt`; filler removal that doesn't eat real words.

## Roadmap (the dev loop: build → QA → push → review → repeat)

1. **Consolidate + ffmpeg + CI** — remove legacy app, ndarray decode, dictionary
   `initial_prompt` plumbing, GitHub Actions CI, hardened signing. _(this iteration)_
2. **Rust crash/UX fixes** — async `paste_entry`; clear `hands_free` on button/tray stop;
   clipboard save/restore; frontend error toasts + listener lifecycle.
3. **PTT rewrite** — single-CFRunLoop state machine, record-then-discard, tap re-enable, double-tap lock.
4. **DB durability** — `wal_checkpoint(TRUNCATE)` on exit, `busy_timeout`, `integrity_check`/repair path.
5. **Pill** — native `NSPanel` per spec; kill the cursor-keeper; Retina-correct; state machine.
6. **Learning** — harvest redesign (`preferred = NULL` candidates, drop stopwords, real
   `heard→meant` corrections from post-paste edits), Rust wires dictionary → worker `prompt`.
7. **Engine** — evaluate `transcribe-rs` + Parakeet-TDT; default profile / latency to target.
8. **Release** — Developer-ID + notarization so TCC survives updates.

## CI/CD

- `.github/workflows/ci.yml`: Python compile + fast tests (`pytest -m "not heavy"`);
  frontend `npm ci && npm run build`; `cargo build --release` + `cargo test` (gate),
  clippy/rustfmt informational until the tree is clean.
- Branch-per-slice + PR as the review surface; conventional commits.
