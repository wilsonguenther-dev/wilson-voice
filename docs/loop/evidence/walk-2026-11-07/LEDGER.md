# Walk ledger, 2026-11-07: dictation, Scratchpad, polish, History, meeting, Yappy News

App under walk: **/Applications/Yap Test.app**, CFBundleShortVersionString **0.9.4-loop3.11**,
bundle id **com.wilsonguenther.wilson-voice.test** (both read from its Info.plist with PlistBuddy at
2026-11-07T14:52:41Z, the app was not launched or signalled). Built from main **d4ad0d712b74997d39989e78f01d266225fbc01b**
(the commit named in Wilson's 14:33Z report; the Info.plist carries no commit, so this is his
statement, not a read). Code citations below are file:line on origin/main d4ad0d71.

## What this ledger is, and what it is not

This ledger was written by a CODE-ONLY coder seat (Yap loop three, lane B6). That seat's binding
guardrails forbid opening a GUI app, driving any copy of Yap, sending synthetic keystrokes, and
running vite or a browser build. So **no step below was walked live, no screenshot was taken, and no
app log was read.** Every row carries a static trace of the code path that step exercises instead,
and the steps that only a live run can settle are marked **NOT WALKED** with the exact thing the
walk seat must do. Nothing here claims a PASS.

One defect is proven from the code alone and matches Wilson's words ("dictation doesn't work in the
Yappy app... if I go to the scratch pad it does not"): **a take dictated into any of Yap's own main
window fields is never inserted.** It is row 4 and is filed as item yap-1107-walk-dictate-into-yap-window.

Zero windows of Wilson's regular Chrome were opened by this seat (no browser of any kind was started).

## Rows

| # | Time (date -u) | Entry point | Expected | Happened | Result | Screenshot | Log excerpt |
|---|---|---|---|---|---|---|---|
| 1 | 2026-11-07T14:52:41Z | Click the pill, speak with `say`, stop | Take is transcribed and inserted into the app in front; row in History | Not walked. Static: the stop edge reads ONE identity, `focus::frontmost_identity` (lib.rs:4501-4517), `focus::resolve_paste_target` (focus.rs:878) drops Yap's own bundle (prefix focus.rs:793). If the walker's target is a Yap window the take is held, see row 4 | NOT WALKED | none (seat may not capture) | none (seat may not read the live app log) |
| 2 | 2026-11-07T14:52:41Z | fn / dictate chord | Same as 1 | Not walked. Same stop-edge path as row 1 | NOT WALKED | none | none |
| 3 | 2026-11-07T14:52:41Z | Menu bar "Start Dictation" | Same as 1; label flips to Stop Dictation | Not walked. Static: the item exists, tray_menu.rs:266 "Start Dictation" maps to `TrayCommand::ToggleDictation` (tray_menu.rs:101 "toggle_dictation") | NOT WALKED | none | none |
| 4 | 2026-11-07T14:52:41Z | Open Scratchpad in the MAIN window (sidebar "Scratchpad"), click into the note, dictate | Text lands at the caret of the note | **Code-proven FAIL.** The main-window Scratchpad view is a plain `<textarea>` (views/Scratchpad.tsx:194) that never calls `scratchpad_focus` (only the separate ⌃⌘J panel does, ScratchpadWindow.tsx:226/245 → scratchpad.rs:80), so `scratchpad::dictation_note()` is None (lib.rs:4501) and `paste::take_target` (paste.rs:529, called lib.rs:5164) gives `Dictated(bundle)`. Yap's main window is a normal activating window, so the frontmost app at release AND press is Yap itself; `resolve_paste_target` (focus.rs:878) refuses Yap's own bundle on both reads and returns no bundle id, and `insert_take_for` holds the take with UNKNOWN_TARGET_MESSAGE (paste.rs:888, message paste.rs:191 "I couldn't tell which app you dictated into — held in the pill"). The same path drops a take into ANY field of the main window (the "Yappy app") | FAIL (static, not walked) | none | expected line in the live log for that minute: `paste target captured app=unknown captured_at=release` (lib.rs, the log::info! after the stop edge) |
| 5 | 2026-11-07T14:52:41Z | Polish the Scratchpad text | Polished text replaces / offers over the take | Not walked. Static: polish lives on the pill (float-main.tsx with polishOffer.ts), not in either Scratchpad view; with row 4 failing there is no landed take to polish in the note | NOT WALKED (blocked by row 4) | none | none |
| 6 | 2026-11-07T14:52:41Z | Open History | Takes from rows 1 to 5 present with correct text | Not walked. A held take is still written to History (paste.rs:888 comment "is in History"), so row 4's take should appear even though it never landed | NOT WALKED | none | none |
| 7 | 2026-11-07T14:52:41Z | Record a 20 s meeting with `say`, stop, wait for transcript and notes | Transcript and notes appear in Meetings | Not walked. Needs the mic and the real build | NOT WALKED | none | none |
| 8 | 2026-11-07T14:52:41Z | Yappy News with the saved cloud key: refresh, pick topics | Refresh succeeds on the cloud key; topics selectable | Not walked. Existing items already own the cloud-key path: yap-1107-news-honours-cloud-key and yap-1107-cloud-first-when-key-present (no duplicate filed) | NOT WALKED | none | none |

## Items filed

One FAIL row, one item, written to ~/code/wilson-voice-loop/loop3/add-walk-2026-11-07.json:
**yap-1107-walk-dictate-into-yap-window** (row 4). Checked against docs/loop/ITEMS-2026-11-07-loop3.json
and every loop3/add-*.json: no existing item covers dictation into Yap's own window.
yap-1107-secure-input-own-field-quiet is about the Secure Input warning, not insertion, so it is not
reused as a dependency.

## Cleanup (History and Meetings counts)

Counted read-only from a temp copy of ~/Library/Application Support/WilsonVoice/wilson_voice.db
(+ -wal/-shm), copy deleted right after:

| When (date -u) | History (transcripts) | Meetings |
|---|---|---|
| before, 2026-11-07T14:52:08Z | 2871 | 18 |
| after, 2026-11-07T14:53:14Z | 2871 | 18 |

This seat created no take and no meeting, so the walk added no fixtures and deleted none.

## What the walk seat (or the integrator) still has to do

Run rows 1 to 8 on /Applications/Yap Test.app with the real build, screencapture -l of the Yap
window at 2x into this folder (one PNG per row, named row-<n>.png), paste the app log lines for each
minute, and replace each NOT WALKED with PASS or FAIL. Row 4 is expected to FAIL until
yap-1107-walk-dictate-into-yap-window lands.
