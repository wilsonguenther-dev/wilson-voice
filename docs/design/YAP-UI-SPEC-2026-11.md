# Yap UI/UX spec for the 2026-11 redesign

*Written 2026-11-06T19:33Z (date -u) by loop two, item `yap-1106-ui-spec` (backlog §13). This is a document only: no code
changed with it. It is the gate for the UI items: §14 Settings, §15 Meetings, §16 Insights, §17 menu bar, §18 pill notes
and meeting, §19 Yappy states and motion. Each of those items preflights on this file existing on main.*

Sources, in order of authority:

1. Wilson's decisions as recorded in the loop context store (`ctx.mjs get decisions`), 2026-11-06. Marked **DECIDED**.
2. Backlog `docs/loop/BACKLOG-2026-11-06.md` Part 2 (§13 to §19) and §2, §22, §23. Their defaults are marked **DEFAULT**:
   the backlog proposed them and nobody has overruled them, but Wilson has not signed each one either.
3. RECON 2026-11-06 §D, the surface map (vault `Projects/Yap/RECON-2026-11-06.md`), and the design review
   (vault `Projects/Yap/DESIGN-2026-11-06.md` plus its pages in `design-2026-11-06/`). Ideas from those pages that are not
   yet decided are marked **PROPOSED**.
4. Today's code on origin/main (bfe998d), cited as `file:line` where it matters. Marked **TODAY**.

When this spec and a mockup disagree, the mockup wins once Wilson drops it into its slot (section 6). When this spec and a
DECIDED line disagree, the decision wins and this file is wrong; fix the file.

---

## 0. Standing rules every UI item follows

- **Look.** Run the `unslop` skill on every screen. Design tokens live in `desktop/src/tokens.css` and nowhere else; no
  hex values in component CSS. No gradients-as-decoration, glows, emoji labels, uppercase tracked labels or pulsing
  chrome. Pixel art (Yappy, friends, rooms) renders at whole-number scale only and stays crisp at device scale 1 and 2.
- **Never name an AI vendor or model on any user-visible surface** (DECIDED, design-page-10). Copy says "Cloud" or
  "On this Mac", "speech engine", "notes engine". Anything that guesses (a speaker name, a role) is shown as a suggestion
  with its reason ("Sounds like Dana, 82%").
- **Full lifecycle.** Every object (take, meeting, note, snippet, dictionary word, voiceprint, scratchpad page) has
  create, find, open, edit, save, close, delete and restore. Delete is undoable for 10 s through a toast with Undo before
  it becomes permanent. Every screen has a way in and a way out (section 2).
- **Every screen has the five states** in section 3: empty, loading, error, permission, offline. Plus keyboard use and
  reduced motion.
- **Contained layout** (DECIDED, design-page-7). The main window has a fixed sidebar rail, a header, one scrolling main
  column, and a pinned footer. Lists end; they never scroll forever. Long lists end with a "See all" row that opens the
  full view, which itself is paged.
- **Keyboard.** Every action is reachable with Tab and Return/Space; focus is visible (`:focus-visible` ring from
  tokens); Escape closes the topmost layer (menu, dialog, detail pane, then nothing). The sidebar answers ⌘1 to ⌘7 in
  sidebar order; ⌘, opens Settings; ⌘F focuses the current screen's search.
- **Reduced motion.** With `prefers-reduced-motion: reduce`, every transition becomes a cross-fade of 120 ms or less,
  Yappy holds a still pose per phase, parallax is off, and the sun clock steps once a minute instead of gliding.
- **Telemetry** (rule `telemetry`). One structured line per user action and per failure:
  `ui <event> key=value`, never transcript text, audio, keys or full paths. Suggested names, so the items agree:
  `ui nav to=<nav>`, `ui settings_search hits=<n>`, `ui meeting_open id=<short>`, `ui meeting_delete id=<short> undone=<bool>`,
  `ui pill_action action=<start_meeting|stop_meeting|note|scratchpad|dictate>`, `ui tray_click id=<menu id>`.
  A surfaced failure is one sentence plus one action, through `toast()` in `appShell.ts` or `status.lastError`.
- **Screenshots.** Before/after PNGs come from headless Chrome through `scripts/headless-chrome.mjs` (screens) and
  `scripts/pill-matrix.mjs` (pill), never from the real app. The integrator renders them once per batch. The capture
  matrix each item must supply is in section 7.

---

## 1. Information architecture

### 1.1 Windows

| Window | What it is | TODAY | Target |
|---|---|---|---|
| Main | The app: sidebar rail plus one screen at a time | `App.tsx`, nav type `Nav` in `appTypes.ts:10-17` | Same window, fixed width 880 px default (Home page v3), rail collapses to icons under 900 px |
| Pill (`float`) | One NSPanel webview, always on top | `float_pill.rs`, `float-main.tsx` mounts the pill AND `pill/MeetingBadge.tsx` as two overlays that drift apart | ONE pill, one state machine (§23, `pill/state.ts`); the meeting clock is a slot inside it (DECIDED, timer-in-pill) |
| Scratchpad | A small notes window | `scratchpad-main.tsx`, `scratchpad::WINDOW_LABEL` | Unchanged window; opened from the pill hover tray and the menu bar |
| Menu bar | The tray menu | `tray_menu.rs MENU_LAYOUT` (23 slots) | Regrouped (section 5) |

Two windows exist at launch: main and pill (DECIDED, timer-in-pill: "2 windows at launch"). Scratchpad opens on demand.

### 1.2 Objects and where each lives

| Object | Created from | Found in | Edited in | Deleted / restored in |
|---|---|---|---|---|
| Take (one dictation) | Pill, fn key, menu bar | Home (last 5), Home "See all" history | History row (copy, re-paste, reveal edit) | History row; 10 s Undo |
| Meeting | Meetings "Record", pill hover tray, menu bar, ⌃⌘M, detected-call offer card | Meetings list (search, filter by kind / has notes / person) | Meetings detail (rename, notes, labels) | Meetings detail; 10 s Undo (restore is missing TODAY, §15) |
| Meeting note (timestamped line) | Pill "Note" while recording (§18 DEFAULT) | Meetings detail, Transcript and Notes tabs | Inline in the detail | Inline; Undo |
| Person / voiceprint | "Who's talking?" strip, People tab (§8 to §10) | Meetings detail People tab; Settings › Meetings › Speakers | People tab rename / merge | Settings › Meetings › Speakers; Undo |
| Dictionary word | Dictionary screen, Gulp's suggestion after a new word (PROPOSED) | Dictionary screen | Dictionary screen | Dictionary screen; Undo |
| Snippet | Settings › Dictation › Snippets | Same | Same | Same; Undo |
| Scratchpad page | Scratchpad window, pill hover tray | Sidebar Scratchpad, Scratchpad window | Scratchpad window | Scratchpad; Undo |
| Export | Meetings detail export menu (`meetings/ExportMenu.tsx`) | Finder (Show in Finder, §21) | n/a | n/a |

### 1.3 Main window sections (sidebar order)

| # | Nav id | Label | Purpose | Badge |
|---|---|---|---|---|
| 1 | `home` | Home | The studio room, a stats line, the last take, the last 5 takes, Needs attention | count of takes (TODAY) |
| 2 | `permissions` | Permissions | Microphone, Accessibility, Input Monitoring, system audio grants and repair | shown only while a grant is missing (TODAY shows it with a count, `App.tsx:142`) |
| 3 | `meetings` | Meetings | Two panes: list and detail (§15) | count |
| 4 | `insights` | Insights | Three headline numbers, one sentence, then charts (§16) | none |
| 5 | `dictionary` | Dictionary | Words Yap should spell your way | count |
| 6 | `scratchpad` | Scratchpad | Pages of loose notes | count |
| 7 | `settings` | Settings | Five sections plus Account & help (section 1.4) | none |

Permissions **DEFAULT**: the sidebar row hides once every grant is green; the screen stays reachable from Settings ›
Models & privacy › Permissions and from any permission state card. Onboarding (`OnboardingFlow.tsx`) is not a sidebar
item; it runs before the main window's first screen and ends on Home.

### 1.4 Settings IA (§14 DEFAULT, with today's decisions applied)

Settings opens as a screen with a slim left list of sections, a search field above it, one main column, and a pinned
footer with Done (Done returns to the screen you came from). Each control appears exactly once. Search matches labels and
help text across every section and jumps to the control, highlighting it for 2 s.

| New section | Holds | From TODAY's tabs (`SETTINGS_TABS`, `appTypes.ts:34-53`) |
|---|---|---|
| General | Shortcut, language, start at login, microphone input | Shortcut, Audio (input device), parts of Advanced |
| Dictation | Formatting, snippets, link to Dictionary, audio cleanup, "Pastes when you pause" (DECIDED, paste-waits) | Dictation, Snippets, Audio (cleanup) |
| Meetings | Call audio, kinds, retention, detected-call offer, calendar, Speakers / voiceprints, destinations, "Ask who is talking during a meeting" OFF by default (DECIDED, meeting-questions) | Meetings |
| Yappy & pill | Voice (Best friend / Badass / Professional, curse mode Badass-only and off by default; DECIDED, voices), how often Yappy talks, Friends visit on/off, dock position, Yappy News refresh (DECIDED, design-page-8), Social presence (HELD with Social) | Companion |
| Models & privacy | Speech engine, notes and polish engine (Cloud / On this Mac), privacy, Permissions link, Delete all recordings (moved from Home, PROPOSED Home decision 5) | Privacy, Advanced (model parts), `settings/ModelProviders.tsx` |
| Account & help (footer link, not a section) | License, Diagnostics, Send feedback | License, Diagnostics |

Pill style control: **removed**. The Classic pill is retired (DECIDED, design-page-2), so there is one style; users whose
saved style is Classic, or who have none (Classic is today's fallback), migrate to Yappy silently.

Engine names in Models & privacy never name a vendor or model: "Speech engine: On this Mac", "Notes: Cloud (falls back to
On this Mac when offline)".

---

## 2. Navigation map

### 2.1 Ways in

| Target | From |
|---|---|
| Main window, Home | Dock icon, menu bar "Open Yap", end of onboarding, clicking a "Notes ready" notification lands on Meetings instead |
| Home history ("See all") | Home last-5 list footer row, menu bar "Transcript History…" |
| Permissions | Sidebar (while a grant is missing), any permission state card, pill `blocked` phase ("Microphone blocked — open Permissions"), Settings › Models & privacy |
| Meetings list | Sidebar, ⌘3 |
| Meetings detail | Click a list row, "Notes ready" in the menu bar (`ID_NOTES_READY`), notification, Home Needs attention row |
| Meetings recording hero | Starting a meeting from anywhere (Meetings Record, pill, menu bar, ⌃⌘M, offer card) while the main window is open |
| Insights | Sidebar, ⌘4 |
| Dictionary | Sidebar, ⌘5, Settings › Dictation link, Gulp suggestion (PROPOSED) |
| Scratchpad | Sidebar, ⌘6, pill hover tray (PROPOSED), pill "Note" when no meeting is running (§18 DEFAULT) |
| Settings | Sidebar, ⌘7, ⌘,, menu bar "Settings…" |
| Settings › General › Shortcut | Menu bar "Keyboard Shortcuts…" |
| Account & help › License | Pill `gated` phase click target (Trial ended, pill page section 13, PROPOSED; purchase URL is a placeholder TODAY), menu bar license header |

### 2.2 Ways out

| From | Out |
|---|---|
| Any main screen | Sidebar to another screen; ⌘W closes the window (the app keeps running in the menu bar) |
| Home history | Back row "Home", Escape |
| Meetings detail | Escape or the list (the list never disappears in two-pane width); under 900 px the detail covers the list and has a Back button |
| Recording hero | Stop (confirm only when the meeting is under 10 s or over 3 h); the hero becomes the meeting's detail once it stops |
| Settings | Done in the pinned footer, Escape, or the sidebar |
| Dialogs and menus | Escape, Cancel, or clicking outside (never for destructive confirms) |
| Pill hover tray | Mouse leaves, Escape, any action |
| Onboarding | Finish lands on Home; Skip on any step lands on Home with Needs attention listing what was skipped |

### 2.3 Map

```
Menu bar ──Open Yap──────────────┐
Dock icon ───────────────────────┤
Onboarding ──finish/skip─────────┤
                                 v
      ┌──────────── Main window (rail) ─────────────────────────────┐
      │ Home ──See all──> History ──Back──> Home                    │
      │  └─Needs attention──> Permissions | Meetings detail         │
      │ Permissions <── pill "blocked", any permission card         │
      │ Meetings: list <──> detail (Notes | Transcript | People)    │
      │            └─ recording hero ──stop──> detail               │
      │ Insights ──chart point──> Meetings detail / History day     │
      │ Dictionary                                                  │
      │ Scratchpad <── pill tray / pill Note (no meeting)           │
      │ Settings: General | Dictation | Meetings | Yappy & pill |   │
      │           Models & privacy  + footer: Account & help, Done  │
      └─────────────────────────────────────────────────────────────┘
Pill ──hover tray──> Dictate | Record meeting | Scratchpad
     ──while recording──> Note | Stop meeting
     ──blocked──> Permissions     ──gated──> Account & help › License
Menu bar ──> Stop/Record meeting, Dictation, Open (Yap, History, Settings), Pill, Quit
```

---

## 3. Screen states

Every screen below lists the five required states plus keyboard. "Offline" means no network; Yap's local engines keep
working, so offline is mostly a chip, never a blocking screen. Copy is draft copy for the builders; plain words, one
sentence plus at most one action.

### 3.1 Home

| State | What shows |
|---|---|
| Normal | The layered studio room full width at a whole-number scale in its wooden frame (DECIDED, studio-rooms and design-page-7; Home v1793988476 frame), one stats line, the last take card (live while a take runs), the last 5 takes with a "See all" row, Needs attention (hidden when empty), footer |
| Empty (no takes yet) | The room, then "Hold fn and talk. Your first take shows up here." with the key drawn as a keycap. No stat boxes |
| Loading | The room draws immediately (it is local art); the stats line and list show 5 skeleton rows at their final height, so nothing jumps |
| Error (history failed to load) | "Couldn't load your recent takes." + Retry. The room still shows |
| Permission | Needs attention lists the missing grant first: "Yap can't hear the microphone." + Open Permissions |
| Offline | Nothing changes except a small "Offline: using On this Mac" chip in the header when cloud polish is on; Yappy News card shows the last briefing with "Updated <time>" |
| Keyboard | Tab order: last take, list rows, See all, Needs attention rows; Return opens; ⌘C on a focused row copies it |

### 3.2 Permissions

| State | What shows |
|---|---|
| Normal (all green) | Each grant as a row with a green check and what it lets Yap do |
| Empty | n/a (always four rows) |
| Loading | Rows with "Checking…" while the probe runs (`permissionRepair.ts`) |
| Error (probe failed) | "Couldn't check permissions." + Check again |
| Permission (a grant missing or stale) | The row turns warn tone with one action: Open System Settings or Repair (stale grant after a signing change) |
| Offline | No change |
| Keyboard | Each row's action button is in tab order; Return runs it |

### 3.3 Meetings (§15)

Two panes: list on the left (date groups, people chips, state chip), detail on the right. Detail header: title, date,
duration, tracks chip (You / Call, §4), people chips. Tabs: Notes | Transcript | People. The "Who's talking?" strip
(§9) sits on top of the detail when a speaker needs a name. Recording gets its own hero card with the live transcript.

| State | List | Detail |
|---|---|---|
| Normal | Groups Today / Yesterday / This week / Earlier, each row: title, time, duration, chips | Header, tabs, content |
| Empty | "No meetings yet. Record one, or Yap will offer when a call starts." + Record | "Pick a meeting" placeholder (only in two-pane width) |
| Loading | 6 skeleton rows | Skeleton header, skeleton paragraphs |
| Processing (meeting-specific) | Row chip: Transcribing / Finding speakers / Writing notes, with real progress where it exists | Transcript tab shows what is ready; Notes tab "Writing notes…"; never a fake percentage |
| Error | "Couldn't load meetings." + Retry | Per-stage: "Notes didn't finish." + Try again; a stuck meeting (e.g. TODAY dddbea70, "transcribing" with 0 segments) shows "This meeting stopped early and has no audio we can use." + Delete |
| Permission | Banner above the list: "Call audio is off, so only your side is recorded." + Turn on (system audio, `meetings/systemAudio.ts`) | Tracks chip shows "You only" |
| Offline | No change to the list | Notes tab: "Notes will be written on this Mac." chip when the cloud notes path is off |
| Recording hero | Pinned above the list: red dot, title, sun-clock time (DECIDED, meeting-clock B), live transcript (You / Call, then speaker names once live labels land, §11), Note field, Stop | Hero is the detail while recording |
| Delete | Row removes; toast "Meeting deleted." + Undo for 10 s; then files go | Same |
| Keyboard | ↑/↓ move the selection, Return opens, ⌘F search, ⌫ deletes with toast | ⌘1/2/3 do not switch tabs (those are sidebar); use ⌥⌘1/2/3 for Notes / Transcript / People; Escape back to list |

Speaker labels are suggestions with a reason. Asking during a meeting is OFF by default; Yap asks again when less than
70% sure (DECIDED, meeting-questions; one named 0.70 constant).

### 3.4 Insights (§16)

| State | What shows |
|---|---|
| Normal | Three headline numbers (words this week, time saved, hours in meetings), one insight sentence, then charts (7 days, 30-day words, 365-day heatmap, months, top apps), then a meetings section (hours, top people from §8). Follow the `dataviz` skill |
| Empty | Each chart keeps its frame with "Not enough takes yet; this fills in after a few days." No zero-height charts |
| Loading | Chart frames at final size with a quiet skeleton; numbers show "—" |
| Error | Per chart: "Couldn't load this chart." + Retry; other charts still render |
| Permission | n/a (Insights reads only Yap's own data) |
| Offline | No change |
| Keyboard | Each chart is one tab stop; arrow keys move through points and announce "Tuesday, 1,204 words" |

### 3.5 Dictionary

| State | What shows |
|---|---|
| Normal | Searchable list of words, each with its spelling and when it was added; Add word |
| Empty | "Add a name or word Yap keeps getting wrong." + Add word |
| Loading | Skeleton rows |
| Error | "Couldn't save that word." + Try again (the typed word stays in the field) |
| Permission / Offline | n/a / no change |
| Keyboard | ⌘N add, Return save, Escape cancel edit, ⌫ delete with Undo |

### 3.6 Scratchpad (sidebar screen and window)

| State | What shows |
|---|---|
| Normal | Page list and an editor; saves on every pause, "Saved" in the footer |
| Empty | "Jot something down, or dictate into it." + New page |
| Loading | Editor frame with a skeleton |
| Error | "Couldn't save. Your text is still here." + Try again (never drop typed text) |
| Permission / Offline | n/a / no change |
| Keyboard | ⌘N new page, ⌘W closes the window, Escape leaves the editor focus |

### 3.7 Settings (§14)

| State | What shows |
|---|---|
| Normal | Section list, search, main column, pinned footer with Done; Account & help in the footer |
| Empty (search) | "No setting matches '<query>'." + Clear search |
| Loading | Controls render at final size disabled until their values load; never flash a default |
| Error (save failed) | Inline under the control: "Couldn't save this." + Try again; the control reverts |
| Permission | Controls that need a grant (call audio, start at login, shortcut monitoring) show the grant inline + Open Permissions |
| Offline | Cloud controls show "Offline: Yap is using On this Mac." License check and update check say when they last succeeded |
| Keyboard | ⌘F search, ↑/↓ in the section list, Tab into the column, Escape or Done leaves |

### 3.8 Onboarding

| State | What shows |
|---|---|
| Normal | Steps: microphone, accessibility, shortcut, speech engine download, try a take. The launch walk (Yappy walks to work, speed bound to real engine-load progress, never a timer; DECIDED, design-page-6) |
| Loading (engine download / load) | Real percent from `model_download_progress`; loading into memory has no percent TODAY, so it shows three named steps (PROPOSED backend signal, pill page section 10) |
| Error (download failed) | "Download stopped." + Resume; disk-full says how much space is needed |
| Permission | Each grant step explains what it enables, has Open System Settings, and re-checks on return |
| Offline | Download step: "Connect to the internet to get the speech engine (about <size>). Everything after that works offline." |
| Keyboard | Return continues, ⌘[ goes back a step, Skip is reachable by Tab |

---

## 4. Pill and Yappy state table

The pill is one window with one reducer (§23): `{dictation phase} x {meeting: none | recording(elapsed, callAudio) |
stopping} x {dock}` gives one layout. Phases are `LivePhase` in `pill/live.ts:387-447`; labels below are today's
accessible labels from `live.ts:470-540`. Hold times are from the design review inventory. "Bubble" is Yappy's speech
bubble: max width, 2-line clamp, its own enter and exit, anchored to the pill box per dock, never over the character or
the dock edge (§19). Bubble copy comes from one table.

### 4.1 Dictation phases

| Phase | Today's label | Tone | Yappy pose (target, §19) | Bubble / hint | Leaves to | Hold |
|---|---|---|---|---|---|---|
| `idle` | Start dictation | calm | Resting, blinks | none | listening, sleepy, model-loading | — |
| `sleepy` | Start dictation | calm | Dozing | none | listening, idle on hover | after 12 s idle |
| `model-loading` | Loading speech engine | busy | Warm-up beats bound to load progress (pill page section 10) | "Warming up…" | listening, error | — |
| `listening` | Stop dictation | live | Ears up, mouth follows voice level (`mouth.ts`) | chatter ladder by take length (props: notepad, desk, typewriter) | thinking, transcribing, cancelled, empty | — |
| `listening-locked` | Hands-free — <key> to stop (`handsFreeLabel`, `live.ts:593`) | live | Distinct hands-free pose (§19) | "Hands-free: I keep listening until you press fn", key as keycap, clickable stop (§18) | same as listening | — |
| `transcribing` | Transcribing | busy | Writing | determinate progress when the take has more than one chunk | polishing, pasting, done, error | — |
| `thinking` | Transcribing | busy | Thinking | none | polishing, pasting, done | — |
| `polishing` | Polishing | busy | Tidying (Tidy may visit, PROPOSED) | none | pasting, done | — |
| `pasting` | Pasting | busy | Handing over | "Pastes when you pause" while waiting on a typing pause (DECIDED, paste-waits); "Copied, press ⌃⌘V" when the app changed | done | — |
| `done` | Done | good | Happy, sparkles (static under reduced motion) | reaction word by voice (DECIDED, voices: 48 per voice) | idle | 1600 ms |
| `empty` | Nothing to type | calm | Shrug | "Didn't catch anything." | idle | — |
| `cancelled` | Cancelled | calm | Shrug | none | idle | 1400 ms settle |
| `error` | Something went wrong | warn | Worried | one sentence + Retry on the pill (PROPOSED) | idle, listening | — |
| `blocked` | Microphone blocked — open Permissions | warn | Covers ears | click opens Permissions | idle | — |
| `waiting` | Waiting for microphone permission | warn | Waiting | "Allow the microphone in the dialog." | listening, blocked | — |
| `gated` | trial-ended headline (`GATED_HEADLINE`) | warn | Sits | click opens Account & help › License (placeholder purchase URL TODAY) | idle | 2500 ms settle |
| `blind` | `BLIND_LABEL` (fn blocked in a secure field) | calm | Eyes closed | "Yap can't type into password fields." | idle | — |

`listening-locked` is a phase in code (`live.ts:447`, owned by yap24-PILL4), even though the backlog lists hands-free as a
flag.

### 4.2 Meeting slot (crossed with every phase above)

| Meeting state | What the pill shows | Dock |
|---|---|---|
| none | The pill alone | user's dock (bottom / left / right today; top-center added, PROPOSED as the general default) |
| recording(elapsed, callAudio) | A red dot, the clock and ■ in the pill's own trailing slot (never a second capsule); the sun crosses the sky as the clock (DECIDED, meeting-clock B); Hoot runs the reel-to-reel tape machine beside Yappy (DECIDED, hoot B); call audio shown as a small "Call" mark when the system track is live | top-center under the notch (DECIDED, timer-in-pill) |
| stopping | "Stopping…" in the slot, ■ disabled | unchanged |

Dictating during a meeting: the dictation phase runs in the main slot and the meeting slot keeps its clock. Hover while
recording shows Note and Stop meeting (§18). Left and right docks rotate the meeting slot with the pill; no label ever
renders detached or horizontal beside a vertical pill (Wilson's 9.28.36 AM screenshot). The NSPanel is sized to the union
of pill and bubble per dock, nothing outside it (§23).

### 4.3 Overlays and forms

| Overlay | Rule |
|---|---|
| Hover tray (PROPOSED) | Dictate, Record meeting (with kind), Scratchpad; keyboard reachable; telemetry `ui pill_action` |
| Drag | Collapses to a 36 px Yappy-head blob with dock targets (PROPOSED); Yappy leans in the drag direction, squashes on drop, settles on dock (§19); springs from `motion.ts` (snap 600/35, settle 300/28) |
| Sky | Follows the Home room's time of day (DECIDED, pill-sky C) |
| Friends | Visit, never move in: one at a time, 4 s max in the pill, once a day after take 50 (PROPOSED, companions page); Friends visit switch in Settings › Yappy & pill |
| Quieting | Heavy-user ladder 1-20 / 21-50 / 51-200 / 200+ takes and the "Yappy talks" setting, default Sometimes (PROPOSED, pill page decision 6) |

### 4.4 Motion

Today's constants: phase morph 100 ms `cubic-bezier(0.05,0.6,0.4,0.95)`, expand 340 ms, collapse 300 ms. §19 adds a
transition table (from, to, duration, easing) as data with a test that every phase pair has an entry. Under reduced motion
every pair resolves to a 120 ms cross-fade.

---

## 5. Menu-bar menu

### 5.1 TODAY (`tray_menu.rs:273-300`, 23 slots)

License header · — · Start Dictation · Paste Last Transcript · Copy Last Transcript · Undo AI Edit (Paste Raw) (`ID_PASTE_RAW`) · — ·
meeting toggle ("Record a meeting" / "Stop meeting · 12:04" / "Stopping…") · "Record a meeting as…" kind submenu · — ·
hands-free state (`ID_HANDS_FREE`) · — · Open Yap · Transcript History… · Settings… · Keyboard Shortcuts… · — · Hide Pill
for 1 Hour (`ID_HIDE_PILL`) · Show Pill Now · — · Send Feedback… · — · Quit Yap. Inserted items: Upgrade, "Meeting notes ready — Open"
(`ID_NOTES_READY`), and the disabled "Recording · <title>" header (`ID_MEETING_HEADER`). Item §2 (tray meeting toggle)
already put Stop first while recording, with the clock refreshed at most once a minute (OS-12: no per-second menu
mutation).

### 5.2 Target (§17 DEFAULT)

```
Yap · Trial, 9 days left            (status header: license, or "Recording · Standup · 12:04")
────────────────
Stop meeting · 12:04                 (while recording; first item)      or   Record a meeting
                                                                              Record a meeting as…  ▸ kinds
Notes ready: <title>                 (only when a meeting's notes just finished)
────────────────
Start Dictation            <live chord>
Paste Last Transcript      <live chord>
Copy Last Transcript
────────────────
Open Yap
Transcript History…
Settings…                  ⌘,
────────────────
Hide Pill for 1 Hour   /   Show Pill Now
────────────────
Send Feedback…
Quit Yap                   ⌘Q
```

Rules: accelerators come from the live bindings, as now. The kind submenu is disabled while recording. Keep the
`MENU_LAYOUT` single-source test (`tests/tray_menu.rs`). Every click logs `tray <id> clicked` (item §2 format for the
meeting toggle). Keyboard Shortcuts… moves into Settings › General and leaves the menu (DEFAULT; restore it if Wilson's
mockup keeps it). Paste raw moves under an ⌥-alternate of Paste Last Transcript and is relabelled "Paste Without Cleanup" (DEFAULT: plain words, no "AI" in the label).

---

## 6. Mockup slots

One slot per screen. Wilson drops a mockup by saving the image under `docs/design/mockups/` with the slot's file name and
changing its status line. A builder whose screen has a filled slot builds the mockup; an empty slot means build from this
spec. Design-review pages already made today are listed as references (they live in the vault, `Projects/Yap/`).

| Slot | Screen | File | Status | Reference so far |
|---|---|---|---|---|
| M-01 | Main shell: sidebar rail, header, footer, rail collapsed under 900 px | `mockups/M-01-shell.png` | EMPTY | Home page v3 (fixed 880 px window, pinned footer) |
| M-02 | Home | `mockups/M-02-home.png` | EMPTY (design approved) | `design-2026-11-06/home.html`, section 05 (layered room in the wooden frame) |
| M-03 | Home history ("See all") | `mockups/M-03-history.png` | EMPTY | none |
| M-04 | Permissions | `mockups/M-04-permissions.png` | EMPTY | none |
| M-05 | Meetings list + detail (Notes / Transcript / People) | `mockups/M-05-meetings.png` | EMPTY (design approved) | Meetings "who said this?" page (after-meeting cards, voiceprints) |
| M-06 | Meetings recording hero | `mockups/M-06-recording.png` | EMPTY | pill page section 06 storyboard (45-minute call) |
| M-07 | Insights | `mockups/M-07-insights.png` | EMPTY | none |
| M-08 | Dictionary | `mockups/M-08-dictionary.png` | EMPTY | none |
| M-09 | Scratchpad screen and window | `mockups/M-09-scratchpad.png` | EMPTY | Wispr scratchpad crops in the pill page section 07 |
| M-10 | Settings (5 sections, search, footer) | `mockups/M-10-settings.png` | EMPTY | none |
| M-11 | Onboarding and the launch walk | `mockups/M-11-onboarding.png` | EMPTY (walk approved) | companions page, launch walk (`assets/walk.js`) |
| M-12 | Pill: every phase x dock | `mockups/M-12-pill.png` | EMPTY (design approved nearly in full) | `design-2026-11-06/pill.html` |
| M-13 | Pill: meeting slot, sun clock, Hoot's tape | `mockups/M-13-pill-meeting.png` | EMPTY (decided) | pill page sections 05, 09, 15 |
| M-14 | Pill hover tray and drag blob | `mockups/M-14-pill-tray.png` | EMPTY | pill page section 07 (Wispr crops) |
| M-15 | Yappy states and friends | `mockups/M-15-yappy.png` | EMPTY (companions approved) | `design-2026-11-06/companions.html` |
| M-16 | Menu-bar menu | `mockups/M-16-menubar.png` | EMPTY | Wispr menu-bar crop in the pill page section 07 |

---

## 7. Screenshot capture matrix (for the integrator)

| Item | Script | Matrix |
|---|---|---|
| §14 Settings | `scripts/headless-chrome.mjs` | each section x {normal, search empty, save error, permission, offline} at 880 px and 1280 px; device scale 1 and 2; plus the rail under 900 px |
| §15 Meetings | `scripts/headless-chrome.mjs` | {empty, loading, processing, normal, error, permission, recording hero} x {two-pane 1280 px, single 880 px} x scale 1 and 2 |
| §16 Insights | `scripts/headless-chrome.mjs` | {empty, loading, normal, chart error} x scale 1 and 2 |
| §17 Menu bar | none (native menu) | the PR lists the built `MENU_LAYOUT` order and the tray unit test output instead |
| §18, §19 Pill | `scripts/pill-matrix.mjs` | every phase in 4.1 x {no meeting, recording, stopping} x {bottom, left, right, top-center} x scale 1 and 2, failing on overlap between hint, bubble, clock and character boxes; plus reduced motion |

Render every review page at device scale 2 as well as 1 before calling it done (lesson from the side-dock crop bug,
design review 17:37Z).

---

## 8. Which item owns what

| Item | Sections it builds |
|---|---|
| `yap-1106-settings-redesign` (§14) | 1.4, 3.7, rail part of M-01, M-10 |
| `yap-1106-meetings-redesign` (§15) | 1.2 meeting rows, 3.3, M-05, M-06 |
| `yap-1106-insights-redesign` (§16) | 3.4, M-07 |
| `yap-1106-menubar-redesign` (§17) | 5.2, M-16 |
| `yap-1106-pill-notes-and-meeting` (§18, after §23 one-pill) | 4.2, 4.3 hover tray, M-13, M-14 |
| `yap-1106-yappy-states-and-motion` (§19) | 4.1 poses and bubble, 4.3 drag, 4.4, M-12, M-15 |
| Not owned yet | 3.1 Home redesign, 3.2, 3.5, 3.6, 3.8 states; the shell rail (M-01) beyond Settings. These need items before they are built |

## 9. Open questions for Wilson

1. Top-center dock as the default for ALL takes, or only during meetings? Decided for meetings; the general default is
   still PROPOSED.
2. Click on the resting pill starts hands-free, and hover opens the tray: keep both? (PROPOSED, pill page decision 4.)
3. Keep "Keyboard Shortcuts…" in the menu bar, or only in Settings › General? (5.2 DEFAULT drops it.)
4. Should Permissions leave the sidebar once every grant is green? (1.3 DEFAULT hides it.)
