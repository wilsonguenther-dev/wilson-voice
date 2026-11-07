# Wispr Flow vs Yap: dictation micro-behaviours (2026-11-07)

Written 2026-11-07T16:58Z by coder seat B3 of Yap loop three, item `yap-1107-wispr-flow-parity-sweep`. Revised 2026-11-07T17:53Z by coder seat B6 after batch 5 bounced it because rows 6, 8, 10 and 12 had no verdict: every row now has a PARITY / GAP / YAP-BETTER verdict, and the four rows are settled from Wispr's own shipped code (see below). The branch was brought up to date with `loop3/integration-5`; the Yap file:line references stay pinned to 31f07d08 so they can be checked with `git show 31f07d08:<path>`.

| | Version | Where it came from |
|---|---|---|
| Yap Test | **0.9.4-loop3.2** (batch 2), installed 2026-11-07T16:54Z | `/Applications/Yap Test.app/Contents/Info.plist` CFBundleVersion; loop log `Loop-Logs/2026-11-07-loop3.md:477-478`. The build commit is not stamped in the bundle (no commit key in Info.plist). |
| Yap code read | `loop3/integration-3` **31f07d08** (batch 3, one batch newer than Yap Test) | `git log -1 origin/loop3/integration-3` |
| Wispr Flow | **1.6.1400** (build 1.6.1400) | `/Applications/Wispr Flow.app/Contents/Info.plist` CFBundleShortVersionString / CFBundleVersion |

## What this sheet is, and what it is not

This seat was a code-only coder. Its loop rules forbid launching any copy of Yap (Yap Test included) and opening any GUI app, so **the live side-by-side was not driven here.** Each row is built from two sources:

- **Wispr side, documented:** Wispr's own help center, fetched headless with `curl` on 2026-11-07 and saved verbatim in [`wispr-help-docs-2026-11-07.txt`](wispr-help-docs-2026-11-07.txt). The `docs:N` citations point at line N of that file.
- **Wispr side, where the help center is silent (rows 6, 8, 10, 12):** Wispr Flow 1.6.1400's installed code, read as files and never run: its JavaScript bundle (`app.asar`) and its Swift helper binary. The excerpts, each with its byte offset, are saved in [`wispr-bundle-excerpts-2026-11-07.txt`](wispr-bundle-excerpts-2026-11-07.txt); `W:N` citations point at excerpt [WN] in that file. Reading the shipped code tells us what the app is built to do, which is enough for a verdict; the live walk below would add a recording on top, not change a verdict.
- **Yap side:** the code at 31f07d08, with file:line references. The phase and size screenshots are the integration-3 pill-matrix 2x captures, copied into this folder.

So the verdicts below come from Wispr's documentation and shipped code plus Yap's code; none was watched on screen. The section "Live walk (optional, for a seat that may launch apps)" lists the steps that would add recordings.

## Parity docs checked first

| Doc | What it covers | Used for |
|---|---|---|
| `~/Obsidian/Wilson-Brain/Projects/Yap/WISPR-FLOW-AUDIT-2026-11-06.md` | 82 Wispr features, 20 items WF-01..WF-20 | Feature-level baseline. Most WF items have landed since (see the next table). |
| `~/Obsidian/Wilson-Brain/Projects/Yap/wispr-notetaker-parity-2026-10-16.md` | Notetaker / Meetings pipeline | Only the meeting-while-dictating row. The meetings pipeline itself is out of scope for this sheet and was not re-checked. |
| `docs/research/wispr-parity.md` | Polish / context / commands / dictionary ("intelligence") | Password-field privacy note (`wispr-parity.md:115`). No micro-behaviour rows: that doc is about text, not gestures. |
| `docs/research/wispr-formatting-deep-dive.md` | Formatting spec, §4 pill docking | No micro-behaviour rows. The pill-state mapping comes from `desktop/src/pill/dock-geometry.json`, which is built from it. |

### Earlier findings that are now fixed (checked in code at 31f07d08)

| Earlier finding (audit 2026-11-06) | Now | Evidence |
|---|---|---|
| #3 / WF-01: the Yappy pill click does not dictate | Fixed: click under 4 pt and 300 ms toggles a take | `desktop/src/pill/press.ts:13-15` |
| #4 / WF-02: no drag blob | Fixed | `desktop/src/pill/forced.ts:293` |
| #5: no Esc cancel while dragging the pill | Fixed | `desktop/src-tauri/src/float_pill.rs:1328` `esc_cancels_drag` |
| #7 / WF-04: hide 1 h is only in the tray | Fixed: the pill's right-click menu | `desktop/src-tauri/src/tray_menu.rs:648-657` |
| #16 / WF-06: no "Backtrack caught" toast | Fixed | `desktop/src/pill/backtrackCaught.ts` |
| #17 / WF-05: no "Polished, Reveal edit" toast | Fixed | `desktop/src/pill/polishReveal.ts` |
| #39 / WF-08: no meeting popover | Fixed | `desktop/src/pill/meetingPopover.ts` |
| #13 / WF-12: no "different language?" card | Fixed | `desktop/src/pill/languageOffer.ts` |
| #58 / WF-13: no silent-mic warning | Fixed | `desktop/src/pill/silentMic.ts` |
| #35 / WF-14: no "Update ready" | Fixed | `desktop/src-tauri/src/tray_menu.rs:79` |
| #10 / WF-16: Yap visible in screen shares during meetings | Fixed | `desktop/src-tauri/src/float_pill.rs:727` |
| #63 / WF-19: no mouse-button push-to-talk | Fixed | `desktop/src-tauri/src/ptt_macos.rs:386` `on_mouse_button` |
| #23 / WF-20: no style chip | Fixed | `desktop/src/pill/styleChip.ts` |

## The sheet

Verdicts: **PARITY**, **GAP** or **YAP-BETTER**. Every row has one. Paths are relative to the repo root unless they start with `docs:` (the Wispr help-center file in this folder) or `W:` (an excerpt in `wispr-bundle-excerpts-2026-11-07.txt`).

| # | Behaviour | Wispr behaviour | Yap behaviour | Evidence | Verdict |
|---|---|---|---|---|---|
| 1 | Start/stop symmetry across surfaces (key, pill, menu bar, Dictate button) | Hold Fn, double-press Fn, Fn+Space, a click on the bar, and the Notetaker controls all start and stop the same session. | A take started from one surface does not always stop from another. | Already filed as `yap-1107-start-stop-any-surface` in ITEMS-2026-11-07-loop3.json. Not filed again. | GAP (already an item) |
| 2 | Click on the pill / bar | A click on the bar starts dictation (audit #3, Wilson's screenshot). | A press under 4 pt of travel and 300 ms toggles a take; longer is a drag. Some clicks do not register. | `desktop/src/pill/press.ts:13-15`; open item `yap-1107-pill-click-always-registers` | GAP (already an item) |
| 3 | Hold vs toggle on the key | Hold Fn = push-to-talk; double-press Fn **within 500 ms** = hands-free; Fn+Space = hands-free; a quick second press of the hands-free shortcut ends it. | Hold past 280 ms = push-to-talk; two taps (each ≤320 ms) **within 450 ms** = hands-free; a tap while hands-free stops and pastes; ⌃⌘H chord. Capture starts on the press, so the 280 ms wait never loses audio. | docs:219, docs:236, docs:64; `desktop/src-tauri/src/ptt_macos.rs:701-705`, `:907-943` `on_down`, `:974-993` `on_tap_up` | PARITY (window 450 vs 500 ms is folded into gap G2) |
| 4 | Third press right after a double-press | **A third press within the same 500 ms cancels the just-started hands-free session.** | A third press goes through `on_down`'s hands-free branch and emits `HandsFreeOff, Stop`: the near-empty take is STOPPED and goes through decode, so a fumbled triple-tap shows "empty" or pastes noise instead of vanishing. | docs:237; `desktop/src-tauri/src/ptt_macos.rs:916-925` | **GAP → G2** |
| 5 | Escape to cancel mid-take | Escape cancels. On Mac, Escape is withheld from other apps **only during dictation**. | Escape is registered only for the life of one take and handed back on every exit; it cancels the take. | docs:43, docs:219; `desktop/src-tauri/src/lib.rs:3683-3694` `register_cancel_shortcut`, `:3949` `cancel_take` | PARITY |
| 6 | Escape during processing (decode / polish / paste) | Escape cancels at every stage. The Escape handler first cancels a running polish, then dismisses the dictation (W:1, W:4); while transcribing, Wispr checks for that dismiss and throws the result away (W:2). The audio was already saved to History when processing began (W:3). | One cancel for every stage: it raises the cooperative flag, ends recording, and fires the engine's cancel hook mid-decode. The audio is parked under recovery, so "actually, transcribe that" still works from History. | W:1-W:4; `desktop/src-tauri/src/lib.rs:3910-3967`, `:3949` `cancel_take`, `:3760-3770` | PARITY (both cancel mid-processing and keep the audio) |
| 7 | Fn + another key during a press | Fn + a system key (arrows, Home/End, PgUp/PgDn, forward delete, F1-F12) **in the first second** cancels silently. Other Fn combinations show a cancellation notice. | Any non-modifier key during a hold that has not latched hands-free cuts the take and discards the pre-armed audio. A take that is already decoding is never cancelled by a key (SP-02). | docs:44-45; `desktop/src-tauri/src/ptt_macos.rs:1047-1064` `key_down`; `desktop/src-tauri/src/lib.rs:13224-13236` | PARITY |
| 8 | Paste into a password / Secure Input field | On Mac Wispr has no password-field paste rule: it only reads `AXSecureTextField` when capturing context (W:5), and its one "paste blocked" notice is the Windows "Run Flow as Administrator to paste anywhere" case (W:6, W:7). So it pastes into a password field like any other field. Android hides the bubble in password fields (docs:122); Wispr says password contents are never read (`docs/research/wispr-parity.md:115`). | Refuses to paste into a secure field: "Not pasted: that's a password field. Held in the pill, not on your clipboard." The field's own AX answer wins, and the global Secure Input flag decides only when AX is blind. | W:5-W:7, docs:122; `desktop/src-tauri/src/paste.rs:194-207` `SECURE_FIELD_MESSAGE` / `secure_target`, `:842-845` | YAP-BETTER (a dictated sentence never lands in a password box, and the words stay on the pill, not the clipboard) |
| 9 | Shortcuts while another app holds Secure Input | Fn+Space and Escape can stop working. **"Flow shows no on-screen alert for this block."** Holding Fn still works. | The fn tap status names the holder: "Blocked by {app} (Secure Input)". The Carbon chord is delivered under Secure Input. | docs:217-234; `desktop/src-tauri/src/hotkey_table.rs:12`, `:151` | YAP-BETTER |
| 10 | Paste timing after a typing pause | Pastes as soon as the text is ready. The helper only logs when keys are still down at paste time (W:8); nothing in the bundle waits for the typing to stop. | If a key went down in the last 1.5 s, the paste is held ("Pastes when you pause") until 600 ms pass with no key, or 5 s at most. Switching apps during the hold copies the take and holds it on the pill. | W:8; `desktop/src-tauri/src/typing_pause.rs:25-33`; `desktop/src-tauri/src/paste.rs:1381-1389`, `:1421-1444` | YAP-BETTER (Wilson asked for this in fb-04: the take never lands in the middle of a word he is typing) |
| 11 | Pill feedback states: idle / arming / listening / processing / done / error | Bar states from the bundle: `resting`, `ready` (armed, not yet listening), `activePopo` (listening), `processing`, `polishProcessing`, `polishCompleted`, `error`. | Every Wispr state has a Yap phase: idle → resting, listening → activePopo, transcribing/pasting/thinking → processing, polishing → polishProcessing, done → polishCompleted, empty/cancelled/error → error. **Yap's only use of `ready` is model-loading.** The fn press itself emits no event until the 280 ms hold arm, so the pill shows nothing for the first 280 ms of every press. | `desktop/src/pill/dock-geometry.json:6-115`; `desktop/src/pill/live.ts:523-564`; `desktop/src-tauri/src/ptt_macos.rs:931-942`; screenshots [`yap-pill-idle__bottom.png`](yap-pill-idle__bottom.png), [`yap-pill-listening__bottom.png`](yap-pill-listening__bottom.png), [`yap-pill-done__bottom.png`](yap-pill-done__bottom.png) | **GAP → G3** (arming); the other states are PARITY |
| 12 | Hotkey while a meeting runs | An active Notetaker meeting keeps the bar visible even when it is hidden or snoozed (docs:114). The hotkey works mid-meeting: dictation and meeting capture run at the same time (W:9, W:10), the meeting keeps recording, and nothing asks first. | The pill always shows while a meeting records. The first hands-free take of a meeting asks once: "Dictate and keep the meeting recording?". Escape during a meeting says "Dictation cancelled — meeting still recording. Stop meeting?". Dictation preempts the meeting's engine (batch 2). | docs:114, W:9, W:10; `desktop/src-tauri/src/lib.rs:4128-4136`, `:4151-4170` `hands_free_meeting_ask_due`; `desktop/src-tauri/src/tray_menu.rs:553-554` | PARITY (both dictate mid-meeting and keep the meeting recording; Yap's one question is asked while the take is already live, so it never delays a word) |
| 13 | Hide for 1 hour, and getting the bar back | Right-click → Hide for 1 hour. **A notification with Undo appears (at most twice per user, about 6 s).** Undo ends that snooze. | Right-click or tray → Hide Pill for 1 Hour. The pill disappears **silently**: there is no toast and no Undo. The only way back is the tray's "Show Pill Now", or a restart. | docs:111-113; `desktop/src-tauri/src/lib.rs:9043-9049` `hide_for_an_hour`; `desktop/src-tauri/src/tray_menu.rs:537-554` | **GAP → G1** |
| 14 | Stuck key / a hold that never releases | Automatic recovery after roughly 10-15 s; it never cuts active dictation short. | A second stop path (a 30 ms poll) reconciles a missed release. A hold with no key-up for 90 s puts "Still recording" on the pill, naming the bound key. | docs:72-77; `desktop/src-tauri/src/ptt_macos.rs:710-713` | PARITY |
| 15 | Mouse buttons as push-to-talk | Middle, Mouse 4/5 (Mac 6-10); left and right click can never be assigned; an assigned button never reaches other apps. | Hold or hands-free mode on any non-primary button. The button is swallowed while it is bound. | docs:50-52; `desktop/src-tauri/src/ptt_macos.rs:386`, `:263-281`, `:424` | PARITY |
| 16 | Esc while dragging the bar | Esc cancels the drag (audit D-BAR). | A fresh Esc press mid-drag restores the old dock. | `desktop/src-tauri/src/float_pill.rs:1328-1341` | PARITY |

## Gaps filed

Three new gaps went to `~/code/wilson-voice-loop/loop3/add-wispr-gaps-2026-11-07.json` (a copy is kept in this folder as `add-wispr-gaps-2026-11-07.json`):

- **G1 `yap-1107-hide-pill-undo-toast`**: hiding the pill shows a short toast with Undo (row 13).
- **G2 `yap-1107-triple-tap-cancels-hands-free`**: a third tap right after a double-tap cancels instead of stopping, and the double-tap window is checked against Wispr's 500 ms (rows 3-4).
- **G3 `yap-1107-pill-arming-feedback`**: the pill acknowledges an fn press in its first frame, not 280 ms later (row 11). Its first step is a live check, because Wispr's `ready` meaning comes from bundle research and not from a capture.

Rows 1 and 2 are gaps that already have items, so they are not filed again.

## Live walk (optional, for a seat that may launch apps)

No row depends on this any more. It would add 2x recordings next to the verdicts; if a recording ever contradicts a verdict, the recording wins and the row is re-filed.

Run on Yap Test and Wispr Flow side by side. Check the frontmost app with `osascript -e 'tell application "System Events" to get name of first process whose frontmost is true'` before every synthetic key. Use one scratch TextEdit document and a local password page in headless Chrome only.

1. Rows 6, 7, 10: in TextEdit, start a take, release, and press Esc during "transcribing". Type a word and stop mid-word as the take finishes. Record what each app does (a screen recording at 2x per app, saved in this folder).
2. Row 8: headless Chrome with `<input type=password>` focused through CDP. Dictate into it with each app and record whether text lands, is copied, or is held.
3. Row 12: start Wispr Notetaker and a Yap meeting, then dictate with fn in TextEdit. Record whether the take pastes and whether the meeting keeps recording.
4. Row 11 / G3: press and hold fn and capture the first 300 ms at 60 fps in each app. Does Wispr's bar change before listening starts?
5. Before and after: count History rows in a temp copy of the Yap Test data dir; they must match once the walk's takes are deleted.

## Fixtures and windows (acceptance line 5)

- **History count before = after:** no walk ran in seat B3 or seat B6. Zero takes were made, so nothing was added to any History. Wilson's data dir and Wispr's data dir were not opened. The live walk above must state its own before and after counts.
- **Chrome:** zero windows of his regular Chrome were opened. The Wispr docs were fetched with `curl` only, and seat B6 read Wispr's installed files with `strings` and a Python file reader; Wispr Flow was not launched and `~/Library/Application Support/Wispr Flow` was not opened. No browser of any kind was started, and no GUI app was launched.

## Which acceptance line each file proves

| File | Acceptance line |
|---|---|
| `SHEET.md` (this file): header table, sheet, parity docs, fixtures | (1), (4), (5) |
| `wispr-help-docs-2026-11-07.txt` | (1): the Wispr column's evidence (documented behaviour) |
| `wispr-bundle-excerpts-2026-11-07.txt` | (1): the Wispr column's evidence for rows 6, 8, 10, 12 (shipped code, read-only) |
| `yap-pill-idle__bottom.png`, `yap-pill-listening__bottom.png`, `yap-pill-done__bottom.png` | (2): every screenshot this sheet references exists here |
| `add-wispr-gaps-2026-11-07.json` (copy of the orchestrator file) | (3) |
