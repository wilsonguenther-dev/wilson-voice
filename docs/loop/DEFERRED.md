# Deferred — real, later, with a destination

Created by the Senior Panel, 2026-09-12, alongside `docs/loop/PLAN.md` §Panel revisions. Nothing here
is rejected; each entry is work the panel judged real and deliberately out of the loop (87 items at the panel; 88 after the owner decisions of 2026-09-13).
A design note is kept for everything that was killed, so a later loop starts from the thinking
rather than from scratch.

## 1. `Y5-H` — Home is a living habitat (**REINSTATED 2026-09-13 as `Y5-K`**)

**Why it left.** Two seats independently named it as the one item to cut. It closes none of
Wilson's six observations, it is a rewrite of a 919-line working canvas scene
(`desktop/src/home/YappyHouse.tsx` — real-clock scene with an ambient director), it depends on the
three riskiest UI items (`Y5-A`, `Y5-D`, `Y5-G`), and its own spec names Wilson's taste as the gate
("if it is not adorable, it is wrong") — unautomatable in a build-first pass that dispatches no
reviewer. Its acceptance was four test-name greps plus `imageSmoothingEnabled`, all of which pass
with art worse than what ships today. Home is also the one surface not named in any of the six.

**Design note, preserved.** Routines on a real clock; intent pathing rather than random walk;
event-driven reactions to app state (a take starting, a paste landing, a model finishing its
download); dithered depth and parallax for the pod interior; pixel art, never smooth vector, never
origami (decision ledger #4). The cute companion still ships in this loop through `Y5-C`, `Y5-D`
and `Y5-I` — the pill is what a working user actually looks at.

**REINSTATED — OWNER DECISION 2026-09-13.** Wilson overruled the kill on product-identity grounds:
the pill is a **character system**, not a choice between two styles, and the habitat is that
system's habitat layer. *"There's a classic pill and there's a yappy pill and there's gonna be
different pills with the different creatures that are coming."* The work now lives in **`Y5-K` —
"pill character system + habitat layer"**, last in `scripts/loop/items/25-y5-ui-ux-polish.mjs` by
dependency (shell → characters → habitat), inside this loop. The design note above is its spec
input and survives intact, including the aesthetic lock.

The seats' cost objection was **not** overruled — it was answered structurally: the shell owns
docks, geometry and the phase machine; a character is a data module behind one interface; tests run
a fixture matrix over registered characters. See `docs/loop/PLAN.md` § "Owner decisions applied
(2026-09-13)" #1.

**What is still deferred from this entry.** Nothing — the item is in the loop. Wilson still judges
the ART on the PR's screenshots and recording; `Y5-K`'s acceptance deliberately gates the
STRUCTURE, because taste is not automatable in a build-first pass.

## 2. `Y10-C` — stacked messages / press-enter (KILLED from the loop, design kept)

**Why it left.** Three seats independently named it, from three different rationales (blast radius,
a 14-day evaluator's trust, a 2027 postmortem). It is the only item in the queue that takes an
irreversible action inside someone else's application, the plan concedes so in §4, it is pure parity
(P §3 #19) with no Wilson request behind it, and the same parity teardown lists auto/nag surfaces
under *explicitly not wanted*. It also competes for the synthesized-keystroke and
receipt-sequenced paste machinery that `Y6-C` and `Y10-B` are hardening — adding race surface to
the one path day 1 depends on. Off-by-default plus a send cap changes the frequency, not the shape.

**Design note, preserved.** Chat-mode detection from the focus context; a hard send cap per take;
an explicit dictation-to-send state machine with a visible armed state and a cancel that beats the
send; never a default.

**Destination.** Reconsider only after a windowed smoke and a review pass actually gate UI
behaviour — i.e. after `Y0-E` and the review pass have shipped.

## 3. In-the-moment formatting diff (out of `Y4-G`)

`Y4-G` said "a see-what-changed panel in History and in the post-take surface". There is no
post-take surface: after `done` the pill is a small always-on-top NSPanel with no text region
(`float_pill.rs:343-371`) and no window appears. `Y4-G` is now scoped to History plus the existing
⌃⌘Z undo. A real in-the-moment panel needs geometry per dock, dismiss rules, a story for not
stealing focus from a non-activating panel, and behaviour when the user keeps typing.
**Destination.** Its own item in the next loop, specced with that geometry, after `Y5-I`'s dock
work lands.

## 4. Fix-the-last-take — the correction path

Correcting a misrecognition today means leaving the app you dictated into, opening Yap's main
window, finding the row, clicking "Fix transcription" (`App.tsx:2637`) and retyping — and
`correct_transcript` (`lib.rs:2481`) only rewrites the stored row, never the words where they
landed. No binding exists for it (`shortcuts.rs:143-148` is paste-last, undo-AI-edit, legacy
toggle, meeting toggle). Highest-frequency UX failure in a daily-driver dictation tool, raised by
one seat with full evidence.
**Design.** One hotkey reopens the last take in a small correction surface, caret in the text;
"Replace" re-pastes through the receipt-sequenced path with the same-frontmost-app guard, falling
back to clipboard when the target moved; the edit still routes through `record_correction` so the
dictionary keeps learning.
**Destination.** Next loop, beside `Y8-A`, which already rewrites the binding table.

## 5. Bring-your-vocabulary import for a Wispr switcher

The day-1 audience's switching cost is the custom vocabulary they accumulated elsewhere; on Yap
day 1 those names come out wrong and read as worse accuracy. The only bulk-vocabulary item in the
loop, `Y9-E`, is a round-trip of Yap's own data at roughly lane-B position 31, and nothing anywhere
mentions importing from another application.
**Design.** Paste-a-list / drop-a-CSV into the dictionary, deduped against existing, reachable from
the dictionary empty state `Y5-B` builds and offered once at the end of `Y6-A`'s try-it step.
Acceptance: 60 fixture terms import, dedupe, survive relaunch, and one transcription fixture proves
an imported term now spells correctly.
**Destination.** Next loop, in the `y4`/`y9` group; `Y9-E` keeps the round-trip and the ranking.

## 6. The 0.5B "fast tier" polish model

`catalog.json` lists `qwen2.5-0.5b-instruct-q4_k_m` at `recommended_rank 2`, described as ~3x
quicker. Measured on an M4 Pro with an unlimited deadline it returns `err=max_out` at 100, 150,
200, 300 and 400 words — i.e. worse than no model, and a `max_out` for `KIND_POLISH` discards the
whole rewrite. `SEC-C` now ships the 1.5B only.
**Destination.** Revisit when `Y4-I`'s measured curve, or a different quantisation, says the small
model can answer a paragraph. Do not offer a tier that silently declines to run.

## 7. A universal / `x86_64` build

`minimumSystemVersion: "12.0"` promises Intel Macs nothing in the build matrix targets
(`git grep -E 'x86_64-apple|universal-apple'` over `.github`, `desktop/package.json` and
`tauri.conf.json` → no matches; the only staged sidecar is `aarch64-apple-darwin`). Building
universal is a release-engineering project — vendored ggml, sherpa-onnx, two sidecars, notarization
for both slices.
**Destination.** A release-engineering decision, not a loop item. `Y6-E` declares the floor machine
in ARCHITECTURE.md; Wilson's call (PLAN.md §Panel revisions, For Wilson #2) decides whether the
floor moves up or the matrix grows.

## 8. Harness work the panel could not apply (writes were scoped to the item files)

- `GATE_CMDS` must stage **both** sidecars and carry `-- -D warnings` on clippy.
- The harness must **execute** each item's `acceptance` and read its exit code, rather than
  interpolating it into a prompt.
- `docs/loop/PLAN.md` and `scripts/loop/items/` must be committed before launch.

**Destination.** `scripts/loop/template.mjs` + `docs/loop/HARNESS.md`, before the run starts. These
are listed as blocking in PLAN.md §Panel revisions, "Before launch".

## Y4-G — the in-the-moment diff panel, and the pill's skip line

**Deferred:** a "see what changed" surface shown right after a take, and the
one-line silent-skip notice on the pill itself.

**Why.** After `done` the pill is a small non-activating `NSPanel` with no text
region (`float_pill.rs:343-371`) and no window appears. An in-the-moment panel
therefore needs real geometry that does not exist yet: size per dock position,
dismiss rules, how it avoids stealing focus from a non-activating panel, and
what happens if the user keeps typing while it is up. Inventing that inside a
build item produces a window nobody specified.

**What shipped instead (Y4-G).** The diff, the stage attribution and the
silent-skip reason all land in History, on the existing `⌃⌘Z` undo, and — for
the skip — in the local log as one counter per reason. The Rust side already
emits `POLISH_SKIPPED_EVENT` (`polish_skipped`, payload = the closed-set reason
tag) on every take whose LLM stage was enabled and produced nothing, so the
pill's line is one `listen()` away once the panel geometry is decided.

## 9. `yap24-NT8` — calendar-aware "Record?" prompt (DEFERRED from pass 1, 2026-09-26)

**Why it left.** 2 of 5 Senior Panel seats (CTO, Senior PM) independently named this the single
item to kill from the yap24-NT pass. It adds a NEW permanent TCC permission (Calendars), a new
EventKit binding, a 5-minute background poll plus a change observer, and a pill interruption —
none of it moves stop → transcript → notes, which is the entire mandate of pass 1 ("the notetaker
is not even working"). Its own spec already says "the panel and Wilson must bless it" (a NEW
permission grant), which by itself blocks an unattended pass-1 launch, and its acceptance can only
prove a synthetic event-policy table, never a real Calendar interaction, headlessly. It DEPENDS on
`yap24-NT6, yap24-NT7`, so deferring it costs nothing else in the chain.

**Design note, preserved.** Settings → Meetings toggle (default OFF); a pure policy module
(`calendar_prompt.rs`) over synthetic EventKit events — attendees or a video link, or a
user-tagged "classes" calendar, starting within 2 min → one prompt, snooze 2 min, never during an
active dictation/meeting, never twice for one event; the pill shows Record / Snooze / Not this
one; Record starts the meeting with the event's title and infers `kind: call` vs `in_person` from
whether the event has a video link. EventKit reads stay a ≤5-minute poll plus
`EKEventStoreChanged`, never a tight poll, and nothing leaves the Mac.

**Destination.** Reconsider after `yap24-NT9` has passed on one real meeting on Wilson's own Mac,
and only after his explicit yes to the new Calendars permission (a product-identity-adjacent call,
same bucket as pricing/trial length). The item stays in
`scripts/loop/items/01-yap24-notetaker.mjs` (`yap24-NT8`), gated `panel`, simply left out of the
pass-1 `panelApproved` array — see `docs/loop/PLAN.md` / `Loop-Logs/PANEL-yap24-2026-09-26.md` for
the exact list.

## 10. `yap24-NT5` — diarization / speaker enrollment (DEFERRED from pass 1, 2026-09-26)

**Why it left.** 3 of 5 Senior Panel seats (Senior macOS/Rust engineer, Senior AI/Models, THE USER)
independently named this the single item to kill from the yap24-NT pass. Its own `DEPENDS` line
says "Y11-A..Y11-E ... must be fixed first or this item ships known-wrong labels," but `DEPENDS` is
prose the harness never enforces (no `DEPENDS` handling exists anywhere in `template.mjs` or
`build.mjs`), and pass 1 runs `only:['yap24-NT']`, which hard-skips every `Y11-*` id. Left approved,
NT5 would ship speaker enrollment on a path independently measured at FAR 1.000 (Y11-C, the
pinned-digest skip) with DER 0.34–0.45 on clean synthetic voices, and Y11-F (the real-voice
corpus) is owner-gated and unrun. Meeting notes that confidently attribute someone else's words to
Wilson are worse than notes with no speaker labels at all. `yap24-NT6`/`yap24-NT7` already render
labels only when present, so leaving NT5 out costs nothing else in the chain, and `yap24-NT9`'s
`DEPENDS` has been corrected (by this panel) to `NT1-NT4, NT6, NT7` so the phase-closing proof does
not wait on it either.

**Design note, preserved.** After transcription completes, hand the mic track to the shipped
`diarize::pool()` sidecar (yap-diarize, sherpa-onnx CAM++ 192-dim) for `kind: in_person`
(and `unknown`) meetings; attribute clusters to segments via the existing
`attribute_clusters` / `rank_and_floor` path; call kind keeps track labels (You/Them) untouched; a
new-or-unknown voice raises the shipped "who is this?" prompt, one question per CLUSTER, never per
segment, stored in `speaker_profiles`; copy states the accuracy honestly ("Speaker labels are a
best guess — tap to fix"); a diarize failure never fails the meeting — transcript and notes stand,
labels are absent, one sentence says why.

**Destination.** Re-approve only after Y11-A..Y11-E merge AND Y11-F's real-voice DER/FAR is
measured against a stated threshold, in a pass scoped `only:['Y11-', 'yap24-NT5']`. The item stays
in `scripts/loop/items/01-yap24-notetaker.mjs` (`yap24-NT5`), gated `panel`, left out of the
pass-1 `panelApproved` array.

## 11. `yap24-OS2`, `yap24-OS4`, `yap24-OS5`, `yap24-X2`, `yap24-X3` — expansions, not fixes
(advisory-only from the 2026-09-26 panel; not auto-included in any yap24 pass by prefix)

The Senior PM seat named `OS2` (SMAppService launch-at-login), `OS4` (entitlement diet, amended
above with a real signing bed), `OS5` (idle-wakeup consolidation, amended above to keep the Secure
Input poll), and `65-yap24-expansions.mjs`'s `X2`/`X3` (local MCP server, "ask your meeting") as
expansions that serve none of the three day-1 stories (meeting notes, the day-8 trial, blind
hotkeys) and should not compete for lane time with `yap24-NT`/`yap24-PILL`/`yap24-OS1`/`OS3` in an
early pass. They are unchanged in the item files (still gated `panel`, still buildable on their
own later pass) — this entry just records the sequencing recommendation: build NT1-4, NT6, NT7,
NT9, NT10, PILL1-4, OS1, OS3, UI1-3, PKG1-2 and the retargeted LIC-A first; revisit OS2/OS4/OS5/
X2/X3 after.
