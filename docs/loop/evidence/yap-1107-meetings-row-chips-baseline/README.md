# yap-1107-meetings-row-chips-baseline — evidence

Wilson, 2026-11-07 12:47Z (Yap Test 0.9.4-loop3.8, ~1190x773): the row's "You / Call" chips wrapped under
the action bar, and the chips and NOTES READY were not on one line.

## What changed

- `desktop/src/views/Meetings.tsx` — the row is two lines (YAP-UI-SPEC-2026-11 §3.3 row anatomy):
  line 1 `.meeting-row-line1` = title (ellipsis) | time · duration (mono, muted, right);
  line 2 `.meeting-row-chips` = people chips on ONE line, then the state chip at the line's end.
- `desktop/src/meetings/MeetingsPanes.tsx` — `PeopleChips fit` measures every chip and folds what does
  not fit into a "+N" chip (`PeopleChipLine`, `moreChipLabel`: aria-label "3 more: Dana, Eli, Fay",
  tooltip = the names). The detail header keeps the wrapping chips.
- `desktop/src/meetings/meetingsLayout.ts` — `fitChipCount` (pure fold decision), `listPeople` honours
  `Meeting.people` (optional on the wire; today's backend omits it and the tracks decide).
- `desktop/src/meetings/meetings.css` — the state chip and a person chip are the same box (font-size
  `--text-xs`, line-height, padding `1px var(--space-2)`, 1 px border); semantic colours unchanged (App.css).

## Decisions (and why)

- **The notes' first line is NOT kept** (and the failure text left the row too). A third line on some
  rows made the list ragged (acceptance 4: one row height); the detail pane beside the list shows the
  notes; the failure text is the FAILED chip's tooltip and the detail's error state.
- **A transcribing row has no exception**: its real progress fills its own state chip
  (`data-meeting-progress`, role=progressbar, aria-valuetext "Transcribed 10 of 30 min"), so every row,
  including Transcribing, is the same height.
- **Line 2 centres equal boxes instead of `align-items: baseline`.** The state chip speaks the pixel
  mono (Departure Mono) and a person chip the UI sans; their ascents differ, so baseline alignment would
  step the boxes vertically. Equal boxes centred give one top AND one measured text baseline (acceptance
  1 measures rect.bottom − padding-bottom − border). This departs from the prompt's literal
  "align-items: baseline" on purpose; the measured acceptance is the binding part.
- The approved artifact folder (~/Obsidian/Wilson-Brain/Projects/Yap/design-2026-11-06/) was empty on
  this Mac when the coder looked, so the spec table §3.3 is the row anatomy used (the prompt says the page
  does not draw the row). Kept from the spec: "each row: title, time, duration, chips", "Row chip:
  Transcribing … with real progress where it exists", "never a fake percentage", and the split layout.

## PNGs (the integrator produces them — the coder may not build)

    node scripts/meetings-row-shots.mjs                       # after: exits 0 when A1-A4 and B1-B5 pass
    # before, from an origin/main worktree (expected to exit 1: the defect):
    node scripts/meetings-row-shots.mjs --label before --out <this dir>

| File | Acceptance line |
|---|---|
| after-idle.png, after-list.png | 1 (1190 baseline/one line), 2 (anatomy, idle), 3 (+N on "Board prep"), 4 |
| after-idle-900.png, after-list-900.png | 1 at 900 px, 3 at 900 px |
| after-hover.png, after-selected.png | 2 (no overlap on hover / selected) |
| before-*.png | 5 (the before shot) |
