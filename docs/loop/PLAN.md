# Yap overhaul — audit and build-item plan

**Authored** 2026-09-12. **Audited against** `origin/main` @ `4e8c9adf` (YV126, 2026-08-16).
**Items** 79, in 12 files. **Panel-gated** 2. **Validator** `npm run loop:validate` → passed,
79 items, 1 part + 1 parent.

> **Read this first.** A local working tree may sit on `main` @ `734aa8c` (YV83, 2026-08-10),
> which is **23 commits behind** `origin/main`. Every file:line below is against `4e8c9adf`.
> Fetch and branch from `origin/main`, never from a stale local `main`.

## 0. Scope

Wilson's six named defects (2026-09-12, verbatim) plus the whole queue that was left on the line
when the yap23 loop stopped mid-finisher. Mode: **build everything first, review in a separate
pass** — so every item's spec is written to be complete enough to ship right the first time.

Standing constraints that bind every item:

- **Never sandboxed.** `Entitlements.plist` pins `com.apple.security.app-sandbox` = `<false/>`.
  Sandboxing silently kills the CGEvent tap, synthesized ⌘V and `AXIsProcessTrusted`.
- **Never rename** the bundle id `com.wilsonguenther.wilson-voice` (resets every TCC grant) or the
  data dir `WilsonVoice` (orphans the SQLite history).
- **No Sentry, no PostHog, no Segment.** Local crash capture (`crash.rs`) is the observability stack.
- **Pixel-art companion** on an LCD/pod. No smooth vector, no origami, no angry eyebrows.
- **The gate** is the eight commands in `docs/loop/HARNESS.md`; `cargo fmt` is informational and
  exits 1 on unmodified main — never reformat to silence it.

---

## 1. Audit findings

### A. Audio permission — Wilson: "not requested or handled at all"

He is right, and the reason is that **Yap has never asked macOS the actual question.**

| # | Defect | Evidence |
| --- | --- | --- |
| A1 | **No TCC read exists anywhere.** `git grep "AVCaptureDevice\|authorizationStatus\|requestAccess" -- desktop` → **one** hit, a *comment* at `syscapture.rs:2117` about the CoreAudio process tap. For the microphone: zero calls. | grep, scoped to `desktop` |
| A2 | The "is the mic ready" answer is a **device probe**, which returns true when TCC is **denied**. | `mic_auth.rs:12-19` (`default_input_device().is_some() && default_input_config().is_ok()`), `permissions.rs:66-96` |
| A3 | That probe feeds the report, which feeds the onboarding checklist, which therefore renders **"Granted ✓" while permission is denied**. Then the take records silence, YV16's no-speech gate correctly refuses to paste, and the user sees a dead hotkey. | `permissions.rs:118` → `Onboarding.tsx:294-303` |
| A4 | **The hotkey never re-checks.** `start_recording` is documented as "the ONE gate ... every way to begin a new dictation funnels into" it — and at line 1135 it says, verbatim, *"Do NOT call mic_auth::request_microphone_access here"*. A grant revoked after onboarding is invisible forever. | `lib.rs:1100-1106`, `lib.rs:1135-1136` |
| A5 | Onboarding offers **"Continue anyway"** past a denied mic, into an app whose only feature cannot run. | `Onboarding.tsx:320` |
| A6 | **The pill has no permission state.** `git grep "permission\|denied" -- desktop/src/pill` → 0. | grep |
| A7 | **Ad-hoc signing** makes the grant genuinely evaporate on every local rebuild: macOS keys TCC to the code signature. `ROADMAP.md` already says so ("ad-hoc re-sign invalidates trust"). | `tauri.conf.json` `bundle.macOS.signingIdentity: "-"` |
| A8 | Input Monitoring (needed for the modifier-only fn/fn⌃ hold) and audio capture are never in the health report, though both deep links already exist. | `permissions.rs:196`, `:202` |

### B. Trial and limits — Wilson: "the pill does not tell people"

The **backend is good and the main window is good**. The pill — the only surface a user looks at
while working — knows nothing.

| # | Defect | Evidence |
| --- | --- | --- |
| B1 | **The pill never receives license status.** `git grep "license\|trial" -- desktop/src/pill desktop/src/float-main.tsx desktop/src-tauri/src/float_pill.rs` → **0 matches**. `float-main.tsx` is 53 lines and subscribes to nothing license-shaped. | grep |
| B2 | A refused press emits an event and a throttled notification, and **the pill does not move** — indistinguishable from a broken app. | `lib.rs:1111-1126` |
| B3 | The one trial warning fires **once at 3 days and never again** — correct for a modal toast, wrong as the only ambient signal. | `license/status.ts:80-88`, `TRIAL_WARN_DAYS = 3` |
| B4 | The **tray says nothing** about the trial, so a user who hides the pill (`show_floating_pill` is a setting) has no signal at all. | `lib.rs:406`, `:4468` |
| B5 | **No usage metering of any kind.** `git grep "quota\|usage_limit\|daily_limit\|words_limit\|minutes_used" -- desktop` → 0 functional matches. | grep |
| B6 | The trial state machine documents its own threat model carefully but has **no test file driving the fortnight or the attacks** (delete-the-file, delete-the-row, roll the clock back). | `license.rs:43-53`, `:361-363`, `:473-483` |

### C. Formatting — Wilson: "does not work whatsoever"

**Literally true, and it is two defaults, not a broken algorithm.**

| # | Defect | Evidence |
| --- | --- | --- |
| **C1** | **Shipped default `cleanup_level: "light"`; the formatting stage requires Medium+.** So on every fresh install there is **no list detection, no spoken punctuation, no email shape, no trailing-period rule**. Saying "new line" types the words "new line". | `lib.rs:417` + `dictation.rs:636` (`runs_format` → Medium\|High), `dictation.rs:607-616` (Light = dictionary + backtrack only) |
| **C2** | **Shipped default `polish_model: ""` = OFF**, and the LLM stage additionally requires High. | `lib.rs:418`, `lib.rs:264-267`, `dictation.rs:644` |
| **C3** | **And every formatting test passes**, because every fixture pins its own level. `group-a-sentence-shape.jsonl` line 4: `{"level":"medium", ...}`. The corpus proves a configuration the product never ships. | fixtures + `formatting_fixtures.rs` |
| C4 | **No polish model can be installed.** Onboarding is welcome → permissions → calibration → done; it downloads an ASR model and never mentions a polish model, so `polish_model` stays `""` forever. | `Onboarding.tsx:41` |
| C5 | **Long-form gets no polish at all**, by declared design: *"Long-form is rules-only: chunking is a later item."* | `polish.rs:71-73`, `MAX_POLISH_WORDS = 400` |
| C6 | **No paragraphing rule exists.** Only the spoken *command* "new paragraph". A five-minute dictation arrives as one wall of text. | `git grep paragraph -- dictation.rs` |
| C7 | Mode is detected and threaded; **nothing tests that it changes the output.** | `mode_for_app`, `lib.rs:415`, `focus.rs` |
| C8 | **No diff, no visibility.** `raw_text` is stored and `undo_ai_edit_text` exists; `git grep "DiffView\|diff" -- desktop/src` → nothing. | grep |

### D. Long dictations — Wilson: "no handling"

One-shot end to end, with an undeclared cliff.

| # | Defect | Evidence |
| --- | --- | --- |
| **D1** | **A 120-second hard wall on ONE decode, for the whole take.** Its own doc says it was sized for "a 60 s take" as a wedge detector. Trip it and the **entire take is lost** — there is no partial result. | `transcription.rs:51-54` |
| D2 | **No chunking on the dictation path**: the whole sample buffer, one call. | `lib.rs:1350` |
| D3 | **Two unbounded `Vec<f32>`** for the whole take (~345 MB for 15 min at 48 kHz stereo before the downsample). | `record.rs:2280-2284`, `:2368`, `:2377` |
| D4 | **No maximum session length** anywhere, so the failure at some unknown length is a timeout or an OOM rather than a message. Wispr declares 20 min. | `git grep MAX_TAKE\|max_take\|MAX_RECORD` → nothing |
| D5 | **No progress after the hold ends.** `busy` is a boolean; a 12-chunk decode shows one undifferentiated state for minutes. | `ClassicPill.tsx:23-24` |
| D6 | **No cancel once decode starts, and no cancel hotkey at all.** The binding table holds four bindings and none is cancel. | `shortcuts.rs:143` |
| D7 | **No crash recovery for a long dictation**, though the meeting path re-decodes abandoned chunks on relaunch and the journal + recovery dir exist. | `transcription.rs:70-84`, YV63 |
| D8 | The live pill tier is still driven by a **time-based word estimate**, not real words. | `live.ts:44-46` (`wordsFromVoiced`) |
| — | **Mitigating:** the chunker, seam dedupe, RT-safe ring and per-chunk budget **already exist for meetings**. Y3 is mostly a reuse job. | `meeting_asr.rs`, `transcription.rs:86-110`, `asr_engine.rs:382-394`, `rtring.rs` |

### E. UI/UX — Wilson: "looks broken, not smooth"

| # | Defect | Evidence |
| --- | --- | --- |
| **E1** | **Empty / loading / error states are essentially absent.** `git grep -c "empty-state\|EmptyState\|skeleton" -- desktop/src` → `App.css: 1`, `App.tsx: 1`. For **seven** views. A fresh install has no history, no meetings, no dictionary, no notes, no insights — every view a new user opens is in its least-designed state. | grep |
| E2 | **Insights renders a literal blank page** at zero takes: `nav === "insights" && insights &&` short-circuits to nothing under a heading. | `App.tsx:2953` |
| E3 | **`App.tsx` is 3,439 lines** holding 7 views and 8 settings sub-tabs; `App.css` is 2,789 lines with no token layer, which is mechanically why unrelated screens look like different apps. | `wc -l`, `App.tsx:55-61`, `:66-85` |
| E4 | **The pill's whole state vocabulary is four values.** Wispr's bar has twelve, with exact per-dock geometry. Wilson's own list of what is missing: transcribing, polishing, pasting, error, permission, model-loading, empty. | `live.ts:252`, `ClassicPill.tsx:23-24` |
| E5 | **The docked pill only moves; it does not reflow.** This is the architecture Wispr tried and abandoned (their own comment records the "30×6-always-horizontal collapse bug"). | YV53/65 vs the parity teardown §4.1-4.2 |
| E6 | **No hover hysteresis** — an edge-docked pill will oscillate expand/collapse on cursor dwell. Half the machinery (published hitbox) is already there. | `ClassicPill.tsx:38-42`, teardown §4.4 |
| E7 | **`String(e)` is the common error path**, not the rare one: nearly every Tauri command answers `Result<_, String>`, so a raw Rust string lands in front of the user. | `errors.ts:3-4`, `:27-30` |
| E8 | No motion system: durations and easings are ad-hoc; the measured spring constants from the teardown are unused. | `float.css`, teardown §4.3 |

### F. End-to-end wiring

| # | Defect | Evidence |
| --- | --- | --- |
| **F1** | **The updater points at a URL that cannot serve.** The DMG moved to Forge and then to Vercel because "the repo went private, GitHub release assets are no longer publicly downloadable" — and the manifest URL never moved. Its correctness now depends on repo visibility, which is deliberately toggled. | `tauri.conf.json` `plugins.updater.endpoints`; commits `2eabf33`, `734aa8c` |
| F2 | **Onboarding never proves the app works.** No paste ever happens; `onboarded` is set without evidence; the four things that can break a take each fail later, separately, with no context. | `Onboarding.tsx:41`, `lib.rs:420` |
| F3 | **Half-built scratchpad.** A DB table and CRUD exist and there is a `Nav` entry — no second window, no `⌥S`, no `note_versions`. | `db.rs:648`, `:2667-2721`, `App.tsx:60`, `tauri.conf.json` (one window) |
| F4 | `paste_last_transcript` exists with a global binding and **is not in the tray menu**; copy-last does not exist at all. | `lib.rs:2714`, `shortcuts.rs:86` |
| F5 | **`ROADMAP.md` is materially wrong** — "v0.4.1", "MLX Whisper", "Carbon ⌘⇧V", "Wilson Voice" — and the harness names the repo docs as a spec source, so a stale table is a wrong instruction that propagates. | `ROADMAP.md` head vs `lib.rs:404`, `:539`, YV34 |
| F6 | **Clippy and rustfmt are `|| true`** in CI, with the comment "Lints are informational until the tree is clean." A loop whose merge gate cannot see a lint failure will accumulate eighty items of them. | `.github/workflows/ci.yml` |
| F7 | **Nothing in the gate launches the app.** 128 Rust integration tests + 25 vitest tests, and Wilson's visual report was invisible to all of them. The headless hook (`--transcribe-file`) exists and is unused as a gate. | `cli.rs`, `lib.rs:3982` |
| F8 | **Six parked defects and six open PRs** from the stopped yap23 loop, all needing a rebase onto `min_embed`. The worst: **FAR 1.000 on the shipped scoring path** (tuned 0.034 on a different population). | `gh issue list` → #150-155; `origin/feat/yv127…yv134` |
| F9 | Eval numbers in the repo are **regression floors on a synthetic `say`-generated corpus** — the embedder hears the synthesizer. | yap23 close-out; `meeting_eval.rs` |

---

## 2. Item index

Lanes alternate by file (file 0 → A, file 1 → B, …). **Items inside a file run sequentially**, so
dependent work is grouped in one file in order; independent groups are in separate files.

Source key: **W** = Wilson 2026-09-12 · **A** = this audit · **P** = Wispr parity teardown
(`Notes/Wispr-Full-Parity-Research-2026-08-09.md` §3, item number given) · **B** = yap build-state /
open-bugs / pill-vision memory · **I** = GitHub issue.

| id | title | gated | source |
| --- | --- | --- | --- |
| **`00-y0-harness-and-gates.mjs`** — lane A | | | |
| `Y0-A` | clippy and rustfmt become blocking CI gates instead of `\|\| true` decoration | — | A (F6) |
| `Y0-B` | one command proves a fresh clone builds, tests and stages both sidecars | — | A |
| `Y0-C` | a test asserts the SHIPPED defaults, so an off-by-default feature can never be called tested | — | A (C1-C3) |
| **`05-y1-audio-permission.mjs`** — lane B | | | |
| `PERM-A` | real TCC: `AVCaptureDevice` authorizationStatus + requestAccess, replacing the device probe | — | W, A (A1-A2) |
| `PERM-B` | a denied mic gets its own screen with a working System Settings deep link | — | W, A (A3, A5) |
| `PERM-C` | every hotkey press re-checks the grant; the pill gets a `blocked` state | — | W, A (A4, A6) |
| `PERM-D` | first run refuses calibration without a grant; a silent take is diagnosed, not pasted as nothing | — | W, A (A3) |
| `SEC-A` | stable signing identity — the reason grants "reset" on every rebuild | — | A (A7) |
| `PERM-E` | one permission health surface, watched for revocation, all four grants | — | A (A8) |
| **`10-y2-trial-and-limits.mjs`** — lane A | | | |
| `Y2-A` | license status reaches the pill window — the wiring that does not exist | — | W, A (B1) |
| `Y2-B` | a quiet trial numeral on the pill in both styles, 30px side dock included | — | W, A (B3), P |
| `Y2-C` | a refused press produces a pill state that explains itself | — | W, A (B2) |
| `Y2-D` | one upgrade path from the pill, reusing the existing Payment Link | — | W |
| `Y2-E` | the menu bar carries the same truth, from one source | — | A (B4), P §2.2 |
| `DB-A` | usage metering and a "limit reached" surface | **panel** | W, A (B5), P §2.8 |
| `SEC-B` | the trial state machine gets the adversarial tests its own doc promises | — | A (B6) |
| **`15-y3-long-dictations.mjs`** — lane B | | | |
| `Y3-A` | bounded capture: the two unbounded Vecs spill to disk with a measured ceiling | — | W, A (D3) |
| `Y3-B` | chunked decode with seam dedupe, reusing the meeting chunker | — | W, A (D1-D2) |
| `Y3-C` | real progress in the pill, from real chunk words | — | W, A (D5, D8), B |
| `Y3-D` | cancel works mid-decode, and a cancel never loses the audio | — | A (D6), P §2.1 |
| `DB-B` | crash recovery for a long take — resume or offer it back | — | A (D7) |
| `Y3-F` | a declared maximum session length with a warning before it | — | A (D4), P §2.1 |
| `Y3-G` | a measured latency and energy budget for long takes | — | A, P §2.11 |
| **`20-y4-formatting.mjs`** — lane A | | | |
| `Y4-A` | **formatting is on for a fresh install** | — | W, A (C1, C3) |
| `SEC-C` | the polish model gets an install path, so the LLM stage can exist | — | A (C2, C4) |
| `Y4-C` | paragraphing: the rule that does not exist | — | W, A (C6) |
| `Y4-D` | lists and punctuation proven at the level the product ships | — | W, A (C3) |
| `Y4-E` | long-form gets polished: the 400-word cliff becomes a chunked pass | — | W, A (C5) |
| `Y4-F` | app-aware formatting that is actually applied, proven per app | — | A (C7) |
| `Y4-G` | the user can see what formatting did and undo it in one key | — | A (C8), P §2.6 #8 |
| `Y4-H` | formatting settings stop being engineer words, and every one persists | — | A, B (audit M6) |
| **`25-y5-ui-ux-polish.mjs`** — lane B | | | |
| `Y5-A` | a token layer so seven views stop inventing their own visual system | — | A (E3) |
| `Y5-B` | **every view gets a real empty, loading and error state** | — | W, A (E1-E2) |
| `Y5-C` | the full pill state machine, including the transcribe/think gap | — | W, B, A (E4) |
| `Y5-D` | soft-body pill motion from the measured spring constants | — | P §3 #2, B |
| `Y5-E` | hover hysteresis and alpha hit-testing | — | P §3 #3 |
| `Y5-F` | one error surface, one sentence per failure, one action | — | A (E7) |
| `Y5-G` | split the 3,439-line `App.tsx` into seven view modules | — | A (E3) |
| `Y5-H` | Home is a living habitat, not a static band | — | B (open-bugs) |
| `Y5-I` | **vertical dock as the CSS base** — the architecture Wispr abandoned trying the other way | — | P §3 #1, §4 |
| **`30-y6-end-to-end-wiring.mjs`** — lane A | | | |
| `UPD-A` | an updater endpoint that can actually serve a private repo | — | A (F1) |
| `Y6-A` | onboarding ends in a working dictation, or says exactly what is missing | — | A (F2), P §2.8 |
| `PRIV-A` | crash reporting stays local, complete and provably offline | — | P §5.2, A |
| `Y6-B` | the menu bar becomes a real surface | — | A (F4), P §2.2 |
| `Y6-C` | paste target and secure input, end to end | — | A, P §5.2 |
| `DB-C` | history, FTS search and export hold up at volume; Clear History still destroys the words | — | A |
| `Y6-D` | cold launch, sleep/wake, display change and a second instance all behave | — | A |
| `Y6-E` | README, ARCHITECTURE, ROADMAP and PRODUCT stop describing an app that no longer exists | — | A (F5) |
| **`35-y7-tests-and-smoke.mjs`** — lane B | | | |
| `Y7-A` | a headless smoke against the real built binary | — | A (F7) |
| `Y7-B` | a windowed smoke that walks all seven views and captures them at two sizes | — | W, A (E1, F7) |
| `Y7-C` | every test this loop adds is proven non-vacuous by a mutation | — | A |
| `Y7-D` | frontend coverage for the pure modules | — | A |
| `Y7-E` | the shipped DMG is smoke-tested the way a first-time user meets it | — | A (A7, F1) |
| **`40-y8-parity-p0.mjs`** — lane A | | | |
| `Y8-A` | the hotkey suite: hands-free, cancel, copy-last, paste-last, scratchpad | — | P §3 #6 |
| `DB-D` | scratchpad as a real second window, dictate-into-note, versions | — | P §3 #4, A (F3) |
| `Y8-B` | the pill becomes a bar: five affordance slots | — | P §3 #5 |
| `Y8-C` | optional earcons, off by default, ordered around YV28's auto-mute | — | P §3 #17 |
| `Y8-D` | coaching nudges in Yappy's voice, without becoming nagware | — | P §3 #20 |
| **`45-y9-parity-p1-intelligence.mjs`** — lane B | | | |
| `Y9-A` | a named transform library with an observable status enum | — | P §3 #7 |
| `Y9-B` | writing samples become a local style profile | — | P §3 #9 |
| `Y9-C` | spoken preference rules, with an explicit Apply step | — | P §3 #10 |
| `PERM-F` | deeper AX context: the selection and the text after the caret | — | P §3 #11 |
| `PERM-G` | IDE identifier bias — the highest personal-ROI item on the list | — | P §3 #12 |
| `Y9-D` | a denylist where the hotkey is inert | — | P §3 #14, #21 |
| `Y9-E` | CSV round-trip for dictionary and snippets, plus frequency ranking | — | P §3 #13 |
| **`50-y10-parity-p2-surface.mjs`** — lane A | | | |
| `Y10-A` | multi-language and the in-bar picker | — | P §3 #15 |
| `PERM-H` | microphone ranking and device intelligence | — | P §3 #16 |
| `Y10-B` | rich-text snippets without racing the receipt-sequenced paste | — | P §3 #18 |
| `Y10-C` | stacked messages, safely | — | P §3 #19 |
| `Y10-D` | a non-primary mouse button as push-to-talk | — | P §3 #22 |
| `Y10-E` | Insights v2 / the Yappy profile, computed in SQLite | — | P §3 #23 |
| `Y10-F` | measure and publish the idle cost number | — | P §2.11 |
| **`55-y11-diarization-carryforward.mjs`** — lane B | | | |
| `Y11-A` | rebase the six parked branches onto main so each is judged against `min_embed` | — | B, I #150-155 |
| `Y11-B` | the impossibility framing survives where the guard cannot see it | — | I #150 |
| `DB-E` | `speaker_profiles` stores a catalog id where it must store the weights digest | — | I #151 |
| `Y11-C` | **FAR 1.000** — bands tuned on one population, applied to another | — | I #152 |
| `Y11-D` | `split_partition` seeding fails when an outlier is the far point | — | I #153 |
| `Y11-E` | a false mechanism claim in a shipped asset, and a comment naming call sites that do not exist | — | I #154, #155 |
| `Y11-F` | a real-voice eval corpus — the numbers are only floors until it exists | **panel** | B, A (F9) |

---

## 3. Dependency order and lanes

**Prerequisite chain, strictly first:** `Y0-A` (a real lint gate) → `Y0-B` (a fresh-clone proof) →
`Y0-C` (a shipped-defaults test). `Y0-C` is what makes the formatting fix provable: it is the
tripwire that fails while `cleanup_level` is `"light"`.

Within-file order is the dependency order and the harness runs it sequentially. Across files:

- `10-y2` (pill license) depends on nothing in `05-y1`, but `Y2-C`'s state precedence must agree
  with `PERM-C`'s gate order (mic before license). Both say so.
- `20-y4` depends on `Y0-C`. `Y4-E` additionally wants `Y3-B`'s chunk geometry.
- `25-y5` collects the pill states `PERM-C`, `Y2-C` and `Y3-C` add, so it runs after them.
  `Y5-G` (the App.tsx split) is the enabling refactor for everything visual after it.
- `30-y6` depends on `PERM-E`, `Y5-C`, `Y5-F`, `Y3-B`, `DB-B`.
- `35-y7` depends on `Y5-B/C/G`, `SEC-A`, `UPD-A`; `Y7-C` collects the mutation rows every earlier
  item contributes and is sequenced last in its file.
- `40-y8` depends on `Y5-I` (orientation-neutral geometry) and `Y3-C/D` (the slots' data).
- `45-y9` depends on `SEC-C`, `PRIV-A`, `Y4-F`, `Y8-A`.
- `50-y10` depends on `PERM-A`, `Y6-C`, `Y8-A`, `PERM-E`, `DB-A`, `DB-C`, `Y4-G`, `Y5-H`, `Y3-G`.
- `55-y11` runs **last**. Nothing in Wilson's list depends on diarization; it is here because six
  blocking defects in a shipped subsystem must not become folklore.

If the loop is cut short, the merge order that buys the most is:
**`Y0-A..C` → `Y4-A` → `PERM-A..C` → `Y2-A..C` → `Y5-B` → `Y3-A..C` → `Y5-C`.**
That is Wilson's six complaints, in the cheapest order.

---

## 4. Open decisions for Wilson

### `DB-A` — usage metering and the "limit reached" surface (**panel**)

Wilson asked the pill to say "when they reach their limits". **Yap has no limits**, by a closed
decision: $29 lifetime, 14-day full-feature trial, no subscription v1 — and the marketing line is
*"no subscription tier gating your words."* The competitor's limit is 2,000 words/week on its free
desktop tier with a `WeeklyWordsLimitReached` notification, and the parity teardown lists "trial nag
surfaces" under *explicitly not wanted*. So the mechanism is buildable now and the policy is not:

1. Does Yap gain a metered free tier after the trial at all, or does the trial simply end?
2. If metered: the unit (words / minutes / takes), the window (day / week / rolling 7d), the number.
3. Does a limit stop **new dictation only**, exactly like the trial boundary?
4. Does the pill show consumption *before* the limit, or only on arrival?

The item ships the rollup, the query, the states, the copy and the tests with `LIMIT_ENABLED = false`
— dark, and one constant away from live. **Trial length is not a question: 14 days is fixed in code
(`license.rs:107`) and in TERMS.**

### `Y11-F` — a real-voice eval corpus (**panel**)

Every diarization number in the repo is a regression floor on a `say`-generated corpus; the embedder
is being asked to tell a synthesizer apart from itself. A real corpus needs Wilson's call on: whose
voices (licensed research corpus vs consented recordings), where it lives (it cannot be committed —
the existing pattern is an external drive plus a checksummed manifest), consent and retention for
what is biometric data, and whether CI ever sees it (almost certainly no). The item ships the
manifest schema, the eval arm that skips with a **named** reason, and a report that refuses to blend
a synthetic and a real number into one figure.

### Not gated, but worth Wilson's eye before the review pass

- `Y3-F` picks the **maximum session length** from what Y3-A/B measurably hold, and declares it.
  If Wilson wants a specific number, that overrides the measurement.
- `Y3-D` binds **cancel** to Escape while a take is live. If he wants a different key, say so before
  `Y8-A` builds the rest of the suite on that table.
- `Y10-C` (**stacked messages**) sends messages on the user's behalf. It ships off by default, chat
  modes only, with a hard send cap — but it is the one item in the queue that can do something
  irreversible in someone else's app.

---

## Panel revisions (2026-09-12)

Eight independent Opus seats (cto · macos-native-engineer · ai-models · senior-pm · ux-designer ·
the-user · pre-mortem · qa-test-engineer), run 3-concurrent under the standing agent cap, each
reading only this plan, `docs/loop/HARNESS.md`, the item files and the working tree. Seven returned
SOUND-WITH-CHANGES; qa-test-engineer returned RETHINK. **Items 79 → 87** (10 added, 2 killed).
Validator after the revisions:
`✓ loop:validate — 2 part(s) + 1 parent would be written, 87 item(s), every check passed.`

**Panel: audited 2026-09-12 — revise-first**

### Before launch — outside the item files, and blocking

1. **`GATE_CMDS` in `scripts/loop/template.mjs` stages only `yap-polish`.** `bundle.externalBin`
   names `binaries/yap-polish` AND `binaries/yap-diarize`, and `desktop/src-tauri/binaries/` holds
   only the polish one, so gate commands 7 and 8 (`cargo clippy`, `cargo test` in `src-tauri`)
   exit **101** in any fresh worktree — item 1 of 87, in both lanes. Two seats measured this
   independently. Amend the stage step to `cargo build -p yap-polish -p yap-diarize --release` +
   copy both (`desktop/package.json` already ships a correct `sidecar` script), and re-measure
   HARNESS.md's gate table, whose "0, ~4s warm / 0, ~41s warm" rows were never taken.
2. **Add `-- -D warnings` to the clippy line in `GATE_CMDS`.** `Y0-A` hardens
   `.github/workflows/ci.yml`, which is inert (Actions disabled account-wide, `ci=local`). Until
   the harness's own clippy is `-D warnings`, every item's `clippy=0` is evidence about a file the
   loop never runs.
3. **The harness never executes `acceptance`.** `item.acceptance` is interpolated into the builder
   prompt (`template.mjs:683`) and nothing spawns a shell with it, so "acceptance passed" is a
   model's self-report. Either move each block to `scripts/loop/acceptance/<ID>.sh` and have the
   harness run it the way it runs `GATE_CMDS`, or accept that the 800+ acceptance lines below are
   instructions, not a gate.
4. **Commit `docs/loop/PLAN.md` and `scripts/loop/items/`.** Both are untracked
   (`git ls-files docs/loop` → `HARNESS.md` only). Lane worktrees are checkouts of `origin/main`,
   so the whole spec corpus is invisible inside them, and a halted run's resume path depends on one
   untracked working tree.
5. **Land the rustfmt sweep as its own pre-launch mechanical commit** (with a
   `.git-blame-ignore-revs` entry), or leave fmt informational — which is what the ledger says and
   what the revised items now assume. Measured: 126 hunks across 19 files, top four `db.rs` 40,
   `dictation.rs` 38, `formatting_fixtures.rs` 14, `meeting_asr.rs` 10.
6. **Prove the amended gate green on unmodified `origin/main` in a throwaway worktree** before
   launching. HARNESS.md's claim that all eight exit 0 on main is currently untested and false.

### Resolved before launch — what was actually done, with the measurement (2026-09-12)

Every one of the six blockers above is closed. The evidence is a command and its exit code, not a
sentence.

1. **Both sidecars are built and staged by `GATE_CMDS`.** Reproduced the failure first, on this
   machine: with only `yap-polish` staged, `cargo clippy --all-targets --features custom-protocol`
   in `src-tauri` exits **101** with
   `resource path 'binaries/yap-diarize-aarch64-apple-darwin' doesn't exist`. It is worse than the
   panel said — the first probe of that same command had exited **0**, because with a WARM target
   dir `tauri-build`'s script does not re-run. A warm cache hides this defect completely, which is
   why the gate table is now measured in a throwaway worktree with a cold pinned target dir.
   `cargo build -p yap-diarize --release` exits 0 (it pulls a prebuilt onnxruntime on a cold
   build), so the gate builds both and copies both under their target triple.
2. **`-- -D warnings` is NOT added, deliberately.** Measured on unmodified main: clippy with
   `-D warnings` fails with ~30 lints. A mechanical `cargo clippy --fix --all-targets --features
   custom-protocol` sweep landed as `0f413a7` (13 files, no hand edits; `cargo test --features
   custom-protocol` = 0 afterwards, 546 lib tests + every integration suite). **16 lints survive
   it** and none is mechanical: `very complex type` (`syscapture.rs:3095`), `too many arguments
   (9/7)` (`lib.rs:735`), `clamp-like pattern` (`record.rs:2580`), `loop variable used to index`
   (`record.rs:916`), `doc list item without indentation`, two `assert!` with constant values and
   several `field assignment outside of initializer` in tests. Each needs a real refactor or an
   `#[allow]` with a reason, i.e. a code change with judgement in it — that is a loop item, not a
   pre-launch sweep, and forcing it now would have meant 16 unreviewed refactors under the gate
   that is supposed to police them. So the gate runs clippy **without** the flag and a clippy-zero
   item owns the residue.
3. **The harness now EXECUTES `preflight` and `acceptance`.** A Workflow script has no shell —
   `bash`, `exec` and `fs` do not exist in that runtime — so the execution is a dedicated
   command-runner **seat** plus control flow in the script: `runCommands()` in `template.mjs`
   returns a schema'd `{exits:[{command,exit}], allZero}`, the seat is forbidden to edit, commit,
   push, merge or comment, and `runPassed()` (the script, not an agent) decides. Per item:
   pre-flight is executed on detached unmodified `origin/main` **before any builder is dispatched**
   (all-zero ⇒ `already-done`, no builder, no branch, no PR), then the builder runs, then
   acceptance is executed on the item's branch; a non-zero buys exactly **one** fix round that is
   handed the raw acceptance output, then acceptance is re-executed by a fresh seat. Still failing
   ⇒ `status: 'failed-acceptance'`, the PR is left OPEN, labelled `needs-human` and carries the
   output, and the lane continues. `build.mjs` now fails the build if that path is missing — eleven
   structural needles plus a check that the runner seat is still told it may not edit or push.
4. **Committed.** `scripts/loop/items/`, `docs/loop/PLAN.md` and `docs/loop/DEFERRED.md` are in the
   launch commit, so every lane worktree (a checkout of `origin/main`) contains the spec corpus.
5. **The rustfmt sweep landed** as `b4b8d06` (`cargo fmt --all`, **152 hunks / 21 files** — more
   than the panel's 126/19 because the clippy sweep in (2) landed first), with
   `.git-blame-ignore-revs` in `9512ebe`. `cargo fmt --all -- --check` now exits **0**, so it IS a
   gate conjunct. The item preamble in `00-y0-harness-and-gates.mjs` that called fmt informational
   was corrected in the launch commit; `GATE_CMDS` remains the single source on any conflict.
6. **The amended gate was proven in a throwaway worktree** (`~/code/wilson-voice-loop/proof`, a
   cold pinned `CARGO_TARGET_DIR`, `npm ci` included) at `9512ebe`. The real exit codes and timings
   are HARNESS.md's gate table, which was rewritten from that run; the worktree and its target dir
   were removed afterwards.

### Applied — PLAN-CHANGE (22)

| # | Change | Converged |
| --- | --- | --- |
| 1 | 63 occurrences of `\${APP}` (escaped, six files 30-y6…55-y11) unescaped to `${APP}`. Rendered, 35 items handed the builder the literal `cd ${APP} && npm ci` — i.e. `cd` into `$HOME`. For Y6-C, DB-C and Y6-D that single line WAS the whole pre-flight. | senior-pm + qa (verified: `grep -cF '\${APP}'` = 63) |
| 2 | The shared preamble's "STANDARD GATE" no longer makes `cargo fmt --all -- --check` a conjunct. It exits 1 on unmodified main, so the gate every item was told to satisfy was unsatisfiable by any permitted action. HARNESS.md + `GATE_CMDS` named as the single source. | ai-models + qa |
| 3 | Four binding rules added to the preamble, applying to all 87 items: (a) a test-NAME grep must be paired with an executed mutation; (b) never anchor a gate to `desktop/src/App.tsx`; (c) pre-flights name a `--test` target; (d) nothing launches the app without `YAP_DATA_DIR` + `--smoke`; (e) an acceptance line green on main is not acceptance. | qa (CRITICAL) + 3 seats on (b) |
| 4 | **`Y5-G` runs first in `25-y5`** and its acceptance is fixed. It asserted exactly 7 `.tsx` in `views/` *and* `views/views.test.tsx` — eight files, so `test 7 -eq` could never pass. Test moved to `views/__tests__/`. Baseline corrected 3,439 → **4,660** (measured) everywhere it appears; a pure-move line-sum ceiling added. | cto + ux + qa (count) · cto + senior-pm + ux + the-user (order) |
| 5 | Ten UI gates re-scoped off `App.tsx` (`PERM-B`, `PERM-E` ×2, `Y2-D`, `Y4-G` ×2, `Y5-B` ×4). | as above |
| 6 | **`Y4-A` migrates unconditionally.** "Migrate only a value that is absent or was never explicitly set … if you cannot, do not migrate" is unimplementable — `save_settings` persists the whole struct under `#[serde(default)]`, and `apply_settings_migrations` returns early for any store at the current version (`lib.rs:2253-2256`). Measured: the reporter's own `settings.json` carries `"cleanupLevel": "light"`, `"schemaVersion": 1`. Now: bump to schema 2, rewrite light→medium, one quiet notice, and start recording provenance for the next time. | ai-models + the-user |
| 7 | `Y0-C`'s tripwire rewritten. Its premise ("no row carries `light`") is false — four rows do, one named `b17-level-light-runs-no-formatting`, and 36 carry `medium` — so it was green in both directions. Replaced with `every_rule_has_a_fixture_row_at_the_shipped_level`, enumerated from the rule table. | ai-models + the-user + qa |
| 8 | File order swapped: `10-y4-formatting.mjs` / `20-y2-trial-and-limits.mjs`. `Y4-A` — the plan's own "cheapest order" first move — was at lane-A position 11, behind seven trial-pill items; it is now position 4. Lane parity preserved. | senior-pm |
| 9 | `Y7-B` split. New **`Y0-E`** ships `scripts/smoke-windowed.sh` with the five structural failure conditions BEFORE the nine UI items, fails on today's tree (that red is the baseline), and carries `--self-test-must-fail`. `Y7-B` becomes the extension pass. `Y5-G`'s "screenshots are pixel-identical" gate — which named a golden-image check `Y7-B` forbids — now cites `Y0-E`. | senior-pm + ux + qa + pre-mortem |
| 10 | New **`Y0-D`**: `YAP_DATA_DIR` honoured at `lib.rs:551` and `models.rs:716`, plus a `--smoke` mode that registers no global hotkey, never pastes, and refuses to start against the default root. Two lanes plus Wilson's installed copy otherwise share one SQLite history, one settings store, one models dir and one global PTT binding — and `Y7-B`'s spec already assumed a flag that did not exist. | cto (CRITICAL, verified: no such override anywhere) |
| 11 | `Y0-A` no longer runs `cargo fmt --all` as merge #1 of the loop. | cto |
| 12 | 40 pre-flights converted from a bare `cargo test <filter>` to `--test <target>`. Measured: an unmatched filter exits **0**, so the "is this already done on main" proof contributed nothing for those items. | qa |
| 13 | `PERM-A` / `PERM-C`: the TCC request never blocks. `authorization_status()` is the only call `start_recording` may make; `requestAccess` fires and returns, the completion block drives the phase, `request_microphone` becomes `#[tauri::command(async)]`. Also `#[repr(isize)]` (NS_ENUM is NSInteger) and a main-bundle `NSMicrophoneUsageDescription` guard + `Info.dev.plist`, because TCC kills a bundleless process. | macos-native + pre-mortem |
| 14 | `PERM-C` now lands the COMPLETE `LivePhase` union in one commit. Six items across two unsynchronised lanes were each adding a variant to `desktop/src/pill/live.ts` from their own branch off main, with LAND told to skip conflicts — a guaranteed collision on the surface carrying defects (a) and (c). `Y5-C` consumes it. | senior-pm |
| 15 | New **`Y1-B`**: register the sleep/wake observer, and the false claim "power.rs already observes sleep/wake" deleted from `PERM-E` and `Y6-D`. `power.rs:63-135` is `IOPMAssertion` only; the repo already records the absent call site at `meeting_matrix.rs:398-408`. | macos-native + pre-mortem |
| 16 | New **`Y1-A`**: handle `kCGEventTapDisabledByTimeout` / `ByUserInput` and re-arm the tap. `CGEventTapEnable` is called exactly once, at startup, and `tap_callback` branches only on types 12 and 10 — so one slow callback kills push-to-talk for the process's life, indistinguishable from the permission bug Y1 is chasing. Zero hits for `TapDisabled` across all 79 original items. | macos-native + pre-mortem |
| 17 | New **`Y2-F`**: a stored key that grants nothing gets its own pill/tray tone, sourced from `license_problem_message`, with no purchase affordance reachable from it. `status.ts:213-217` already computes it; `Y2-A`'s policy enumerated only licensed / trial / license_required, so a revoked or seat-capped key was being shown the lapsed-trial chip and a price. | senior-pm + ux + the-user/pre-mortem |
| 18 | New **`LIC-A`**, first in `20-y2`: payment → issuance → activation → working dictation, proven once. Four new surfaces push users to pay and no item owned the leg between the charge and a key; the signer lives on the Forge box and `REVOCATION_URL` carries the server's IP in its hostname. | the-user + pre-mortem |
| 19 | New **`UPD-B`**: publish `latest.json` + `.app.tar.gz` + `.sig` to the host `UPD-A` repoints at, verify the live endpoint, keep the previous build as rollback. `docs/DEPLOY-SITE.md:40-46` stages HTML/CSS/fonts + one DMG; the only producer of the updater triple is the disabled Actions workflow. | macos-native + pre-mortem |
| 20 | New **`PRIV-B`**: erasure covers the audio and the partial words, not only the rows. `clear_history` is one line (`lib.rs:2386-2388`) while `Y3-A` spills takes to disk, `Y3-D` parks cancelled clips for 7 days and `DB-B` persists chunk text — a privacy regression this loop creates. Retention also becomes visible in History. | pre-mortem (CRITICAL, grounded) |
| 21 | New **`Y4-I`**, before `SEC-C` and `Y4-E`: measure the polish sidecar against the real weights. Measured on an M4 Pro at an unlimited deadline — 1.5B ok to ~150 words, `err=max_out` (whole rewrite discarded) at 200 and 400; 0.5B errors at every length from 100 words up; cold ready 7,323 ms against a 10 s budget. All 11 existing `yap-polish` tests are prompt/protocol level and none loads a model. | ai-models (measured) |
| 22 | New **`Y5-J`**: the accessibility floor — contrast bar on the token layer, a global `:focus-visible`, an accessible name per pill phase, one `aria-live`, clickable divs to buttons. 87 items added ~13 pill phases and ~21 view states with zero a11y requirements, and `Y5-A` freezes the look at the one cheap moment to fix contrast. | ux-designer |

### Applied — ITEM-CHANGE (20)

`Y0-A` title + "ci.yml is not the loop's gate" note · `Y0-B` asserts BOTH staged sidecars ·
`Y0-C` verifies its own mutation restore with `git diff --exit-code` · `Y4-A` acceptance drops two
lines that were green on main and adds a migration proof · `Y4-D` drops a medium-row count (already
36) for per-rule coverage · `Y4-E` sized from `Y4-I`'s curve, deadline ceiling raised above the
measured per-chunk cost, a `max_out` chunk keeps its rules text, one real-sidecar test ·
`Y4-G` records which stages ran and why the LLM stage produced nothing, and the phantom
"post-take surface" is deleted (there is none — the pill is a non-activating NSPanel) ·
`SEC-A` fails loudly with no `APPLE_SIGNING_IDENTITY`, signs for real in acceptance, two profiles ·
`SEC-B` gains the forward-clock excursion (the floor is not clamped the way the start is) and a
signed grace claim · `SEC-C` drops the already-done catalog work for the real gap (no Tauri command,
no frontend reference), adds a free-space precondition, and ships the 1.5B only · `PERM-E`
tri-state per grant with `IOHIDCheckAccess` named, audio capture Unknown by construction, and
`ffmpeg_ok` deleted · `Y3-B` re-scoped against a measured 601 s decode that finished in 30.5 s
(the 120 s wall sits near 45 minutes, not four) · `Y3-G` writes to a TRACKED `docs/BUDGETS.md` and
gains a resident-footprint ceiling · `Y5-B` enumerates each view's real state set instead of 21
markers · `Y5-I` scopes the Wispr geometry table to `ClassicPill` · `Y6-C` **decided**: a take whose
target moved is HELD AND OFFERED, never dropped · `Y6-D` splits its seven promises into
state-machine / observed / manual-checklist, because a `cargo test` cannot sleep a Mac · `Y6-E`
declares the floor machine · `Y7-C` collects executed mutations, one row per new test file ·
`Y7-E` runs its checklist against a real DMG instead of `bash -n`.

### DEFER (7) — see `docs/loop/DEFERRED.md`

### REJECTED (7)

1. **Make rustfmt blocking / reformat the tree** — relitigates the decision ledger ("`cargo fmt` is
   informational … never reformat to silence it").
2. **Add a ninth gate command (`cargo test -p yap-diarize --release`)** — relitigates the
   ledger-fixed eight-command gate. Staging both binaries is a *precondition* fix and is accepted;
   growing the gate is not.
3. **Move `40-y8`…`55-y11` (26 items) into a separate run** — violates the standing rule that one
   loop carries all prompts. The priority problem is fixed by ordering (change 8), not by cutting.
4. **"The blocking TCC prompt lands on the CGEvent tap thread"** (pre-mortem) — wrong on the
   mechanism: `lib.rs:4731` hops through `run_on_main_thread`, so the tap callback returns and the
   AppKit main thread is what blocks. macos-native-engineer was right; the remedy (never block) is
   identical and was applied.
5. **Kill `Y4-E`** (ai-models) — solo; the measurement is right and became change 21 + an item
   change, which is the cheaper fix than losing long-form polish.
6. **Kill `Y6-D`** (macos-native) — solo; the honest-scope split preserves the coverage.
7. **Kill `Y7-C`** (qa) — solo; the per-item mutation rule moved into the preamble, which is what
   the seat actually wanted.

### For Wilson (new, beyond `DB-A` and `Y11-F` which stay panel-gated)

1. **`LIC-A` — issuance policy.** Automated (Stripe → issuer → email → in-app retrieval), or manual
   with a stated turnaround plus a signed grace claim so nobody who paid sits in a dead app? Money.
2. **The support floor.** `minimumSystemVersion: "12.0"` invites 8 GB Intel Macs and no
   `x86_64`/universal target exists anywhere; the only staged sidecar is `aarch64`. Raise the floor
   to the arm64 reality (a public support promise), or fund a universal build.
3. **Which pill style ships in v1** — `ClassicPill` (today's default, `lib.rs:407`) or `YappyPill`.
   Keeping both doubles the render and test surface of every pill item and leaves `Y5-I`'s geometry
   table pointed at an unstated target.

---

## Owner decisions applied (2026-09-13)

Wilson answered the panel's three owner questions. Two are closed, one stays open and
non-blocking. Items **87 → 88**. Validator after these changes:
`✓ loop:validate — 2 part(s) + 1 parent would be written, 88 item(s), every check passed.`

### 1. The pill is a CHARACTER SYSTEM, not a choice — `Y5-H` reinstated

Wilson, verbatim: *"I thought we were gonna develop it and then make more characters and make it
more flexible ... there's a classic pill and there's a yappy pill and there's gonna be different
pills with the different creatures that are coming."*

"Which style ships in v1" (panel, For Wilson #3) was the wrong question and is closed: **both ship**,
as the first two characters of a pluggable system, and more creatures follow. The living habitat
(`Y5-H`), killed by the panel on a cto + ux-designer convergence, is **REINSTATED** as the habitat
layer of that system.

The panel's cost objection was right and is answered **structurally**, not by picking one. The
doubled surface it measured (*"Y5-C alone is 13 phases × 2 styles × 3 dock positions"*) collapses
because the multiplication is removed, not because a factor is deleted:

- **The shell** owns everything that is not the creature — window, dock, geometry table,
  hover/hit-testing, motion, the phase state machine, a11y names. **Dock positions are handled once,
  in the shell**, never per character.
- **A character is a data-driven module** behind one interface: `(phase, tone, level, box, dock,
  reducedMotion) → sprite/animation + copy`. It knows nothing about docks, windows, settings or
  license state.
- **Tests run a fixture matrix over the registered characters** instead of duplicated code paths:
  13 phases × 3 docks once in the shell, plus one data-completeness sweep per registered character.
- **A new creature is a new module plus a fixture row, with no shell change** — and that is proven,
  not asserted: `Y5-K` ships a deliberately minimal third character whose only job is to be that
  proof.

The cute/Tamagotchi rule is unchanged and binding: pixel art, chunky pixels,
`imageSmoothingEnabled = false`, limited retro palette, hand-coded. Origami stays rejected.

**Items changed / added**

| Item | Change |
| --- | --- |
| `25-y5` file header | New OWNER DECISION block stating the shell/character seam and the matrix rule; binding on every item in the file. |
| `Y5-C` | "a rendering in BOTH pills at ALL THREE dock positions" → the **shell** places the phase at all three docks once; each character owes **phase coverage as data**. New table test `every_shipped_character_has_copy_and_art_for_every_phase`, added to acceptance. No third branch on `pill_style` outside the two components until `Y5-K` lifts the data out. |
| `Y5-I` | The panel's "scope the geometry table to ClassicPill" is **superseded**. The Wispr geometry table is the **pill SHELL's**, character-independent: `dock-geometry.json` is keyed phase × dock with **no style dimension**, and a character renders inside the box it is handed. New acceptance line asserts the fixture carries no style key. |
| **`Y5-K` (new)** | *Pill character system + habitat layer.* Last in `25-y5` by dependency: shell (`Y5-A`, `Y5-C`, `Y5-D`, `Y5-E`, `Y5-I`) → characters → habitat. Ships `characters/{types,registry}.ts`, ports `ClassicPill`/`YappyPill` into `characters/classic|yappy` as data, adds `characters/example` as the no-shell-change proof, and rebuilds `home/YappyHouse.tsx` (919 lines, kept — refactor plus a layer, never a rewrite) into `home/habitat/` with its director, scene and event-driven reactions. |

`Y5-K`'s pre-flight fails on main today (`desktop/src/pill/characters/` and `desktop/src/home/habitat/`
do not exist), and its acceptance carries the binding-rule (a) mutation: registering an incomplete
character must turn the contract test red, then `git checkout` + `git diff --exit-code`.

### 2. `LIC-A` — issuance moves to Supabase (ungated, decision made)

Wilson: *"they buy it from Stripe — where do they get it?"*, and his lean, *"connect this to
Supabase"*. The panel's two options (automated vs manual) were both wrong about the starting state.

**How licensing works today.** Checkout is a Stripe **Payment Link**, a single compile-time constant
(`desktop/src-tauri/src/license.rs:137`) opened by `open_purchase_page` with no argument
(`desktop/src-tauri/src/lib.rs:3557-3570`); the link is deliberately `active: false` on Stripe until
delivery is proven (`license.rs:131-136`). Fulfilment is already **automated, but outside this
repo**: a Fastify service on the Forge box registers `POST /v1/yap/stripe-webhook`,
`GET /v1/yap/license`, `GET /v1/yap/revoked.json` and `POST /v1/yap/resend`
(`drivia-forge server/src/routes/yap.ts:93,402,479,493`, wired at `server/src/index.ts:406`), signs
Ed25519 claims in `server/src/yap-license.ts` (`signClaims`) with a key at
`/etc/forge/yap/license-signing-ed25519.pem` root:root 0400 (`yap-license.ts:43,179`), mails the key
through Resend (`yap-license.ts:623-690`, where a mail failure deliberately never becomes a non-2xx
for Stripe), and is idempotent on both Stripe `event.id` and the checkout session id
(`yap-license.ts:580-583`). The app verifies **offline** against the pinned public key
(`license.rs ISSUER_PUBLIC_KEY_SPKI_B64`, `ISSUER_SKID`) and makes exactly one network call, for the
public revocation list at `license.rs:117-119` — a URL carrying the Forge box's **IP address** in its
hostname. So the real defects are not "is it automated": the fulfilment path is invisible to this
repo and untestable in this gate, revocation is pinned to a box IP, and nobody has ever walked the
purchase end to end.

`LIC-A` is therefore a **migration with a proof**: Stripe Checkout → a Stripe webhook handled by a
**Supabase Edge Function** on a **dedicated Yap Supabase project** (explicitly *not* the Drivia
project `vlfrzdbqwsnrosmcygca`, which is over its free-tier limits — a licensing outage caused by an
unrelated product's usage is the worst possible coupling) → the key signed **server-side** with the
signing key moved into Supabase secrets, the public half still compiled into the app for offline
validation → delivered by **Resend** *and* retrievable in-app by purchase email → the **revocation
list served from Supabase**, so `sslip.io` disappears from `license.rs` and the box can move. The
claims wire format is **frozen** (`base64url(claims).base64url(sig)`, signature over the ASCII bytes
of the first segment) because every shipped copy of Yap pins the verifier.

The Supabase project ref and function URL, the Stripe webhook signing secret, `RESEND_API_KEY` and
`YAP_SIGNING_KEY_PEM` are **runtime dependencies Wilson provisions** — none may be committed. The
item builds and tests against `supabase start` or injected stubs so **the gate stays offline**: the
local-stack walk is a documented manual step, never a gate conjunct. Provisioning is documented in a
new `docs/YAP-LICENSING.md` with a **Runtime Dependencies** table (the convention `Y6-E` puts in
`ARCHITECTURE.md`) plus an `ISSUANCE` section in `docs/RELEASE.md`.

`LIC-A` stays **ungated** — it already was (`gated: null`), and nothing in the code makes that
unsafe: the signing key never enters the repo, the verifier is unchanged, and the Forge issuer is
explicitly **not** decommissioned by this item (two issuers sharing one key is fine; one dead
customer path is not).

One harness consequence: `scripts/loop/build.mjs`'s wrong-stack drift rule banned the string
`supabase` as "a web-stack surface Yap does not have". That is no longer true — `supabase/functions/**`
is now a real surface of this repo — so the rule was narrowed to `next.config|app/dashboard`, with
the reason written at the rule.

### 3. The support floor — STILL OPEN, non-blocking

arm64-only vs a universal build is **not answered**. `Y6-E` is unchanged and still declares the floor
machine in `ARCHITECTURE.md`; the universal-build question stays in `docs/loop/DEFERRED.md` §7 as a
release-engineering decision. Nothing in the loop blocks on it.

### 4. Gated, unchanged

`DB-A` (usage metering) and `Y11-F` (real-voice eval corpus) stay `gated: 'panel'`. Wilson confirmed.
They cost nothing and are skipped unless launched with `args: {panelApproved: [...]}`.
