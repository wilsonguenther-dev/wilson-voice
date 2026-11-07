# Live walk ledger: dictation, Scratchpad, polish, History, meeting, News (2026-11-07)

Item: yap-1107-walk-dictation-scratchpad-meeting. Code line numbers below are on b2877116 (origin/main at 22:50Z).

## App under test

| Field | Value | How read |
|---|---|---|
| Bundle | /Applications/Yap Test.app, `com.wilsonguenther.wilson-voice.test` | `plutil -p Contents/Info.plist` (read only, app not launched) at 2026-11-07T22:50Z |
| CFBundleShortVersionString | **0.9.4-loop3.7** | same |
| Build commit | NOT RECORDED: Info.plist carries no commit key; needs the integrator's install log | — |

Note: Wilson walked **0.9.4-loop3.11** (main d4ad0d71) at 14:33Z. The copy installed now reads **0.9.4-loop3.7**, which is older. Before the walk, the walker must reinstall the current build (or confirm which build is installed), or the results describe an old app.

## Status: NOT WALKED (blocked for a code-only seat)

This ledger was written by code-only coder B2. That seat's hard guardrails forbid what the walk needs: launching or driving any copy of Yap, GUI keystrokes, `screencapture` of the Yap window, `say` through the speakers, and running vite/npm for the headless UI shim. So no step below was walked. There are no PASS/FAIL verdicts and no screenshots, and nothing was invented to fill them. The walk has to run in a seat that may drive the installed app (the integrator or a walk seat). Use the step table as the script and fill in the Time / Happened / Verdict / Screenshot / Log columns.

## Steps

| # | Time (date -u) | Entry point (code) | Expected | Happened | Verdict | Screenshot | Log excerpt |
|---|---|---|---|---|---|---|---|
| 1 | — | Pill click: `pill_click_toggle` lib.rs:7228 → `start_recording` lib.rs:3546 | A click starts the take; `say` speech is transcribed; stop pastes into the front app and adds a History row | NOT WALKED | — | docs/loop/evidence/walk-2026-11-07/step1-pill.png (to capture) | — |
| 2 | — | Dictate chord (fn / configured chord) → `start_recording` lib.rs:3546 | Same as 1, started from the hotkey; the chord never reaches the front app | NOT WALKED | — | step2-hotkey.png (to capture) | — |
| 3 | — | Menu bar "Start Dictation": tray_menu.rs:266 (label) / lib.rs:1837 | Same as 1, started from the menu bar item | NOT WALKED | — | step3-menubar.png (to capture) | — |
| 4 | — | Scratchpad: floating panel reports focus via `scratchpad_focus` (scratchpad/ScratchpadWindow.tsx:226,245 → lib.rs:9022 → scratchpad.rs:80 `set_focused_note`); stop edge reads `scratchpad::dictation_note()` lib.rs:4577; `paste::take_target` paste.rs:615 → `PasteTarget::Note` | The text lands in the focused Scratchpad note with the outcome "Added to your Scratchpad note" (paste.rs:626) | NOT WALKED | — | step4-scratchpad.png (to capture) | — |
| 5 | — | Polish: `polish_protocol.rs:240` `polish` | The step-4 text comes back polished in place, with one version row | NOT WALKED | — | step5-polish.png (to capture) | — |
| 6 | — | History: `get_history` lib.rs:7474 | The takes from steps 1-5 are listed with the right text and target app | NOT WALKED | — | step6-history.png (to capture) | — |
| 7 | — | Meeting: `start_meeting` lib.rs:12242 / `stop_meeting` lib.rs:12790 | A 20 s meeting records, then the transcript and notes appear | NOT WALKED | — | step7-meeting.png (to capture) | — |
| 8 | — | Yappy News: `news_refresh` lib.rs:10145 | With the saved cloud key, refresh works and topics can be selected | NOT WALKED | — | step8-news.png (to capture) | — |

## What the code shows ahead of the walk (code reading only, NOT a walk result)

Wilson's report was "dictation doesn't work in the Yappy app... if I go to the scratch pad it does not". The code reading found two likely causes. The walk has to confirm them before any item is filed.

H1. Dictating into Yap's own main window never lands. `focus::resolve_paste_target` (focus.rs:937-963) drops the app at the front at release when it is Yap itself (`usable_target` focus.rs:918 → `is_own_app` focus.rs:906). If the press read is also Yap, the target bundle id is `None`, and per its doc comment "an unconfirmed target is never typed into", so the take is held on the pill. The call site is lib.rs:4586-4592. The likely symptom: dictating into any Yap text field (the main window's Scratchpad view, Settings fields, a note) does nothing visible apart from a held take.

H2. The main window's Scratchpad view is not a dictation target. desktop/src/views/Scratchpad.tsx only calls `list_scratch` (line 94) and `open_scratchpad` (line 213). It never reports `scratchpad_focus`, so `scratchpad::dictation_note()` stays `None` and H1 applies. Only the floating panel (scratchpad/ScratchpadWindow.tsx:226,245) is wired to `PasteTarget::Note`. If Wilson's "scratch pad" was the main-window view, this explains his report.

Existing items checked so these are not duplicates: yap-1106-paste-target-defect (wrong target app in History, a different defect), fix-b5-1-pill-note-opens-its-own-note, yap-wf-15-scratchpad-window-controls, and yap-1107-secure-input-own-field-quiet (Secure Input alarm, not delivery). None covers dictating into Yap's own window.

## New items (acceptance 3)

No add-walk-2026-11-07.json was written because there are no observed FAIL rows. H1 and H2 become items only after the walk confirms them, one item per FAIL row.

## Cleanup (acceptance 4)

The baseline was read from a temp copy of wilson_voice.db (+wal/shm), and the copy was deleted right away. Wilson's data dir was not written.

| When | History (`transcripts`) | Meetings (`meetings`) |
|---|---|---|
| Before (2026-11-07T22:51:20Z) | 2916 | 18 |
| After | n/a: no takes or meetings were created by this seat | n/a |

The walker records its own before/after counts with the same method. Its takes and meetings must be deleted until the counts match.

## Chrome (acceptance 5)

This seat opened zero windows of Wilson's regular Chrome and started no browser of any kind.
