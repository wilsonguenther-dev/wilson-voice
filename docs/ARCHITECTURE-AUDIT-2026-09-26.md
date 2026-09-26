# Yap — architecture, design and UX audit (2026-09-26)

Planning-and-architecture phase for the next Yap ("Yappy") CI/CD loop, the same shape as the
Drivia Learn / Drivia Consulting planning passes. Written against `main` at `55fd5bb`
(`c79acd6` + the 2026-09-15 stop note). Read-only with respect to Wilson's install: the SQLite
history was inspected through a **copy** in a scratch directory, logs were read, nothing on his
machine was written, no app window was opened.

**Wilson's directive (dictated 2026-09-26, ground truth):** start a CI/CD loop on Yappy; go over
every architectural decision, design decision, item and UI/UX flow; "the pill does not indicate
when [state] … 14 days"; "the notetaker is not even working and that's one of the most important
things — every component of that"; better UI, clearer pixels, smoother; better with the operating
system, the kernel, the permissioning system; faster, smoother hotkeys; way better than Wispr Flow.

## Evidence conventions

- `path:line` is relative to `desktop/src-tauri/src/` for `.rs` files and to `desktop/src/` for
  `.ts`/`.tsx` files unless a fuller path is given. Line numbers are at `55fd5bb`.
- **LIVE** marks a fact read from Wilson's own install on 2026-09-26 (read-only): the log files in
  `~/Library/Application Support/WilsonVoice/logs/`, and a copy of `wilson_voice.db` (+WAL).
  Transcript text was never read or printed — only row metadata.
- **RAN** marks a command executed headlessly for this audit in a fresh clone
  (`~/.cache/yap-review-2026-09-26`, deleted afterwards) with its exit code.
- External facts carry a URL. Anything without one of the three markers or a URL is an inference
  and is labelled **UNVERIFIED**.

## TL;DR

1. **The notetaker records but never transcribes.** Stopping a meeting sets the row to
   `transcribing` (`meeting_control.rs:652-656`) and the `stop_meeting` command returns
   (`lib.rs:4410-4423`). The meeting-ASR job that should take it from there — `MeetingAsr`
   (`meeting_asr.rs:1718`), `run()` (`:1767`), `WarmEngineChunkAsr` (`:614`) — is constructed only
   in two integration tests; the module is `#![allow(dead_code)]` (`meeting_asr.rs:52`), and
   `Database::append_meeting_segments` (`db.rs:1652`) has no caller in `src/`. Summaries then refuse
   ("no transcript", `lib.rs:4216-4218`), diarization is never invoked (`meeting_matrix.rs:141-145`),
   and the 7-day retention sweep deletes the audio regardless of state (`db.rs:1936-1972`).
   **LIVE:** all 4 of Wilson's surviving meetings are stuck in `transcribing` with
   `processed_through_seconds = 0`, both WAV paths `NULL`, `audio_kept = 0`, and an empty
   `meetings/` directory. **RAN:** 105 lib meeting tests + 30 integration tests, all green.
2. **The pill fails to indicate** (a) the trial for its first week — the policy is silent above 7
   days (`pill/license.ts:136-165`) and the numeral is not even on `main` (Y2-B, PR #186 open);
   (b) polishing and pasting — declared phases no event ever produces (`pill/live.ts:411,413`,
   reducer `:1152-1196`); (c) a blind hotkey — Secure Input / disabled tap / no Accessibility are
   on the status payload (`lib.rs:893-922`) but the float window reads four fields
   (`float-main.tsx:60-65`); (d) hands-free vs hold; (e) everything a meeting does after stop.
3. **Fresh-Mac runtime dependencies that fail today:** new purchases (the Forge issuer is off and
   the revocation host `license.rs:117` does not answer — **RAN** curl `000`); first launch with no
   network (every model is a first-run download, `models.rs:134`); any Intel Mac (arm64-only);
   system audio below macOS 14.4 (by design, gated). Auto-update works only while the repo is
   public (the feed is a GitHub release asset, `tauri.conf.json:67`).
4. **Hotkey latency is structural, not tuning:** capture starts 280 ms after key-down
   (`ptt_macos.rs:32,488-509`) and there is no pre-roll, so the first syllable of a fast talker is
   not in the take. Arm on key-down, discard on tap (yap24-OS1).
5. **Plan:** the 88-item Y-plan is kept (30 merged, 2 already-done, 11 open PRs, 43 not started, 2 owner-gated —
   every item now carries a dated `notes:` status line) and 26 new `yap24-*` items are added,
   notetaker chain first. The harness gained `args.only` / `args.now` (ported from the Drivia
   harness). All yap24 items are `gated: 'panel'` until a Senior Panel on this document rules.

---

## 1. What exists today (verified from code)

### 1.1 Process and repo map

| Piece | What it is | Evidence |
|---|---|---|
| App | Tauri 2 + React 19 + Rust, one binary `wilson-voice`, bundle id `com.wilsonguenther.wilson-voice` (frozen: renaming resets TCC) | `tauri.conf.json` identifier; `desktop/src-tauri/Cargo.toml:18` |
| Windows | `main` (the app) and `float` (the pill, converted to an NSPanel via `tauri-nspanel`, status level, non-activating, all Spaces) | `float_pill.rs:1-5,293-315,343-369` |
| Sidecars | `yap-polish` (llama.cpp, Qwen2.5 GGUF; formatting + summaries) and `yap-diarize` (sherpa-onnx; pyannote-seg-3 + CAM++). Out of process because ggml/onnxruntime cannot co-link with transcribe-cpp | `bundle.externalBin`; `polish.rs:1-30`; `diarize.rs:16` |
| ASR | `transcribe-cpp` 0.1.3 in-process, Metal, one warm engine held by `TranscriptionManager` | `Cargo.toml:70,108`; `transcription.rs:846,885`; `asr_engine.rs:189,340` |
| Data | `~/Library/Application Support/WilsonVoice/` (frozen name), SQLite WAL + FTS5; overridable with `YAP_DATA_DIR` | `ARCHITECTURE.md` Runtime Dependencies; `app_paths.rs` |
| Size | 50 Rust modules (~48k lines in the 22 largest), 565 `#[test]` in `src/`, 131 integration test files; 17 vitest files / 274 tests | **RAN** `grep -c`, `ls tests`, vitest |

### 1.2 Dictation capture pipeline

1. **Hotkey** — a listen-only `CGEventTap` at the HID location on its own CFRunLoop thread
   (`ptt_macos.rs:309-343`), fn / fn⌃ hold = push-to-talk, double-tap (450 ms, `:34`) = hands-free,
   `+⌘` = command mode. Other shortcuts via `tauri-plugin-global-shortcut` (`shortcuts.rs:32`):
   ⌃⌘V paste-last, ⌃⌘M meeting toggle, ⌘⇧V, Escape while a take is live.
2. **Arm** — after `HOLD_ARM_MS = 280` (`ptt_macos.rs:32`) a thread fires `Start`
   (`:494-509`) → `start_recording` (`lib.rs:1454`) arms the **persistent cpal stream**
   (`record.rs:390`, YV35). The stream closes after 60 s idle (`record.rs:1316`), so the first take
   after a minute pays a cold open (bounded by `ARM_TIMEOUT = 3 s`, `record.rs:1312`).
3. **Capture** — lock-free ring (`rtring.rs`), spill-to-disk for long takes (Y3-A, #170),
   crash-safe journal (YV63), resampler (`resample.rs`), optional `nnnoiseless` denoise, signal
   hygiene, input-format watch for device swaps (**LIVE:** AirPods Max 24 kHz ↔ MacBook mic 48 kHz
   swaps are frequent in the log).
4. **Gate** — Silero VAD v4 via `vad-rs`, sha256-pinned (`vad.rs:491-508`); **LIVE:** loads warm at
   every startup in the current log; the older logs show 178 "silero unavailable" fallbacks to energy
   VAD, which stopped after the yap12 fix. Hallucination gate `is_hallucinated_repetition`
   (`dictation.rs:621`, YV66 windowed/non-destructive).
5. **Decode** — `stop_and_transcribe` (`lib.rs:2071`) → `transcribe_native` (`lib.rs:1964`) →
   `TranscriptionManager::transcribe` (`transcription.rs:846`); long takes decode in windows with
   seam dedupe and real progress events (`transcribe_progress.rs:40`, Y3-B/Y3-C).
6. **Format / polish** — deterministic formatting (`dictation.rs:123`, Y4-A on by default), then the
   optional LLM polish in the `yap-polish` sidecar with a deadline (`lib.rs:441`), chunked for long
   form (Y4-E); the sidecar's output is treated as untrusted (`polish.rs:4`).
7. **Paste** — clipboard write + synthesized ⌘V through `CGEventPost` at the HID tap
   (`paste.rs:404-442`), with a read receipt (YV74); **LIVE:** 4 × "paste not confirmed: no app read
   the clipboard within 1500ms" — a state the pill never shows (§3).

**Measured latency** (memory `project_yap_build_state`, yap12): 77-423 ms hold→clip on real rows,
down from 2,565 ms. That number starts at `Start`, i.e. 280 ms after the key went down.

### 1.3 STT engine, models, and the Runtime Dependencies table

Model catalog `src/catalog.json`: ASR `parakeet-unified-en-0.6b` (default), `nemotron-3.5-asr-
streaming-0.6b`, `whisper-tiny` — each pinned to a Hugging Face revision with per-file sha256,
mirror `blob.handy.computer`; polish `Qwen2.5-1.5B/0.5B-Instruct` Q4_K_M pinned revisions; diarize
models from `wilsonguenther/yap-diarize-models@c0f5026b` (archives, sha-pinned). Every download is
resumable and sha256-verified before use (`models.rs:7-8,189,207-220`). **Nothing ships in the DMG**
(`models.rs:134`). **RAN** (HEAD, 2026-09-26): parakeet, Qwen 1.5B and the diarize archive all
return 200 at their pinned revisions.

Runtime Dependencies, per `03_Intelligence/AI-Context/project-discovery-protocol.md` (nothing may
be "found on the machine"):

| Dependency | How it is supplied | Fresh Mac verdict | Evidence |
|---|---|---|---|
| ASR model (~0.6B GGUF) | First-run download from huggingface.co (+ handy mirror), sha256-verified | **Fails offline** until one download succeeds | `models.rs:134,202`; catalog |
| Silero VAD ONNX | First-run download from a pinned github.com raw URL, sha256 | Falls back to energy VAD offline (works, less accurate) | `vad.rs:491-549` |
| Polish/summary model (Qwen GGUF) | Optional download (SEC-C install path, #184) | Formatting works without it; summaries need it | `models.rs:207-220` |
| Diarization models | Download from Wilson's HF repo | Only needed by the (unwired) diarization step | catalog `diarize_models` |
| `yap-polish`, `yap-diarize` | Bundled `externalBin` | OK | `tauri.conf.json` |
| onnxruntime for `yap-diarize` | Prebuilt tarball fetched by `sherpa-onnx-sys` **at build time** | Build-machine dependency, not runtime | HARNESS.md "The gate" |
| License issuer (purchase → key) | Stripe Payment Link → Forge Fastify → Ed25519 signer → Resend | **Fails: Forge box decommissioned** — no issuer exists | memory `project_yap_loop_state_20260912`; LIC-A |
| Revocation list | GET `https://forge.87-99-149-214.sslip.io/v1/yap/revoked.json` | **Fails** (RAN: HTTP 000); designed as a no-op on failure, so licensed users are unaffected | `license.rs:117`; LIVE log "revocation refresh skipped" |
| Stripe Payment Link | `buy.stripe.com/…` | Link answers 200; whether it is `active` is UNVERIFIED (memory says `active:false`) | `license.rs:137` |
| Updater feed | GitHub `releases/latest/download/latest.json` | Works **only while the repo is public** (RAN: 200, 0.8.0) | `tauri.conf.json:67`; UPD-A |
| macOS ≥ 12.0 | `minimumSystemVersion` | OK; system audio needs 14.4 (gated) | `tauri.conf.json`; `os_version_gate.rs` |
| Apple silicon | arm64-only DMG | **Fails on Intel Macs** — open decision (DEFERRED #7) | DEFERRED.md §7 |
| TCC: Microphone, Accessibility, Input Monitoring, System Audio Recording | User grants, keyed to the bundle id | Required; recovery via deep links (§4) | `permissions.rs:804-820` |

### 1.4 Hotkey system — see §4.

### 1.5 macOS permissions

| Grant | How it is read | How it is requested | Recovery when denied | Evidence |
|---|---|---|---|---|
| Microphone | `AVCaptureDevice authorizationStatusForMediaType` | `requestAccessForMediaType:` (guarded by the usage string) | Denied screen + `Privacy_Microphone` deep link | `mic_auth.rs:10,127,170-192`; PERM-A/B/C/D merged |
| Accessibility (paste) | `AXIsProcessTrustedWithOptions` | the same call with the prompt option | `Privacy_Accessibility` deep link | `permissions.rs:118-133,808` |
| Input Monitoring (the fn tap) | `IOHIDCheckAccess(ListenEvent)` read-only | **never requested** — the tap failure says "enable Accessibility" | `Privacy_ListenEvent` link exists but the copy points elsewhere | `permissions.rs:92,377-392,811`; `ptt_macos.rs:323` |
| System Audio Recording | **no public read API** — inferred from tap delivery | Settings "Set up meeting recording" pre-warm | `Privacy_AudioCapture` link; LooksDenied sentence | `syscapture.rs:2243,2324-2341,3535`; `permissions.rs:781,817` |

Revocation is watched by a 45 s poll with a 30 s floor (`permissions.rs:536-542`). A grant made in
System Settings is not noticed until the next tick; there is no re-check on app activation.

### 1.6 The pill — see §3 for what it fails to show.

States (`LivePhase`, `pill/live.ts:387-428`): idle, listening, thinking, done, sleepy, blocked,
waiting, gated, transcribing, polishing, pasting, error, model-loading, empty, cancelled. One shell
(`float-main.tsx:76-320`) folds two reducers — the gate (`reduceGatePhase`: mic permission, license,
cancel) and the take (`reduceTakePhase`, `live.ts:1152-1196`) — into one phase with a precedence
rule, draws a phase strip once for both characters (ClassicPill, YappyPill), and carries license and
dock on `<html data-*>`. Meetings add a recording badge (`pill/MeetingBadge.tsx`).

### 1.7 Updater, DMG, notarization

- `tauri-plugin-updater` with a minisign public key and a GitHub latest.json endpoint
  (`tauri.conf.json:67`). Tauri's updater requires a signature and it "cannot be disabled"; the
  feed must carry `version`, `platforms.<target>.url` and `.signature`
  (https://v2.tauri.app/plugin/updater/). The v2 keypair was regenerated in YV82 and the password is
  stored retrievably (memory).
- **RAN:** the feed answers 200 and serves 0.8.0 (darwin-aarch64, 2026-08-11). **LIVE:** the log
  shows "update check failed: error sending request" on offline launches and 22 older
  "did not respond with a successful status code" lines from the private-repo window.
- Releases are hand-built: `tauri build --bundles app` from a non-iCloud clone, Developer ID sign
  with hardened runtime, `hdiutil` DMG (the bundled `bundle_dmg.sh` fails headless), `notarytool`
  with the `yap-notary` keychain profile, staple, `spctl` (memory `reference_yap_dmg_notarization`,
  `docs/RELEASE.md`). `.github/workflows/release.yml` is disabled; Actions is off account-wide.

### 1.8 Licensing and the trial

Offline Ed25519 verification against a pinned public key; a 14-day full trial; the license gates
**only new dictations** — history, export and settings work forever (`ARCHITECTURE.md` attack
surface; `license.rs`). Trial state is corroborated by `license_state` (two integers, rollback
floor). **LIVE:** "license: trial, 5d left" on 2026-08-20; "licensed (lifetime)" since 2026-09-25.
The purchase leg is dead with Forge (see 1.3) — LIC-A (#183) moves it to a Supabase Edge Function
on a dedicated Yap project, blocked only on Wilson provisioning that project.

### 1.9 Telemetry, crash reporting, QueryGuard

No analytics SDK, no Sentry, no PostHog (`lib.rs:3784`; PLAN.md §0). Local crash capture: panic
hook + rotating log + `.ips` ingestion into `crash_events`, a Stability UI and a "email crash report
to Wilson" button (YV64, memory). **LIVE:** `crash-summary.txt` lists 5 crashes, the newest two
`EXC_BAD_ACCESS (SIGSEGV)` in 0.8.0 on threads 39/40 (2026-08-18, 2026-08-29); the `.ips` files have
since rotated out of DiagnosticReports, so the faulting frames are **UNVERIFIED**. QueryGuard is a
Drivia web surface and does not apply to a local desktop app; the local equivalent is the
memory/disk telemetry line every hygiene interval (`lib.rs:5257-5284`).

### 1.10 Settings and data

Settings are a serde struct persisted atomically to `settings.json` (salvage on parse failure,
yap13), eight tabs (`views/settings/`: Advanced, Audio, Companion, Dictation, License, Privacy,
Shortcut, Snippets). **LIVE** DB tables: transcripts(+FTS5), dictionary, dict_candidates,
snippets, scratchpad, daily_stats, failed_dictations, crash_events, license_state, settings_kv,
meetings, meeting_segments(+FTS5). **Not present:** `meeting_actions`, `speaker_profiles`
(planned in the notetaker epic). 1,777 transcripts, 0 failed dictations, 0 meeting segments.

---

## 2. The notetaker — why it does not work

### 2.1 The path, stage by stage

| Stage | What should happen | What the code does | Verdict |
|---|---|---|---|
| Start | tray / ⌃⌘M / pill / Meetings button → `MeetingController::start_with_kind` | Works: creates the row, starts mic capture (+ system tap when the setup ack says so), power assertion, ticker (`meeting_control.rs:486-560`). **LIVE:** 6 starts logged 2026-08-14..08-20 | OK |
| Capture | two tracks, never pre-mixed, spilled to disk, journaled | Works (yap22-A/B, 30 green integration tests) | OK |
| Stop | close capture, write WAV paths, state → `transcribing`, **hand to the transcription job** | Writes WAVs and `transcribing` (`meeting_control.rs:627-722`); `stop_meeting` returns (`lib.rs:4410-4423`). **No hand-off.** | **BROKEN** |
| Transcribe | `MeetingAsr` chunker over the warm engine, preemptible, resumable, writes segments | Exists, tested, **never constructed in `src/`** (`meeting_asr.rs:52,614,1718,1767`; constructors only at `tests/meeting_dictation_preempts_transcription.rs:145`, `tests/matrix_new_asr_chunk_timeout.rs:108`); `append_meeting_segments` (`db.rs:1652`) has no caller | **DEAD CODE** |
| Diarize | `yap-diarize` sidecar clusters the room track | `diarize::pool()` has no caller (`meeting_matrix.rs:141-145`); six parked defects (#142-#146, #149 / issues #150-#155) | **UNWIRED** |
| Notes | map-reduce summary + action items | `summarize_meeting` is manual and refuses with no segments (`lib.rs:4201-4218`); no `meeting_actions` table | **UNREACHABLE** |
| UI | progress, notes, failure, retry | "Yap is still working through the audio." forever (`views/Meetings.tsx:339-347`) | **MISLEADING** |
| Retention | delete audio after the transcript is safe | purges any meeting older than 7 days, any state (`db.rs:1936-1972`, `lib.rs:2966-3001`, `meetings.rs:73`) | **DATA LOSS** |

### 2.2 Root causes (with file:line)

- **RC-1 — no production caller for the meeting ASR job.** `meeting_control.rs:652-656` sets
  `transcribing` with the comment "YV93's transcription pipeline moves it on to `complete`". There
  is no such pipeline: `MeetingAsr` (`meeting_asr.rs:1718`) and its `run` (`:1767`) are built only by
  tests, and `#![allow(dead_code)]` at `meeting_asr.rs:52` hides it from the compiler. **Fix:
  yap24-NT2.**
- **RC-2 — the tests prove the parts, never the wire.** `tests/meeting_manual_start_stop.rs:158-162`
  asserts `transcribing` "(YV93 finishes it)" and ends; `meeting_matrix.rs` honestly records rows as
  `PolicyOnly` because their call sites do not exist. **RAN:** `cargo test --lib meeting` 105 passed;
  `meeting_manual_start_stop` 13, `meeting_dictation_preempts_transcription` 4,
  `meeting_track_b_wiring` 7, `meeting_audio_retention` 2, `meeting_event_contract` 4 — all green,
  exit 0. This is the "verification that verifies nothing" class (memory
  `reference_drivia_verification_that_verifies_nothing`). **Fix: yap24-NT2 tests + yap24-NT9.**
- **RC-3 — retention ignores state.** `purge_meeting_audio` selects on `started_at < cutoff` only
  (`db.rs:1945-1969`); its own caller's comment says it is time-based because "there is no summarize
  stage yet (YV97)" (`lib.rs:2960-2965`). **LIVE:** every meeting lost its audio before it was ever
  transcribed. **Fix: yap24-NT1.**
- **RC-4 — silence is scored as a denied permission.** `permission_verdict` returns `LooksDenied`
  whenever the tap delivered nothing non-zero for `DENIAL_GRACE = 3 s` (`syscapture.rs:2243,
  2334-2341`), which is also what "nothing was playing" looks like. **LIVE:** 3 of 4 meetings carry
  "macOS has not granted System Audio Recording to Yap, and it will not ask again", one of them an
  `in_person` meeting that never needed system audio. **Fix: yap24-NT3.** Apple documents process
  taps from macOS 14.2 (https://developer.apple.com/documentation/coreaudio/audiohardwarecreateprocesstap(_:_:))
  and the `NSAudioCaptureUsageDescription` purpose string from 14.2
  (https://developer.apple.com/documentation/bundleresources/information-property-list/nsaudiocaptureusagedescription);
  Yap gates at 14.4 (`os_version_gate.rs`) — keep the stricter gate, the reason for 14.4 over 14.2
  is recorded only in the plan note (UNVERIFIED here).
- **RC-5 — the plan never listed it.** The 2026-09-12 88-item plan was built from Wilson's six
  dictation complaints (PLAN.md §1 A-F). The notetaker appears only as the diarization carry-forward
  (Y11). A broken feature nobody itemized is a feature no loop will fix — which is why the yap24
  chain runs first.

### 2.3 LIVE record of Wilson's meetings (metadata only)

| id (prefix) | started (UTC) | length | kind | state | processed | audio | error column |
|---|---|---|---|---|---|---|---|
| 9ba4f7e4 | 2026-08-14 18:01 | 10.3 s | unknown | transcribing | 0.0 | purged | — |
| 56964c52 | 2026-08-15 12:15 | 4.1 s | unknown | transcribing | 0.0 | purged | LooksDenied sentence |
| cc9f6153 | 2026-08-16 23:18 | 7.6 s | **in_person** | transcribing | 0.0 | purged | LooksDenied sentence |
| 0a052e58 | 2026-08-20 22:01 | 1.3 s | unknown | transcribing | 0.0 | purged | LooksDenied sentence |

Two further meetings in the log (f4a5810f, dae30e59, 2026-08-15) are no longer in the DB (deleted).
`settings_kv.meeting_system_audio_setup_ack_v1 = "ran 2026-08-20T22:02:28Z"`. No
`meeting_asr`/segment/summary line appears anywhere in `yap.log*` after any stop.

### 2.4 The chain that fixes it (items in `scripts/loop/items/01-yap24-notetaker.mjs`)

`yap24-NT1` stop the data loss + reconcile stranded rows → `NT2` wire the transcript (remove
`allow(dead_code)`, one worker, preemptible, resumable, both tracks, CLI `--transcribe-meeting`) →
`NT3` honest system-audio verdict, in-person never taps → `NT4` automatic notes + `meeting_actions`
+ export → `NT5` diarization joins (after Y11-A..E) → `NT6` Meetings view shows every state with
Retry → `NT7` pill + tray follow the meeting after stop → `NT8` calendar-aware "Record?" (opt-in,
EventKit, macOS 14+: https://developer.apple.com/documentation/eventkit/ekeventstore/requestfullaccesstoevents(completion:))
→ `NT9` headless end-to-end proof script. One lane, sequential.

---

## 3. The pill — what it fails to indicate, and why

Wilson: "the pill does not indicate when [state] … 14 days". Five gaps, each verified:

| # | Missing indication | Why (code) | Item |
|---|---|---|---|
| P1 | **Trial days left for the first week (days 14-8)** | `pillLicense` policy: "trial, more than 7 days → nothing (ambient silence)" (`pill/license.ts:136-165`). And on `main` neither capsule draws the numeral at all — Y2-B's PR #186 is open, unmerged, never visually QA'd | yap24-PILL1 (+ Y2-B) |
| P2 | **Polishing / pasting** | Declared (`pill/live.ts:411,413`), never produced: `reduceTakePhase` (`live.ts:1152-1196`) only keeps them if already set, and the backend emits no stage event (RAN: the `.emit("…")` set in `src-tauri/src` is settings, license, recording, navigate, transcript, settings-tab, microphone-status, audio_level, status, shortcut, pill_visible, license_required, float, plus `transcribe_progress`). After decode the pill reads "Transcribing" (`phaseVisual` "thinking") through the LLM and the paste | yap24-PILL2 |
| P3 | **Hotkey is blind** (Secure Input, tap disabled, Accessibility missing) | `build_status` computes `secure.blocked`, tap health and accessibility (`lib.rs:893-922`); `float-main.tsx:60-65` reads only `recording/busy/engine_loading/last_error`. **LIVE:** >100 "Secure Input ENABLED by loginwindow/Google Chrome — the fn PTT event tap is blind" warnings | yap24-PILL3 |
| P4 | **Hands-free vs hold** | `hands_free` is on status (`lib.rs:896`), never read by the pill; **LIVE:** dozens of multi-minute hands-free takes | yap24-PILL4 |
| P5 | **Meeting after stop** (processing, notes ready, failed) | `MeetingBadge` is the recording state only (`pill/MeetingBadge.tsx:2`) | yap24-NT7 |

Also noted, not itemized separately: the paste-not-confirmed receipt (**LIVE** 4 lines) is a log
line, not a pill state — folded into PILL2 step 3.

**Keep:** the shell/character split, `PHASE_PRECEDENCE`, the hold/timeout policy, the tone table —
Y5-C (#193) built a sound machine; it is starved of events, not wrong.

---

## 4. Hotkeys and permissions

### 4.1 Latency path

`key-down` → CGEventTap callback (HID, listen-only, own run loop) → **280 ms hold-arm sleep**
(`ptt_macos.rs:494-509`) → `Start` → `start_recording` → persistent stream arm (µs warm; cold open
after 60 s idle, `record.rs:1316`) → buffering. The YV38 comment is right that 280 ms is the
tap-vs-hold decision window; the defect is that **capture waits for the decision**. There is no
pre-roll (**RAN** grep for pre-roll/backfill in `record.rs`, `rtring.rs`, `lib.rs`, `ptt_macos.rs`:
none). Change: arm speculatively on key-down, keep the buffer if it becomes a hold, discard on tap
(**yap24-OS1**). Expected gain: 280 ms earlier start and no clipped first word (UNVERIFIED until
OS1's latency test measures it).

### 4.2 Conflicts

- **Secure Input** blinds the tap (password fields, loginwindow at lock/unlock, some Chrome pages).
  Correctly detected by a 2 s poll (`secure_input.rs:41-48` — macOS publishes no notification for
  it); not surfaced on the pill (P3).
- **fn / Globe** is also the macOS emoji / input-source / dictation key depending on the user's
  keyboard setting. Whether Yap warns when System Settings binds fn to Apple Dictation is
  UNVERIFIED — worth a check inside OS3.
- **⌃⌘V / ⌃⌘M** are global shortcuts registered on every launch (**LIVE** log lines); no conflict
  detection against other apps (Wispr validates hotkeys against reserved combos — parity note line 74).

### 4.3 Permission findings

- The fn tap is **listen-only at the HID location** (`ptt_macos.rs:314-320`), which macOS gates by
  **Input Monitoring**; the failure copy says "enable Accessibility" (`ptt_macos.rs:323`). Use
  `CGPreflightListenEventAccess` / `CGRequestListenEventAccess` (macOS 10.15+,
  https://developer.apple.com/documentation/coregraphics/cgpreflightlisteneventaccess() and
  https://developer.apple.com/documentation/coregraphics/cgrequestlisteneventaccess()) — **yap24-OS3**.
- `NSMicrophoneUsageDescription` says "transcribe dictation with Whisper" (`Info.plist`); the default
  engine is Parakeet and meetings use the mic too — **yap24-OS3**.
- Grants are re-read on a 45 s poll only (`permissions.rs:542`) — add re-check on activation and
  per press (**yap24-OS3**).
- System Audio Recording has no read API; the heuristic must not call silence a denial (**NT3**).

---

## 5. UI/UX flow audit, screen by screen

| Screen | What works | Defects | Expansions |
|---|---|---|---|
| **Onboarding** (`Onboarding.tsx`, steps welcome → permissions → calibration → done, `:43,73`) | Refuses calibration without a real mic grant (PERM-D); model auto-download ribbon (yap15b) | Input Monitoring never requested (§4.3); ends without proving a pasted dictation (Y6-A open); offline first run has no explanation (PKG2) | End on one real pasted dictation (Y6-A); a 20-second "try the notetaker" step after NT9 |
| **Permissions** (`views/Permissions.tsx`, `PermissionHealthRow.tsx`) | One health surface for mic/AX/IM/audio capture (PERM-E, merged needs-human) | Grant noticed only on the 45 s tick; IM copy (§4.3) | Re-check on focus (OS3) |
| **Pill** (`float-main.tsx`, `pill/*`) | Phase machine, two characters, three docks, drag-to-dock, cancel | §3 P1-P5; not visually QA'd since Y5-C/Y5-D (STATUS-yap.md); Y5-E hover hysteresis killed mid-item (its partial commit is lost with the lane dir) | Integer pixel grid (UI1), one animation clock (UI2), screenshot matrix (UI3), Y5-I vertical-as-base, Y5-K character system |
| **Dictation** (hold/hands-free/command) | Formatting on by default, app-aware modes, undo-AI, long takes with progress, cancel mid-decode | 280 ms arm with no pre-roll (OS1); paste-not-confirmed invisible (PILL2) | Hotkey suite (Y8-A), mouse-button PTT (Y10-D), multi-language (Y10-A) |
| **Home** (`views/Home.tsx`, `home/YappyHouse.tsx`) | Living habitat, real stats, pixel voice | Own animation loop (UI2); Y5-B empty/loading/error states still an open PR (#192) | Habitat as the character's world (Y5-K) |
| **Meetings** (`views/Meetings.tsx`) | List, transcript render (single and two-track), FTS search, delete cascade, Markdown export | "still working through the audio" forever (`:339-347`); no progress, failed state, retry, notes or action items | NT6; ask-your-meeting (X3); notes mirror (X1) |
| **Insights** (`views/Insights.tsx`) | Voiced-WPM, streaks, monthly series | — | Insights v2 (Y10-E) |
| **Dictionary / Snippets / Scratchpad** | Auto-learn, snippets, scratchpad table | Scratchpad is half a feature (DB-D) | DB-D, CSV round-trip (Y9-E) |
| **Settings** (8 tabs, `views/settings/`) | Tabbed, persisted, license card | Launch-at-login uses a LaunchAgent (OS2) | Notes folder (X1), calendar reminders (NT8) |
| **Updates** (`updater.ts`) | Consent-based, signed | Feed dies if the repo goes private (UPD-A) | One-command release (PKG1) |
| **Menu bar** (tray, `lib.rs` Wispr-style dropdown) | Left-click dropdown, paste-last, state line from the pill's source (Y2-E) | No meeting post-stop state (NT7) | Hide-for-an-hour (Y6-B) |

Pixels and motion: YappyPill clamps DPR to 2 and sizes its canvas from the CSS box
(`pill/YappyPill.tsx:262`); `image-rendering: pixelated` is set (`float.css:379`,
`App.css:1969-1975`), but nothing forces an **integer** device-pixel cell, which is what "clearer
pixels" needs (**UI1**). Two independent animation loops (Yappy pill, habitat) and per-frame motion
are candidates for judder on ProMotion (UNVERIFIED per loop — **UI2** measures).

---

## 6. OS integration quality

| Area | Today | Verdict | Change |
|---|---|---|---|
| Mic capture | cpal (CoreAudio HAL) persistent stream, closes after 60 s idle | Keep — AVAudioEngine buys nothing for PTT; cpal already survives device swaps with the format watch | OS1 pre-roll; measure cold open |
| System audio | CoreAudio **process taps** via `objc2-core-audio` 0.3.2, runtime-gated to 14.4, weak-linked (`scripts/assert-weak-linked-14_4-symbols.sh`) | **Keep over ScreenCaptureKit**: no Screen Recording grant, audio-only purpose string, the decision was researched 2026-08-10 | NT3 honest verdict |
| Hotkey | CGEventTap listen-only at HID + Carbon-style global shortcuts via the plugin | Keep; correct grant naming (OS3) | OS1, OS3 |
| Paste | pasteboard + `CGEventPost` ⌘V with a read receipt | Keep; needs Accessibility; not sandboxable | PILL2 surfaces the receipt |
| Pill window | NSPanel, non-activating, status level, all Spaces, full-screen auxiliary; a space-keeper tick (`float_pill.rs:47`) | Keep; replace the tick with Space/screen notifications where possible (OS5) | OS5 |
| Launch at login | `tauri-plugin-autostart` LaunchAgent (`Cargo.toml:32-34`) | **Change** to `SMAppService.mainApp` on 13+ (https://developer.apple.com/documentation/servicemanagement/smappservice/mainapp) | OS2 |
| Menu bar + Dock | Regular activation policy (Dock + menu bar), template tray icon | Keep (Wilson wanted Dock presence) | — |
| Energy | Lazy ASR load (YV80), idle park ≤10 fps (YV81), polish idle-unload; **LIVE** idle RSS ~30 MB now (pid 90056), ~135 MB with engine warm (yap23 log) | Good; wakeups never measured | OS5, Y10-F |
| Sandbox | `app-sandbox = false` (`Entitlements.plist`) | **Keep — Wilson's rule** (utility apps are never sandboxed: CGEventTap, ⌘V posting and AX all break inside the sandbox) | — |
| Hardened runtime | `cs.allow-jit`, `cs.allow-unsigned-executable-memory` | **Change if unneeded** — prove per entitlement (OS4) | OS4 |
| Single instance | `tauri-plugin-single-instance` | Keep | Y6-D covers second-copy behaviour |

---

## 7. Wispr Flow parity — gaps that remain, and the wins Yap must own

Source: `~/Obsidian/Wilson-Brain/Notes/Wispr-Full-Parity-Research-2026-08-09.md` (asar teardown of
Wispr Flow 1.6.447) and memory `reference_wispr_parity_research`.

**Closed since the teardown:** paste-last (⌃⌘V), cancel (Y3-D), max session length (Y3-F),
drag-to-dock (YV65), press-enter (YV58), local notetaker **recording** (yap22), command mode, snippets,
dictionary, AX context.

**Still open (already itemized in the Y-plan unless marked):** vertical reflow when side-docked
(Y5-I); hover hysteresis / alpha hit-testing (Y5-E); in-bar affordance slots (Y8-B); coaching nudges
(Y8-D); earcons (Y8-C); hotkey suite completion and validation (Y8-A); scratchpad window (DB-D);
transform library (Y9-A); style profile from writing samples (Y9-B); spoken preference rules
(Y9-C); IDE context (PERM-G); multi-language picker (Y10-A); ranked mic list (PERM-H); mouse PTT
(Y10-D); meeting reminder pill (**yap24-NT8**); notetaker transcript/notes/action items
(**yap24-NT2/NT4**); "what did I miss" / notetaker chat (**yap24-X3**); MCP (**yap24-X2**).

**Wins Yap must own (and Wispr structurally cannot):**
1. **Works on a plane.** Wispr's transcription is cloud-only (teardown). Yap must make the
   offline first run honest or bundled (**PKG2**) — today it is the one place Yap also fails offline.
2. **Nothing leaves the Mac, provably.** No SDK, no upload path; the notetaker keeps audio local
   and deletes it only after the transcript is safe (**NT1**).
3. **Your notes outlive the app.** Markdown mirror to a folder you own (**X1**) and a read-only local
   MCP server (**X2**) — the "AIs can always retrieve transcripts even if Yap breaks" epic.
4. **A companion, not a bar.** The pixel character system (Y5-K) is Yap's identity.
5. **Price.** $29 one-time vs Wispr's subscription (memory: pricing decision 2026-08-10).

**Packaging expansions:** a one-command local release with a dry run (**PKG1**).

---

## 8. Decisions register — keep / change / kill

Sources: `ARCHITECTURE.md` decision table, `docs/loop/PLAN.md` §0 and owner decisions 2026-09-13,
memory `project_wilson_voice` / `project_yap_build_state`, and the code.

| # | Decision | Verdict | Reason | Evidence |
|---|---|---|---|---|
| D1 | Tauri 2 + React + Rust (no native Swift rewrite) | **Keep** | Pill + global key + paste all ship; a rewrite is 6-10 weeks for zero user gain | memory `project_wilson_voice`; NSPanel via tauri-nspanel works |
| D2 | `custom-protocol` feature required | Keep | Without it the UI loads localhost (blank window) | `ARCHITECTURE.md` |
| D3 | Stable signing identity (Apple Development locally, Developer ID for release) | Keep | TCC grants survive rebuilds only with a stable identity; ad-hoc resets them | SEC-A #165 |
| D4 | Mic in-process (cpal), not a helper | Keep | TCC attributes to the bundle id | `ARCHITECTURE.md` |
| D5 | In-process transcribe-cpp (GGUF, Metal), Python/MLX sidecar killed | Keep | Fresh-Mac install root cause fixed 2026-07-31 | memory; `Cargo.toml:70,108` |
| D6 | Parakeet default; Whisper only as tiny fallback | Keep | Accuracy/latency; **but** fix docs and usage strings that still say Whisper/MLX | `catalog.json`; `ARCHITECTURE.md:5`; `Info.plist` (Y6-E, OS3) |
| D7 | Polish and diarize as separate sidecars | Keep | ggml / onnxruntime link clashes with transcribe-cpp; isolation keeps dictation latency | `polish.rs`, `diarize.rs:16` |
| D8 | SQLite WAL + FTS5; no RAG, no GraphQL | Keep | Single-user local; FTS is enough for X2/X3 v1 | `ARCHITECTURE.md` |
| D9 | fn/Globe hold as the primary hotkey (CGEventTap) | Keep | Carbon cannot bind bare fn | `ARCHITECTURE.md` |
| D10 | 280 ms hold-arm before capture starts | **Change** | The window is the gesture; capture need not wait for it | `ptt_macos.rs:32,488-509` → OS1 |
| D11 | Persistent stream closed after 60 s idle | Keep (measure) | Mic-indicator privacy vs cold-open latency; OS1 records both numbers | `record.rs:1316` |
| D12 | Pill = NSPanel over full-screen via collection behaviour, Regular activation policy | Keep | Wilson wanted Dock presence; the panel floats regardless | memory; `float_pill.rs` |
| D13 | Pill is a pluggable character system (Classic + Yappy + habitat) | Keep | Owner decision 2026-09-13 | PLAN.md owner decisions §1; Y5-K |
| D14 | Pixel-art Tamagotchi aesthetic; origami rejected | Keep (locked) | Wilson | memory `feedback_companion_must_be_cute` |
| D15 | Pill silent while the trial has > 7 days | **Change** | Wilson 2026-09-26; half the trial is invisible | `pill/license.ts:136-165` → PILL1 |
| D16 | Take phase inferred from `status` without stage events | **Change** | Polishing/pasting unreachable | `live.ts:1152-1196` → PILL2 |
| D17 | Notetaker: CoreAudio process taps, two tracks never pre-mixed, 14.4 gate | Keep | Researched 2026-08-10; no Screen Recording grant | memory; `syscapture.rs` |
| D18 | Meeting ASR shares the one warm engine and yields to dictation | Keep — **and wire it** | Right design, zero callers | `meeting_asr.rs:1-45` → NT2 |
| D19 | Meeting audio retention: time-based, 7 days | **Change** | Must be state-aware (never before the transcript exists) | `db.rs:1936-1972` → NT1 |
| D20 | System-audio denial inferred from 3 s of silence | **Change** | Silence ≠ denial; in-person never needs the tap | `syscapture.rs:2243,2334-2341` → NT3 |
| D21 | Diarization IRL-first, sherpa-onnx CAM++ in a sidecar | Keep, gated on Y11-A..E | Six parked defects incl. FAR 1.000 on the shipped path | Y11 file |
| D22 | Summaries in yap-polish, GBNF JSON, 1.5B default, no 7B in v1 | Keep | Owner decision O2 2026-08-10 | memory |
| D23 | Consent: one TERMS line + one-time notice, no heavy consent UX | Keep | Owner decision O1 | memory |
| D24 | Licensing: offline Ed25519, 14-day trial, gate only new dictations | Keep | Privacy + never hold data hostage | `ARCHITECTURE.md`; `license.rs` |
| D25 | Issuer on the Forge box | **Kill** | Box decommissioned; no issuer exists (RAN 000) | LIC-A → Supabase Edge Function on a dedicated project |
| D26 | Updater feed on GitHub releases | Change before privatization | Dies with a private repo | UPD-A/UPD-B |
| D27 | Hand-notarized releases; CI release workflow disabled | Change | One scripted, dry-runnable command | PKG1 |
| D28 | Models downloaded on first use, sha256-pinned, nothing in the DMG | **Panel** | Secure and small, but no offline first run | PKG2 |
| D29 | arm64-only | Open (non-blocking) | DEFERRED #7 | — |
| D30 | `app-sandbox = false` | Keep (rule) | CGEventTap / ⌘V / AX break under the sandbox | memory `feedback_never_sandbox_utility_apps` |
| D31 | Hardened-runtime JIT + unsigned-executable-memory entitlements | **Change if unneeded** | Weakens the runtime; need is unproven | OS4 |
| D32 | No Sentry/PostHog; local crash capture + email button | Keep | Local-only is the brand | PLAN.md §0; YV64 |
| D33 | Launch-at-login via LaunchAgent plugin | Change | SMAppService.mainApp on 13+ | OS2 |
| D34 | Bundle id and data dir names frozen | Keep (irreversible) | TCC and history | HARNESS.md |
| D35 | Repo public until app completion | Keep (owner, 2026-08-11) | Free Actions minutes, but Actions is still dead account-wide → `ci=local` | memory; HARNESS.md CI mode |
| D36 | Loop gate local, 12 commands, both sidecars staged, measured cold | Keep | The only honest gate available | HARNESS.md |
| D37 | Loop harness: parts, two lanes, 3-agent cap, executed preflight/acceptance, halt on null | Keep; **added `args.only`/`args.now`** | Needed to run a chain without re-pre-flighting 30 finished items | `scripts/loop/template.mjs`, `build.mjs` |
| D38 | "Yap-first" Cortex/wake word | Kill (already) | Keyboard binding, not a wake word | memory `feedback_no_wake_word`; `WAKE-COMMAND.md` is historical |

---

## 9. Security

| Check | Result | Evidence |
|---|---|---|
| Data stays local | Yes: no analytics SDK; the only network calls are model downloads (pinned, sha256-verified), the updater feed, and the revocation GET (no identifiers sent). Audio never leaves the Mac. | `lib.rs:3784`; `models.rs:7-8`; `license.rs:117` |
| Model downloads verified by hash | Yes — ASR per-file sha256, polish and diarize pinned revisions + sha256, Silero sha256 (`vad.rs:496,508,549`); archive extraction capped (yap23 supply-chain tests) | `models.rs:93,189,300,369` |
| Update signature verification | Yes — Tauri updater minisign, mandatory, "cannot be disabled" (https://v2.tauri.app/plugin/updater/); pubkey pinned in `tauri.conf.json` | `tauri.conf.json` plugins.updater |
| Secrets in the repo | **RAN** `gitleaks detect` over 315 commits: 1 finding, a **false positive** — the updater minisign **public** key quoted in `scripts/cicd-loop.mjs:189` (commit `4a96cc3`). No private key, token or password. | gitleaks 2026-09-26 |
| License signing key | Public half only in the app; `tests/license_gate.rs` fails the build if a signing primitive appears in the shipped module | `ARCHITECTURE.md` |
| CSP | Strict: `default-src 'self'`, no remote script, `connect-src` ipc only, `object-src 'none'` | `tauri.conf.json` app.security.csp |
| Capabilities | One capability set for **both** windows includes clipboard read/write and global-shortcut register — the float window needs neither; least-privilege split is an inexpensive hardening (not itemized; fold into OS4 if the panel agrees) | `capabilities/default.json` |
| Sidecar IPC | Sidecar output treated as untrusted; validate_polish reject rules | `polish.rs:4` |
| Entitlements | sandbox false (rule), audio-input, JIT + unsigned-exec-memory (OS4) | `Entitlements.plist` |
| Crashes | 2 SIGSEGV in 0.8.0; frames unrecoverable (reports rotated) — UNVERIFIED cause; the crash ingest will catch the next one | LIVE `crash-summary.txt` |

---

## 10. The item plan

**Existing Y-plan (88 items), revised in place:** every item now carries a dated `notes:` status
line (the harness puts `notes` into the builder prompt, `template.mjs:825`).

| Status | Count | Items |
|---|---|---|
| Merged | 30 | PERM-A..E, Y0-C/D/E, Y1-A, SEC-A, SEC-C, Y3-A/B/C/D/F/G, Y4-A/C/E/F/G/H/I, Y2-A/C/E/F, Y5-C, Y5-G |
| Already-done (pre-flight) | 2 | Y1-B, Y4-D |
| Built, PR open (stale) | 11 | Y0-A #156, Y0-B #163, DB-B #178, LIC-A #183, Y2-B #186, Y5-A #190, Y2-D #191, Y5-B #192, SEC-B #195, Y5-D #196, UPD-A #197 |
| Not started | 43 | incl. Y5-E and UPD-B (killed in flight 2026-09-15) |
| Gated (owner) | 2 | DB-A, Y11-F |
| Obsolete | 0 | none proven obsolete — every unfinished item's premise was re-checked; UPD-A's premise is stale but the item is still needed before privatization |
| Amended | 10 | Y2-B, LIC-A, UPD-A, Y5-E, UPD-B, Y6-E, Y8-A, Y11-A (pre-flight rewritten), Y10-F, Y7-B |

**New yap24 items (26, all `gated: 'panel'`, each with evidence, steps, headless acceptance,
DEPENDS and `Panel: pending`):**

| File (lane) | Items | Theme |
|---|---|---|
| `01-yap24-notetaker.mjs` (B) | NT1-NT9 | capture durability → transcript → notes → UI → calendar → E2E |
| `02-yap24-pill-and-trial.mjs` (A) | PILL1-PILL4 | trial from day 14, stage events, blind states, hands-free |
| `03-yap24-hotkeys-permissions-os.mjs` (B) | OS1-OS5 | pre-roll arm, SMAppService, TCC truth, entitlement diet, idle timers |
| `04-yap24-ui-pixels-motion.mjs` (A) | UI1-UI3 | integer pixel grid, one clock, headless screenshot matrix |
| `60-yap24-packaging.mjs` | PKG1-PKG2 | one-command release, offline first run |
| `65-yap24-expansions.mjs` | X1-X3 | Markdown notes mirror, local MCP, ask-your-meeting |

**Passes:** (1) `only: ['yap24-NT']` — the notetaker chain alone; (2) `only: ['yap24-PILL',
'yap24-OS']` — two lanes in parallel; (3) `only: ['yap24-UI', 'Y5-']` with the open Y5 PRs;
(4) a full cold build pass for the remaining Y-items; then `mode: 'review'` once. The exact launch
lines live in the resume doc (`~/Obsidian/Wilson-Brain/Projects/Loop-Logs/YAP-RESUME-2026-09-26.md`).

**Harness change in this PR:** `args.only` (item-id prefixes; hard-skip non-matching items and whole
parts, build mode only) and `args.now` (echoed into the Recon log) ported from the Drivia harness;
the dry-run validator now executes a third arg shape (`{mode:'build', now, only, panelApproved}`).
`node scripts/loop/build.mjs` → "every check passed", 114 items, 2 parts.

---

## 11. UNVERIFIED and open questions

1. Why the system tap is gated at 14.4 when Apple documents 14.2 — the reason is in the 2026-08-10
   plan note, not re-verified here.
2. Whether `cs.allow-jit` / `cs.allow-unsigned-executable-memory` are needed (OS4 measures).
3. Per-loop ProMotion judder (UI2 measures).
4. The cause of the two 0.8.0 SIGSEGVs (reports rotated out).
5. Whether the Stripe Payment Link is active (memory says `active:false`; the page answers 200).
6. Whether fn is bound to Apple Dictation/emoji on Wilson's Mac and how Yap behaves then.
7. The real gain of OS1 in milliseconds (its test publishes it).

Sources fetched for this audit (headless):
- https://developer.apple.com/documentation/servicemanagement/smappservice (macOS 13.0)
- https://developer.apple.com/documentation/servicemanagement/smappservice/mainapp (macOS 13.0)
- https://developer.apple.com/documentation/coreaudio/audiohardwarecreateprocesstap(_:_:) (macOS 14.2)
- https://developer.apple.com/documentation/bundleresources/information-property-list/nsaudiocaptureusagedescription (macOS 14.2)
- https://developer.apple.com/documentation/coregraphics/cgpreflightlisteneventaccess() and cgrequestlisteneventaccess() (macOS 10.15)
- https://developer.apple.com/documentation/eventkit/ekeventstore/requestfullaccesstoevents(completion:) (macOS 14.0)
- https://developer.apple.com/documentation/bundleresources/information-property-list/nscalendarsfullaccessusagedescription (macOS 14.0)
- https://developer.apple.com/documentation/coreaudio/kaudiodevicepropertydeviceisrunningsomewhere
- https://v2.tauri.app/plugin/updater/
(Apple pages read through their `tutorials/data/documentation/*.json` endpoints, since the HTML is
script-rendered.)

---

## 12. Loop lessons that shaped this plan

From the Yap loop logs (`Projects/Loop-Logs/yap22b-2026-08-14.md`, `yap22c-2026-08-15.md`,
`yap23-2026-08-15.md`, `2026-09-12-yap-part01.md`, `STATUS-yap.md`):

1. **Merged mechanisms are not shipped behaviour.** yap22-B merged 10/10 tap items while "nothing in
   the shipped build actually calls `start_system_tap` in a real meeting" (yap22b reflection, lesson
   3). The notetaker ASR repeated it. Every yap24-NT item's acceptance proves a **call site**
   (source scan) and a **behaviour** (stop → segments), not only a unit.
2. **Two items owning one file need a rebase-time regression guard named up front** (`syscapture.rs`,
   yap22b refinement 2). NT2/NT3 both touch the stop path; NT3 DEPENDS on NT2 and runs after it on
   the same lane.
3. **In-loop finisher beats a separate drain** (yap22b headline): keep a stalled chain item on its
   lane and finish it in dependency order rather than parking it.
4. **An acceptance seat cannot tell "assertion failed" from "toolchain refused to run"** — six items
   burned a fix round on the Xcode 27 licence (exit 69). Accept the licence before launch; the
   resume doc's pre-launch block checks `/usr/bin/cc --version` exits 0 (it does as of 2026-09-26).
5. **Mutation checks must run on committed trees** — `git checkout -- .` reverted uncommitted fixes
   and shipped a test without its fix (yap23 line 817). NT items say "commit before any mutation".
6. **The wall-clock test flake** (`meeting_manual_start_stop` elapsed-clock test) recurred 3+ times;
   NT2 touches that suite — assert on payload `elapsed_seconds`, never on notification counts.
7. **Warm caches hide build-script failures** — the gate is measured cold (HARNESS.md); a pass-1
   Recon pays ~9.5 min and ~7.5 GB per lane.
8. **Visual QA debt compounds**: four merged/open UI items were never looked at. UI3 pays it
   headlessly before any further pixel work lands.

## Panel revisions 2026-09-26T17:35:00Z

Senior Panel (5 seats: CTO, Senior macOS/Rust engineer, Senior AI/Models, Senior PM, THE USER)
audited this document and the yap24 item files against decision ledger
`Loop-Logs/PANEL-yap24-2026-09-26.md`. All 61 findings deduped/classified there; every
BLOCKING/HIGH-GROUNDED finding below is applied in the item files
(`scripts/loop/items/*.mjs`) as a "Panel revisions 2026-09-26T17:35:00Z" block inside the
affected item, and in `scripts/loop/template.mjs`'s LAND-step prompt. This section corrects
claims elsewhere in this document that the panel's re-verification found stale or wrong;
treat any passage below that this section contradicts as SUPERSEDED.

**Corrections to this document:**
- **§10 "already-done" count.** `Y1-B` is NOT already-done. Its pre-flight grep matches a doc
  comment in `power.rs` (line 18, `//! ... NSWorkspaceWillSleepNotification path`), not a real
  observer registration; `meeting_matrix.rs` still carries 4 `absent_call_site:
  "NSWorkspaceWillSleepNotification"` entries today (confirmed by direct grep on 2026-09-26),
  which is Y1-B's own acceptance criterion for done. Correct the already-done count from 2 to 1
  (`Y4-D` only). See `docs/loop/items/05-y1-audio-permission.mjs`'s corrected `notes` and the new
  `yap24-NT10` item, which ships a meetings-scoped subset of this fix inside pass 1.
- **§9 revocation "harmless no-op."** Every launch sends an unauthenticated GET to
  `forge.87-99-149-214.sslip.io` (a decommissioned box's IP, via a wildcard DNS host Wilson does
  not control the certificate authority trust for) and accepts whatever unsigned `{kids:[]}`
  comes back with no signature check (`license.rs:1100-1124`). This is not harmless: it sends
  client IP and launch timing to a third party, contradicting both `license.rs:111-113`'s "the
  ONLY host this module ever contacts" and the "nothing leaves the Mac" privacy claim. Mitigation
  (`REVOCATION_URL: Option<String> = None` until the real endpoint exists) is now specified inside
  `LIC-A`'s Panel revision in `scripts/loop/items/20-y2-trial-and-limits.mjs` and should ship as
  its own tiny PR, independent of the rest of `LIC-A`.
- **§1.3 / §1.8 / D25 licensing destination.** Every reference to "a dedicated Yap Supabase
  project" is superseded by decision ledger item 8 (2026-09-26): issuance moves to the Drivia
  Consulting app's Supabase DB #2 (the products licensing backend, already provisioned and
  already seeded with a `yap` product and `/api/v1/licensing/*` routes), not a new one-off
  project. `LIC-A`'s spec is amended accordingly; PR #183 must not land as originally scoped.
- **Capture row ("Works ... OK").** Correct to "recording OK; crash recovery of an abandoned
  journal is UNWIRED" — `meeting::recover_orphaned_meetings` (`meeting.rs:1599`) has zero
  production callers (only a unit test and `meeting_matrix.rs` reference it), so a crash
  mid-meeting leaves the row stuck in `recording` forever. Fixed inside `yap24-NT1`'s Panel
  revision.
- **Models "sha256-verified before use."** Accurate only at download time; at load time the
  check is file size alone (`models.rs:806-812` `is_downloaded`). Reword to "sha256-verified at
  download; size-checked at load."
- **§7 win #5, "$29 one-time vs Wispr's subscription."** Downgrade to PENDING WILSON'S PRICING
  DECISION. The app hardcodes a $29 lifetime SKU (`license.rs:7,104`); the Drivia Consulting DB #2
  seeds Yap with `free`, `pro_monthly`, `pro_annual` plans only, `trial_days 0`, prices
  unset ("Wilson sets the price"). The two sides disagree on the product's own pricing shape —
  this is an owner decision (ledger item 9), not something this loop or this audit resolves. See
  `forWilson` in the panel log.
- **Notetaker phase-closing proof.** `yap24-NT9`'s original acceptance (`test -x` + `bash -n`
  only, script never executed, SKIP-on-missing-model exits 0) repeats this document's own
  RC-2 finding (135 green tests around a dead pipeline) one level up the chain. Fixed inside
  `yap24-NT9`'s Panel revision: acceptance now runs the script for real and SKIP is a hard
  failure under `YAP_E2E_REQUIRE=1`.

**New items added to the loop (see `scripts/loop/items/01-yap24-notetaker.mjs`):**
- `yap24-NT10` — a meeting survives a lid-close/sleep (registers the real observer this
  document's Y1-B entry wrongly counted as already done; scoped to meetings for pass 1).

**Items deferred out of pass 1** (full design notes in `docs/loop/DEFERRED.md` #9–#11):
`yap24-NT8` (calendar-aware Record? prompt — new permanent TCC permission, fixes nothing in
stop→transcript→notes), `yap24-NT5` (diarization — ships on a FAR-1.000 enrollment path per its
own unenforced DEPENDS), and (advisory sequencing only, unchanged in the files) `yap24-OS2`,
`yap24-OS4`, `yap24-OS5`, `yap24-X2`, `yap24-X3`.

**Harness change applied:** `scripts/loop/template.mjs`'s LAND-step prompt now tells the builder
agent, when `args.only` is set, to merge ONLY a PR whose branch matches one of those prefixes and
to skip every other open `loop-build`-labelled PR by name — closing the path by which a scoped
notetaker-only pass could otherwise land an unrelated stale PR (e.g. `LIC-A` #183, on a backend
the product ledger has since moved off).

Full seat-by-seat verdicts, every classified finding (PLAN-CHANGE / ITEM-CHANGE / DEFER /
REJECTED), the two killOne contradictions and their resolution, and the exact pass-1/pass-2
launch lines are recorded in `~/Obsidian/Wilson-Brain/Projects/Loop-Logs/PANEL-yap24-2026-09-26.md`
and `YAP-RESUME-2026-09-26.md`.
