# Yap: call detection, notetaker capture, bring-your-own-model, and where notes live

Written 2026-10-26T14:43Z (date -u; brief dispatch stamp said 14:50Z) by the research + spec seat. Main at `a740614`. Items: `scripts/loop/items/09-yap-calls-byom-2026-10-26.mjs` (10 items, all `gated: 'panel'`).

Wilson, 2026-10-26: "I hope the calling system can detect phone calls like FaceTimes or Zooms like Read AI can … a deep extensive look, build, rebuild and polishing on the meeting notetaker, the way it plugs into browser calls versus desktop calls … Then polish the notetaker. And we gotta build the API model plugin so people can bring their own models to do their note summaries and polishing, and they can connect their internal database on their phone or computer for where the data lives and notes live."

Every claim cites a URL or `file:line`. "UNVERIFIED" means nobody has checked it yet.

---

## 0. What this plan adds on top of what already exists

These items are already written. This plan **extends** them and never rebuilds them:

| Existing item | Owns | What this plan does with it |
|---|---|---|
| `yap-notetaker-detect` (08 file) | `meeting_detect.rs`, the "Record this meeting?" offer, the calendar signal, `tests/calendar_prompt_policy.rs` | Every CALL item **depends on it**. CALL-0 replaces how it attributes audio to an app (see §2.2: its "that app's audio" matched by PID is silent for browsers). |
| `yap-notetaker-onboard` | the setup card, EventKit read-only access | Unchanged. The calendar signal stays opt-in. |
| `yap-notetaker-notes` | track turns, live transcript, search, Markdown export, zero-network privacy test | RES-0 writes what its export produces. BYOM reuses its deny-all HTTP seam. |
| PR #250 (`feat/meeting-summary-default`, OPEN, draft) | `summary_jobs.rs` (`NotesBackend` trait), the 1.5B default notes model, map/reduce prompts | BYOM-0/1/2 add providers **behind** `NotesBackend::summarize`. They are held behind #250 by the dependency marker. |
| `yap24-NT0..NT11` (01 file) | durability, transcription, notes eval, sleep (NT10), echo (NT11) | Untouched. |
| `yap24-X1` (65 file, deferred) | notes mirror folder (`notes_mirror_dir`) | **RES-0 absorbs it.** RES-0 uses the same setting key, so X1's preflight passes and X1 retires as already-done. |
| `yap24-X3` (deferred), X2 (cut), MCP (deferred) | — | Not touched. No MCP is built here. |

## 1. Goals and non-goals

**Goals**
1. Yap notices a call in **any** app that uses the microphone for a call: Zoom, Teams, Meet, Slack huddles, Discord, WhatsApp, Webex, FaceTime, iPhone calls through the Mac Phone app, and browser calls in Chrome, Safari, Arc, Edge, Brave and Firefox. It asks "Call detected: record?" and never records without a click.
2. The capture is honest about what it got. Each meeting records which app was detected, which signal fired, and whether the call side (track 1) ever carried sound.
3. Bring your own model. Notes and polish can each run on the bundled model (the default), a local server (Ollama, LM Studio), any OpenAI-compatible endpoint (OpenRouter, a company gateway), or Anthropic. Keys are kept in the macOS Keychain. Every model choice carries a "Leaves this Mac: Yes/No" label.
4. "Connect your internal database" as v1 destinations: a folder of Markdown and JSON (works as an Obsidian vault), a signed webhook, a SQLite file the user picks, and an iCloud Drive mirror so notes show up on the phone. Each one carries the same Yes/No label.

**Non-goals (v1)**
- No meeting bot, no auto-record, and no recording without consent.
- No Notion, Google Drive, Dropbox, Salesforce or any other cloud SaaS connector. Those are deferred to "after the first 100 users"; yap-res-0 writes that line into `docs/loop/DEFERRED.md`.
- No Nango. Every connector is built from scratch.
- No MCP. It stays deferred.
- No Screen Recording permission. Yap has never needed it (Obsidian `Projects/Yap/wispr-notetaker-parity-2026-10-16.md:12`), and window titles through `CGWindowListCopyWindowInfo` would require it (Apple DTS: https://developer.apple.com/forums/thread/839069).
- No private API. In particular, no `responsibility_get_pid_responsible_for_pid`.
- No change to the 1.5B default model, the bundle id, the data directory, or sandboxing. Yap is never sandboxed.

## 2. Research

### 2.1 How the competition detects and captures calls without a bot

| Product | Capture | Detection signal | FaceTime / phone | Source |
|---|---|---|---|---|
| Granola | System audio plus mic. "cannot isolate audio from individual applications." | Calendar plus notification. Ad-hoc auto-stop "relies on detecting when your call app stops using the microphone." Below macOS 14.2, non-admin users may not get the call-ended signal. | Not addressed | https://docs.granola.ai/help-center/taking-notes/transcription.md |
| Granola, troubleshooting | Needs Microphone and "Screen & System Audio Recording" | — | — | https://docs.granola.ai/help-center/troubleshooting/transcription-issues.md |
| Read AI desktop | "Botless capture". Zoom, Teams, Meet, Slack Huddles, in person. macOS 14+. | Not documented | Not mentioned | https://read.ai/desktop |
| Wispr Flow Notetaker | "If your computer can play it, Notetaker can capture it" | "will detect that you are in a call and ask" (Meet, Zoom, Teams, WhatsApp, FaceTime). Auto-start is opt-in. | Claims FaceTime | https://wisprflow.ai/notetaker, via the Obsidian parity note lines 23-24 |
| Circleback | Desktop app | "detects when an app (like Zoom or Google Meet) starts using your microphone and sends you a notification". Per-app ignore list. Optional auto-start on mic use. | — | https://support.circleback.ai/en/articles/10460578-record-meetings-with-the-desktop-app, https://circleback.ai/ja/releases/ignore-specific-apps-when-detecting-meetings |
| MacWhisper | Needs Screen Recording plus Microphone | Fixed list: Arc, Brave, Chime, Chrome, Discord, Edge, Firefox, Safari, Skype, Slack, Teams, Telegram, Webex, WhatsApp, Zoom | **"FaceTime is not available due to privacy restrictions built into macOS."** | https://docs.macwhisper.com/article/30-record-meetings |
| tl;dv desktop | Computer audio, plus "any other sound … notifications or music" | "Live meeting notifications": audio-only call, Slack huddle, browser meeting | — | https://intercom.help/tldv/en/articles/14433337-how-to-record-without-the-bot |
| Krisp | Bot-free or bot. Local storage on Business plans. | UNVERIFIED (only third-party listings found) | — | https://deepgram.com/ai-apps/krisp-ai-meeting-assistant (third-party) |

**What the industry actually does.** The signal is **an app starting to use the microphone** (Circleback, Granola's auto-stop). Most products then add a list of known apps (MacWhisper) and the calendar (Granola, Wispr). Nobody documents per-app audio isolation, and Granola says outright that it cannot isolate per app. FaceTime is contested: MacWhisper says macOS blocks it, while Wispr claims support. That is an open spike (§6).

### 2.2 macOS APIs, and what this Mac actually reports

**(1) Mic-in-use per process.** Three CoreAudio pieces do this:
- `kAudioHardwarePropertyProcessObjectList`: "the Process objects for all client processes currently connected to the system" (SDK `CoreAudio.framework/Headers/AudioHardware.h:586`, `:633`).
- Per process object: `kAudioProcessPropertyPID`, `kAudioProcessPropertyBundleID` (a CFString), `kAudioProcessPropertyIsRunningInput` ("at least one active input stream") and `kAudioProcessPropertyIsRunningOutput` (`AudioHardware.h:1940-1978`; Apple: https://developer.apple.com/documentation/coreaudio/kaudioprocesspropertyisrunninginput). The header carries no availability macro on these selectors. Yap's meeting floor is 14.4 (`desktop/src-tauri/src/os_version_gate.rs:30`), so the exact OS version that introduced them does not matter to us.
- **Yap already enumerates them.** `syscapture::imp::process_output_activity()` (`desktop/src-tauri/src/syscapture.rs:3338`) reads PID plus IsRunningOutput plus `parent_pid` through `proc_pidinfo`, into `ProcessOutputActivity` (`syscapture.rs:2614-2623`). It does not yet read **IsRunningInput** or **BundleID**. CALL-0 adds both.
- `kAudioDevicePropertyDeviceIsRunningSomewhere` (`AudioHardware.h:918`) works only per device. **We measured that it is useless here**: on this Mac, with no call running, it reads `1`, because Yap itself holds the input open (probe below). It has to be the per-process signal, with Yap's own PID excluded.

**Measured on this Mac (macOS 26.6.2, 2026-10-26T14:4xZ, a read-only Swift probe of the selectors above; no TCC prompt, unsigned CLI):** 33 process objects were listed. Excerpt:
```
in=1 out=0 bundle=com.wilsonguenther.wilson-voice     (Yap itself, /Applications/Yap.app — holds the mic)
in=0 out=0 bundle=com.google.Chrome.helper   ppid -> /Applications/Google Chrome.app (attributable)
in=0 out=0 bundle=com.apple.WebKit.GPU (x4)  ppid -> 1 launchd   (Safari/WebKit: NOT attributable by ppid)
in=0 out=0 bundle=com.apple.avconferenced    ppid -> 1           (FaceTime's conferencing daemon)
in=0 out=0 bundle=com.apple.TelephonyUtilities  = callservicesd  (Continuity / phone calls)
default input device running somewhere: 1
```
Four findings follow from this probe:
- (a) Audio comes from **helper** processes, and the process object carries the helper's bundle id. This confirms the reason the panel gave for killing app-scoped taps (`syscapture.rs:13-25`, finding OS-5).
- (b) For Chrome, the parent PID resolves the browser (verified).
- (c) Safari/WebKit's GPU process is parented by launchd, so the parent PID attributes nothing. The only public fallback is to require that a known browser is running **and** that one of its AX window titles matches a call pattern.
- (d) FaceTime and phone calls run audio through **system daemons** (`avconferenced`, `callservicesd`), not through FaceTime.app or Phone.app. UNVERIFIED that either shows `in=1` during a live call. CALL-4 records that fixture.

**(2) System-audio capture.** Yap already ships a CoreAudio process tap, **global-exclude-self**, using `CATapDescription initMonoGlobalTapButExcludeProcesses` with exactly one exclusion, itself (`syscapture.rs:1-40`). It runs on 14.4+ and needs System Audio Recording (`NSAudioCaptureUsageDescription`, `desktop/src-tauri/Info.plist:11`). `AudioHardwareCreateProcessTap` is `API_AVAILABLE(macos(14.2))` (SDK `AudioHardwareTapping.h:44`), and Yap gates on 14.4 for the reasons at `os_version_gate.rs:30`. ScreenCaptureKit was rejected (`syscapture.rs:32`).
**New in macOS 26:** `CATapDescription.bundleIDs` ("the bundle ID of a process to tap or exclude") and `processRestoreEnabled` ("save tapped processes by bundle ID when they exit, and restore them … when they start up again") are both `API_AVAILABLE(macos(26.0))` (SDK `CATapDescription.h:131-167`; Apple: https://developer.apple.com/documentation/coreaudio/capturing-system-audio-with-core-audio-taps). That **may** answer OS-5's helper-process problem, but only on 26+. CALL-3 is a spike. The default stays global-exclude-self.

**(3) Frontmost app and window titles.**
- `NSWorkspace.frontmostApplication.bundleIdentifier` needs no permission and is already in `focus::frontmost_bundle_id()` (`desktop/src-tauri/src/focus.rs:403`).
- Window titles through AX (`AXUIElementCreateApplication(pid)` → `AXWindows` → `kAXTitleAttribute`) need the **Accessibility** grant. Yap already holds that for paste and already reads `AXTitle` in `focus.rs:313`.
- `CGWindowListCopyWindowInfo` window names need Screen Recording (https://developer.apple.com/forums/thread/839069), so we do not use them.
- **Permission truth:** if Accessibility is not granted, a browser call shows as "Browser call (site unknown)". The pill never guesses.

**(4) Bundle ids.**

| App | Bundle id | Verified how |
|---|---|---|
| Zoom | `us.zoom.xos` | `defaults read /Applications/zoom.us.app/Contents/Info CFBundleIdentifier`, this Mac |
| FaceTime | `com.apple.FaceTime` | on disk, this Mac |
| Phone (iPhone calls on Mac, macOS 26) | `com.apple.mobilephone` | on disk, `/System/Applications/Phone.app`. The app came with Tahoe: https://www.apple.com/newsroom/2025/06/macos-tahoe-26-makes-the-mac-more-capable-productive-and-intelligent-than-ever/ |
| iPhone Mirroring | `com.apple.ScreenContinuity` | on disk |
| Microsoft Teams (new) | `com.microsoft.teams2` | https://learn.microsoft.com/microsoftteams/teams-client-bulk-install |
| Chrome / helper | `com.google.Chrome` / `com.google.Chrome.helper` | on disk, plus the probe |
| Safari / WebKit audio | `com.apple.Safari` / `com.apple.WebKit.GPU` | on disk, plus the probe |
| Slack `com.tinyspeck.slackmacgap`, Discord `com.hnc.Discord`, WhatsApp `net.whatsapp.WhatsApp`, Webex `Cisco-Systems.Spark`, Arc `company.thebrowser.Browser`, Edge `com.microsoft.edgemac`, Brave `com.brave.Browser`, Firefox `org.mozilla.firefox` | — | **UNVERIFIED** (not installed here). CALL-4's walk ledger verifies each one with `defaults read`, and the catalog marks a row `verified: false` until then. |
| Meet PWA (Chrome app shim) | `com.google.Chrome.app.<id>` | UNVERIFIED. `~/Applications/Chrome Apps.localized` exists here. Attribution goes through the Chrome helper either way. |

**(5) What Yap has today**
- `syscapture::imp::process_output_activity` (`syscapture.rs:3338`) and `ProcessOutputActivity` (`:2614`).
- `TapEnvironment.system_output_active` (`:683`).
- The global tap via `open_meeting_tap` (`meeting_control.rs:845/901`) and `track_b_plan` (`meeting_control.rs:810`).
- `toggle_with_kind` (`meeting_control.rs:753`).
- The pipeline worker (`meeting_pipeline.rs:218 spawn`).
- `focus::frontmost_bundle_id` (`focus.rs:403`) and `frontmost_app_name` via AXTitle (`focus.rs:299`).
- **Missing on main:** `meeting_detect.rs`, `tests/calendar_prompt_policy.rs` (both are `yap-notetaker-detect` deliverables, not built), any IsRunningInput read, any BundleID read, any browser-title matching.

### 2.3 Bring your own model

| Provider | Endpoint | Auth | Notes |
|---|---|---|---|
| Ollama (local) | `http://localhost:11434/v1/chat/completions`, `GET /v1/models` | Key required by clients but ignored | https://docs.ollama.com/api/openai-compatibility |
| LM Studio (local) | `http://localhost:1234/v1/chat/completions`, `GET /v1/models` | Not stated | Port is "assumed" in the docs. https://lmstudio.ai/docs/developer/openai-compat |
| OpenRouter | `https://openrouter.ai/api/v1/chat/completions` | `Authorization: Bearer` | Optional `HTTP-Referer`, `X-OpenRouter-Title` / `X-Title`. https://openrouter.ai/docs/api-reference/overview |
| Anthropic (native) | `POST https://api.anthropic.com/v1/messages`, `GET /v1/models` | `x-api-key` or `Authorization: Bearer`. `anthropic-version` is required. `anthropic-workspace-id` is required for keys not scoped to a workspace. | https://platform.claude.com/docs/en/api/overview |
| Anthropic OpenAI shim | `https://api.anthropic.com/v1/` | — | "not considered a long-term or production-ready solution". `response_format` is ignored. https://platform.claude.com/docs/en/cli-sdks-libraries/libraries/openai-sdk. **Therefore a native Anthropic provider (BYOM-2), not the shim.** |

- **Detecting a local server.** Send `GET {base}/models` with a 1.5 s timeout. Both local servers document it (links above). Probe only on an explicit "Find local models" click, never in the background.
- **Keys.** Today the licence key sits in `license.json`, not the Keychain (`desktop/src-tauri/src/license.rs:179`, `:499`), and no code in `src/` calls `SecItem*`. `security-framework` 3.7.0 is already in `desktop/Cargo.lock`, pulled in by `native-tls` / `rustls-platform-verifier`, so declaring it directly adds no new crate. Its `passwords::set_generic_password` / `get_generic_password` / `delete_generic_password` API: https://docs.rs/security-framework/latest/security_framework/passwords/index.html. Service `com.wilsonguenther.wilson-voice.byom`, account = provider id. Keys never go to SQLite, settings, logs or crash reports.
- **Streaming.** Not needed in v1. Notes are a background job behind `SummaryJobs` (#250 `summary_jobs.rs:182-200`, `trait NotesBackend { model_ready; fetch_model; summarize(id, cancel) }`). Non-streaming with a hard timeout is simpler and still lets cancel work: drop the request when the `CancelToken` fires. Dictation polish has a deadline (`PolishRequest` deadline_ms, `polish_protocol.rs:97-160`). A remote polish must answer inside that deadline or the take is inserted unpolished, exactly as a sidecar timeout is today.
- **Where providers plug in.** For notes: one `NotesBackend` implementation per provider. Its `model_ready()` returns true for remote providers, and `fetch_model` becomes a no-op. The prompts and map/reduce budgets in `summarize.rs` are reused. Each provider gets its own context budget setting, default 8k tokens. For polish, the stage reads `settings.polish_model` (`lib.rs:366`). A new `polish_provider` sits beside it, `polish_model` stays the bundled-model override (ledger), and empty means bundled.

### 2.4 Where the data lives ("connect their internal database")

What competitors offer:
- Granola shares to Notion **note by note**, with no bulk sync. Automatic sending goes through Zapier (https://docs.granola.ai/help-center/sharing/notion.md, https://docs.granola.ai/help-center/sharing/integrations/integrations-with-granola).
- Wispr's notes leave through a cloud MCP that requires Cloud Sync (Obsidian parity note lines 25, 28).
- Yap already has a Markdown export (`export_meeting_markdown`, `desktop/src-tauri/src/lib.rs:5330`).

v1 destinations. All run on the Mac and are written by Yap after the notes reach `notes_ready`:

| Destination | Leaves this Mac | Mechanism |
|---|---|---|
| Folder (Markdown + JSON sidecar, Obsidian front matter) | **No** | atomic temp-then-rename, never deletes a user file |
| SQLite file the user picks | **No** | `rusqlite` (bundled, `Cargo.toml:46`), a fixed documented schema `yap_meetings`, `yap_segments`, upsert by meeting id |
| Webhook POST (JSON) | **Yes** for any non-loopback host, **No** for `127.0.0.1`/`localhost` | `X-Yap-Signature: sha256=<hex HMAC-SHA256(secret, body)>` plus `X-Yap-Timestamp`. Same scheme as GitHub's `X-Hub-Signature-256` (https://docs.github.com/en/webhooks/using-webhooks/validating-webhook-deliveries). Secret in the Keychain. Retries 3x with backoff, then "failed" on the meeting card. |
| iCloud Drive mirror (`~/Library/Mobile Documents/com~apple~CloudDocs/Yap/`, exists on this Mac) | **Yes** (Apple iCloud) | the folder destination aimed at iCloud Drive. Appears in Files on the iPhone. UNVERIFIED that a non-sandboxed app writing there syncs with no entitlement. RES-1 walk-verifies it. |
| Notion, Google Drive, Dropbox, generic Postgres | — | **DEFERRED**; yap-res-0 writes the line into `docs/loop/DEFERRED.md` |

## 3. Detection architecture: signal fusion

`meeting_detect.rs` (created by `yap-notetaker-detect`) gets a pure function:
`fn score(snapshot: &CallSnapshot, catalog: &CallCatalog, memory: &OfferMemory) -> Option<CallCandidate>`.

`CallSnapshot` is plain data. That keeps it fixture-testable: the recorded probe JSON *is* the test input.
- `procs: Vec<ProcAudio { pid, parent_pid, bundle_id, running_input, running_output }>`, with Yap's own PID and children removed.
- `running_apps: Vec<{ pid, bundle_id }>` (NSWorkspace).
- `frontmost: Option<bundle_id>`.
- `browser_titles: Option<Vec<{ bundle_id, title }>>`. `None` means Accessibility is not granted, which is different from an empty list.
- `calendar: Option<NextEvent>`. Only when the user connected the calendar.

Attribution, in order:
1. A process bundle id is in the catalog: a native call app.
2. A helper bundle id (`*.helper`, `*.helper.*`) whose `parent_pid` resolves to a catalog browser: a browser.
3. `com.apple.WebKit.GPU` while Safari is running: Safari, low confidence until a title matches.
4. `avconferenced` / `callservicesd` with `running_input`: FaceTime or Phone, depending on which of the two apps is running.

Scoring (0–100; offer at ≥ 60):

| Evidence | Points |
|---|---|
| Attributed call app has `running_input` (the Circleback/Granola signal) | +60 |
| …and `running_output` | +10 |
| Browser title matches a call pattern (`Meet - `, `meet.google.com`, `Zoom`, `teams.microsoft.com` / `Microsoft Teams`, `Huddle`, `Discord`, `Whereby`, `Webex`) | +30 |
| A connected calendar event with a call link starts within 2 min, or started under 5 min ago | +20 |
| The call app is frontmost | +5 |
| A browser with input but no title match and AX granted (a mic test, a voice-search page) | cap at 55 → no offer |

So the outcomes are: a native app using the mic offers. A browser with the mic open **and** a matching title offers. A browser with the mic open and no AX grant offers as "Browser call (site unknown)" only if a calendar event backs it up.

Cadence is event-driven:
- NSWorkspace launch/terminate notifications, plus a property listener on `kAudioHardwarePropertyProcessObjectList`, plus a single 30 s timer. Never faster (ledger: polling ≥ 30 s).
- The **listener** is what makes detection prompt without polling. CALL-0 adds it. Until then, the 30 s timer is the floor.
- Before an offer fires, the AX title read runs once per candidate.

**The pill prompt (CALL-1):**
- "Call detected: <App> — Record?" with the consent line (`desktop/src/meetings/consent.ts`), **[Record] [Not now]** and a **10 s auto-dismiss**. Auto-dismiss counts as Not now for that call. It never counts as Record.
- Per-app memory in `settings_kv` `call_offer_policy_v1`: `{bundle_id: "ask" | "always_offer" | "never"}`. "Never for <App>" sits in the expanded pill and in Settings › Meetings. "Always" means *always offer*, never *always record*. There is no auto-record setting, and the grep gate from `yap-notetaker-detect` stays.
- While recording, if the detected app's `running_input` drops for 20 s, the pill shows "Call ended? Stop recording" with [Stop] [Keep going]. Nothing stops on its own (open question 2).

## 4. Capture architecture
- **Default (unchanged):** mic track 0 plus global-exclude-self system tap track 1 (`syscapture.rs:13-40`). The meeting row records `detected_app` (bundle id), `detected_via` (`mic_in_use|title|calendar|manual`) and the tap verdict.
- **Per-app tap (CALL-3, a spike):** on macOS 26+ only, `CATapDescription.bundleIDs = [app, app.helper…]` plus `processRestoreEnabled`. It ships behind a setting that defaults OFF and is turned on only if the spike's evidence shows non-zero samples from Chrome Meet and Zoom **and** that the silence discriminator (`TapLiveness::ever_nonzero`, `syscapture.rs:196`) still separates denial from wedge. Otherwise the result is NO-GO, recorded, and global stays.
- **FaceTime / Phone:** whether the global tap hears the far side of a FaceTime or Continuity call is **unknown**. MacWhisper says macOS blocks it, and Wispr claims it works. CALL-4 measures it. If the far side is silent, the pill says so truthfully: "FaceTime: only your side is recorded". It never claims a two-sided recording.

## 5. BYOM provider model

```rust
pub enum ProviderKind { Bundled, OpenAiCompatible, Anthropic }
pub struct ProviderConfig { id: String, kind: ProviderKind, label: String, base_url: Option<String>,
                            model: String, context_tokens: u32, timeout_s: u32 /* key lives in Keychain under id */ }
pub trait ChatProvider: Send + Sync {
    fn complete(&self, system: &str, user: &str, max_tokens: u32, cancel: &CancelToken) -> Result<String, ProviderError>;
    fn leaves_this_mac(&self) -> bool;   // false for Bundled and loopback base_url
}
```
- **Per-task assignment:** `notes_provider`, `polish_provider` (settings; empty = Bundled).
- **Test connection:** a 10-token completion (`max_tokens: 10`) with a 10 s timeout. It shows latency, the model echo, or the exact HTTP status in plain words. No key is ever echoed.
- **Timeouts:** notes 120 s per map/reduce call. Polish uses the take's deadline.
- **Fallback:** any provider error on a notes job reruns that job on the Bundled model. The meeting card then carries the truthful banner "Notes written by the built-in model — <label> didn't answer (<reason>)". Polish falls back to the existing unpolished-insert path.
- **Privacy test:** with every provider Bundled, the summarize and polish paths make zero network calls. This reuses the deny-all seam from `tests/meeting_notes_stay_local.rs` (`yap-notetaker-notes`).
- **Label:** each provider row and each meeting card shows "Leaves this Mac: Yes — <host>" or "No". The README sentence "your words never leave the Mac unless you point them somewhere" stays true by construction.

## 6. The three hardest sub-problems

1. **Attributing a call's audio to an app.** Browsers and FaceTime play through helpers and daemons.
   - Chrome-family: **real answer**. The probe proves the helper's parent PID resolves Chrome.
   - Safari/WebKit: **spike**. The GPU process is parented by launchd, so this rests on the title match plus Safari running.
   - FaceTime and Phone: **spike**. `avconferenced` / `callservicesd` must show `running_input=1` during a live call.
   - What makes it real: the CALL-4 walk records a JSON snapshot per app *during a real call*, and each one becomes a committed fixture CALL-0's tests score.
2. **Hearing the far side of FaceTime and iPhone calls.** **Hope** until measured. MacWhisper (docs above) says macOS forbids it, and Wispr markets it. What makes it real: one CALL-4 walk where track 1 shows non-zero RMS during a FaceTime call through the existing global tap. Until then the pill promises "your side".
3. **Capturing only the call app's audio.** **Spike.** It is possible only on macOS 26 (`CATapDescription.bundleIDs`). The 14.4–15.x users keep global-exclude-self, which already works and already exists. What makes it real: the CALL-3 evidence file `SPIKE-VERDICT GO` with per-app non-zero samples, and a silence discriminator that still distinguishes denial.

## 7. Runtime dependencies (nothing "found on the machine")

| Dependency | Ships in the app / OS / user installs | Used by |
|---|---|---|
| CoreAudio process objects + taps | OS (14.4+) | CALL-0, CALL-3 |
| AppKit NSWorkspace, ApplicationServices AX | OS | CALL-0, CALL-2 |
| EventKit (read-only, opt-in) | OS | existing `yap-notetaker-onboard` |
| `security-framework` (Keychain) | ships (already in Cargo.lock) | BYOM-0, RES-0 webhook secret |
| `reqwest` | ships (`Cargo.toml:74`) | BYOM-1/2, RES-0 webhook |
| `hmac` crate (+ `sha2`, already `Cargo.toml:75`) | ships (new small crate) | RES-0 |
| `rusqlite` bundled | ships (`Cargo.toml:46`) | RES-0 SQLite destination |
| Bundled 1.5B notes model + llama.cpp sidecar | app downloads it, sha-pinned (#250) | default and fallback |
| Ollama / LM Studio | **user installs, optional**. Yap never requires them, and the fallback is Bundled. | BYOM-1 presets |
| OpenRouter / Anthropic / custom endpoints | **user's account, optional** | BYOM-1/2 |
| iCloud Drive | OS + user's iCloud, optional | RES-1 |

## 8. Privacy labels (one vocabulary, everywhere)

- "Leaves this Mac: No" is shown for Bundled, loopback model servers, folder, SQLite, and a loopback webhook.
- "Leaves this Mac: Yes — <host>" is shown for cloud models, a remote webhook, and iCloud Drive ("Yes — Apple iCloud").
- The label appears on the Settings row, on the meeting card that used it, and in the Test-connection result. If any Yes is active, a one-line summary sits at the top of Settings › Meetings: "2 things send notes off this Mac: OpenRouter (notes), iCloud Drive mirror."

## 9. Open questions for Wilson (owner-only)

1. **Cloud models for dictation polish.** Notes on a cloud model is one request per meeting. Polish on a cloud model sends **every dictation** off the Mac. Should v1 let the user pick a cloud model for polish, or allow local servers only for polish? Default if unanswered: allowed, with the Yes label on the dictation pill's settings row.
2. **When the call ends.** The default prompt is "Call ended? Stop recording" with no auto-stop. Should it instead auto-stop 60 s after the call app releases the mic, as Granola does? Default if unanswered: prompt only.
3. **What destinations receive.** Meetings only, or meetings plus a daily dictation log (X1 offered it as optional)? Default if unanswered: meetings only, with the dictation log offered as a separate opt-in in the folder destination.

## 10. Items

See `scripts/loop/items/09-yap-calls-byom-2026-10-26.mjs`. All are panel-gated. The order is the file order.

| id | title | class | depends |
|---|---|---|---|
| yap-call-0 | Call signal matrix: per-process input + bundle id, a catalog, a fused score, recorded fixtures, a probe CLI | SEC | yap-notetaker-detect |
| yap-call-1 | "Call detected — Record?" with 10 s auto-dismiss, per-app memory, call-ended prompt | SEC | yap-call-0 |
| yap-call-2 | Browser calls through AX window titles, with permission truth | SEC | yap-call-0 |
| yap-call-3 | Per-app tap on macOS 26 (SPIKE, default off) | SEC | yap-call-0 |
| yap-call-4 | Native-app coverage matrix + walk ledger (Zoom, Teams, FaceTime, Phone, Slack, Discord, WhatsApp, Webex, iPhone Mirroring) | SEC | yap-call-0, yap-call-2 |
| yap-byom-0 | Provider model, Keychain, Settings › Models, test connection, privacy labels | SEC | PR #250 |
| yap-byom-1 | OpenAI-compatible provider (Ollama, LM Studio, OpenRouter, custom) behind notes and polish, with truthful fallback | SEC | yap-byom-0 |
| yap-byom-2 | Native Anthropic provider | SEC | yap-byom-0 |
| yap-res-0 | Destinations: folder/Obsidian (absorbs yap24-X1), SQLite file, signed webhook, with labels | SEC | PR #250, yap-byom-0 |
| yap-res-1 | iCloud Drive mirror, visible on the phone | SEC | yap-res-0 |


## Panel revisions 2026-10-26

Stamped 2026-10-26T14:58:31Z (date -u). Senior Panel on calls + BYOM + destinations: seat A = Security + macOS, seat B = PM + The User +
Pre-mortem, synthesis = Sonnet. Both seats ran all ten preflights on main a740614 (10/10 held, DEPENDENCY-NOT-ON-MAIN). This
section supersedes the text above wherever they differ; nothing above is rewritten.

**Verdict: revise-first (applied in the items).** v1 set = yap-call-0, yap-call-1, yap-call-2, yap-byom-0, yap-byom-1, yap-res-0.
Folded: yap-call-4 into yap-call-0, yap-res-1 into yap-res-0. Deferred to the next release: yap-call-3 (kept gated, spec corrected),
yap-byom-2, the SQLite destination, the Safari/Firefox browser path.

### Findings by class (after dedupe)

PLAN-CHANGE (3): the v1 ship set above; ownership of the app/browser signal moves from yap-notetaker-detect (b) to yap-call-0 +
yap-call-2 (one owner per deliverable); two folds (call-4, res-1).

ITEM-CHANGE (35), every one encoded as a gate in 09-yap-calls-byom-2026-10-26.mjs (and 08 for detect):
- yap-call-0 (6): per-process IsRunningInput listeners re-registered on every ProcessObjectList change (a process-list listener
  never fires when input starts), 30 s backstop, timer off when "Offer to record" is off; --probe-calls prints {pattern_id, len},
  never a raw title (and a no-"title"-field fixture test); scope the syscapture.rs:28-29 comment, fix the AudioHardware.h cite
  (:1015); Yap's idle state is in=0 (two fixtures), WebKit.GPU scored only with a Safari title; the walk script prints which AX
  grant it saw and never verifies a browser row on a Terminal-AX run; FaceTime/Phone default to "your side only".
- yap-call-1 (6): consent.ts:68 copy made conditional (consentNotice) + CONSENT_PILL_LINE; accept_call_offer(offer_id) checked in
  the backend + a policy test that only UserChoice::Record starts recording; the offer persists as a one-click chip after the 10 s
  expiry; Offer / Never only; "! grep auto_stop" paired with a positive "Call ended" grep; copy "Zoom call — record it?" /
  "FaceTime call — record your side?" / "Let them know you're recording."
- yap-call-2 (1): say plainly that the title is the FRONT tab (a background-tab call is not offered; the safe failure).
- yap-call-3 (1): the correct tap construction written in (below).
- yap-byom-0 (6): Keychain account = provider id + host (a host change deletes the key), service from the running bundle id,
  test_model_provider takes the provider id only; sentinel-key leak test replaces the grep; ONE reqwest factory with
  redirect::Policy::none() + a mock-307 test; hide unwired kinds (no "Coming in this release"); "-cloud" / ":cloud" models label
  "Yes — ollama.com"; consent wiring.
- yap-byom-1 (7): prompts move into polish_protocol.rs with system/user framing, no tools, non-text = bad response, a byte-identity
  test; its own zero-network test; fallback only if model_ready(), else "Download & retry"; circuit breaker + 10-minute job cap;
  remote polish skips password fields and code mode; Ollama/LM Studio context 4096 with chunk cap = min(1400, ctx - out);
  cloud polish: Test connection vs the deadline + "polished X of last N takes"; RAM note on <= 8 GB Macs.
- yap-byom-2 (1): the x-api-key / redirect rule rides the byom-0 factory.
- yap-res-0 (5): filename sanitiser + path-inside-folder + quoted YAML; hash-of-last-write and "(Yap update).md"; its own worker
  with a persisted outbox; one webhook scheme (timestamp.body HMAC-SHA256, GitHub label dropped, stable X-Yap-Delivery, fresh
  signature per retry, 300 s window, 32-byte secret shown once, TLS except loopback, verify snippet); the iCloud/Obsidian/
  CloudStorage classifier.
- yap-notetaker-detect (2): signal (b) struck; its "Recording is sacred" grep replaced by CONSENT_PILL_LINE gates.

DEFER (7): yap-call-3 build -> next release; yap-byom-2 -> next release; SQLite destination -> next release (with seat A's no-WAL and
no-iCloud-path rules); Safari + Firefox browser path -> next release; local offer->Record and first-destination counters -> next
release (a local counter, no telemetry); a user-editable notes prompt (seat B's one "SHORT" against MacWhisper) -> next release;
hiding the offer chip from screen capture -> OWNER decision (below).

REJECTED (3): seat B's byom-0 acceptance grep for "leaves_this_mac" in consent.ts (an import satisfies it; a vitest replaces it);
seat B's CUT of yap-call-3 from the repo (the item stays, deferred, because the corrected spec is cheap to keep and OS-5 needs an
answer someday); seat A's broader "any loopback model name containing cloud" (narrowed to the ledger rule "-cloud" / ":cloud";
the broader form would false-label a local model named after a company).

### Duels (settled with evidence)

(a) yap-call-3: B cut vs A keep. Winner: DEFER with A's fix written in. Evidence: the item is default OFF and NO-GO until Wilson walks
it, so no user sees it pass; and CATapDescription.h says exclusive = "tap all processes except the process listed", so bundleIDs on
today's exclusive description taps everything EXCEPT the call app. The corrected construction is in the item now.
(b) yap-call-4: B split vs A keep-with-changes. Winner: B (fold into call-0), carrying A's constraints. Evidence: its acceptance
passed with every row UNVERIFIED; the machinery (call-walk.sh, ledger test) is cheap and belongs with the fixtures; the walk is a
human act (checklist below). A's three constraints (no raw titles, print which grant was seen, browser rows never verify on a
Terminal-grant run) are in call-0.
(c) yap-res-1: both said fold. Done: the classifier covers Mobile Documents, CloudStorage and else No; res-1 is a tombstone.
(d) yap-byom-2: B defer vs A keep-with-redirect-fix. Winner: B (defer), with A's rule in byom-0's factory.
(e) SQLite destination: B defer vs A keep-with-rules. Winner: B (defer); A's rules recorded for the build.
(f) call-2 Safari: both defer. Chrome-family ships.

### The correct app-scoped tap (yap-call-3, for the day it is built)

Build the AppScoped description with initMonoMixdownOfProcesses:@[] , THEN setExclusive(false), THEN setBundleIDs([the app's bundle
id + helper ids]), setProcessRestoreEnabled(true). Global keeps initMonoGlobalTapButExcludeProcesses with exclusive == true.
tests/tap_app_scoped_plan.rs asserts exclusive == false for AppScoped and == true for Global. Safari is excluded from app-scoping
(com.apple.WebKit.GPU taps every WKWebView process, Yap's own included).

### Webhook scheme (supersedes §2.4)

§2.4 said HMAC(secret, body) "same as GitHub". The shipped scheme is the item's: X-Yap-Signature = sha256=HMAC-SHA256(secret,
timestamp + "." + body) with X-Yap-Timestamp, X-Yap-Delivery (stable across retries), X-Yap-Event-Id (meeting id : notes revision),
300 s replay window. It is NOT GitHub's body-only X-Hub-Signature-256 and nothing in Yap or its docs calls it that.

### Wilson's walk checklist (yap-call-0 / call-2 / res-0; scripts/call-walk.sh prints these)

Run from Yap's own build for the Accessibility rows; a CLI run from Terminal reports TERMINAL's grant and can never verify a
browser row. Nothing here is automated; the loop has no mic, no TCC and no call. For each, record the --probe-calls snapshot
(titles come out as pattern id + length, never text) and the meeting diagnostics row, and say UNVERIFIED if you did not run it.
1. Zoom desktop, in a call: offer reads "Zoom call — record it?"; leave it 10 s; the "Record Zoom call" chip is still there; click it.
2. Chrome with Meet in the FRONT tab, Accessibility granted to Yap: offer appears; then put Meet in a background tab: no offer.
3. Chrome with a YouTube tab playing (output only): no offer.
4. FaceTime call: offer reads "FaceTime call — record your side?"; after recording, is track 1 non-zero? If yes, flip the row.
5. iPhone call via the Phone app on the Mac: same question as 4.
6. Teams, Slack huddle, Discord, WhatsApp, Webex: one line each, detected Y/N and which process carried the input.
7. Safari with Meet: record the snapshot only (the path is deferred); expect no offer.
8. The 10-minute listener check: start a Zoom call and confirm the per-process IsRunningInput listener fires for an
   already-registered HAL client. If it does not, latency is the 30 s backstop.
9. Notes folder inside iCloud Drive and inside an Obsidian iCloud vault: the label reads "Yes — Apple iCloud"; the note appears in
   Files on the iPhone; note whether the first write raised a Files & Folders prompt (UNVERIFIED today).
10. Edit a note in Obsidian, then re-run notes: your edit is kept and a "(Yap update).md" appears.

### Owner-only (not decided here)

The pill has no content protection, so the call offer is visible in a full-screen share. Hide the offer (never the recording dot)
from screen capture? And the iCloud first-write permission behaviour is UNVERIFIED until step 9.

### Decided after the panel

2026-10-26 Wilson: hide the offer only, never the recording dot. yap-call-1 DO 8 now sets content protection on the call-offer window only (content_protected_for), and its tests prove the pill/recording-dot window is never protected.
