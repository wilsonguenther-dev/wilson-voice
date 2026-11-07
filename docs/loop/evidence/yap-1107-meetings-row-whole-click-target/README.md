# yap-1107-meetings-row-whole-click-target: evidence

Wilson, 2026-11-07 12:47Z: "I can't even click on the whole thing, I have to go in the corner."

## What changed

The list row's open button is now the card itself. `li.meeting-row` is a one-row grid with no padding; `.meeting-row-main` spans the whole grid row (`grid-column: 1 / -1`) and carries the padding inside it, so a press anywhere on the card opens the meeting. A row that has a status button (Retry, Cancel notes, Write notes) shows it in the same grid row on the right. `reserveStatusWidth` (MeetingsPanes.tsx) measures that column and the button reserves its width as right padding, so no text runs under it. The column's own box lets presses through (`pointer-events: none`), and only its buttons catch them.

States come from new tokens in tokens.css (light and dark): `--row-hover` is a subtle fill, `--row-pressed` (on `:active`) is one step deeper, and `--row-selected` (the soft action accent) plus `--row-selected-bar` (the 3 px inset bar) mark the selected or open row. Open and selected look the same. The keyboard ring is the one from a11y/focus.css, pulled inside the clipping row through `--focus-ring-offset` rather than a per-component `:focus-visible` rule. That rule from the prerequisite branch broke `focus_visible_is_styled_once_globally`, so it was removed. Transitions are 120 ms and are turned off under `prefers-reduced-motion`.

On semantics, the list is a list of buttons with `aria-current="true"` on the open one and a roving tab stop (`rowTabStop` in meetingsLayout.ts). That means the list is ONE Tab stop, the selected row, and ↑/↓ move both the selection and keyboard focus. Return opens the row through the existing `meetingsKeyAction` handler. Nothing interactive is nested inside a row button.

## Batch 3 bounce, fixed

The integrator's run failed two checks on this branch alone. C1: the 2 px-inset corner points of the first and last row in each group landed on `section.meetings-group`, because the list card (`ul.meetings-list`, 16 px radius, `overflow: hidden`) clipped those row corners, so a press in a row's corner did nothing. In the list pane the card now holds its rows as calm blocks: the list has `padding` and `gap` of `--space-1`, each row has its own `--radius-xs` (6 px) radius and no separator line, and the open button inherits the row's radius so the pressed fill and the inset focus ring follow its corners. A 2 px-inset corner point sits 5.66 px from a 6 px corner centre, so it is inside the row, and the row's corner sits well inside the card's 15 px inner radius. C4: the script read "which row has focus" after its Tab walk had (correctly) carried focus past the list; it now puts focus back on the list's one Tab stop, asserts that stop is the selected row, then walks with the arrows. The app's keyboard code was right; the check was wrong.

## Approved sentences kept

These come from spec §3.3: "each row: title, time, duration, chips" plus one state chip; groups Today / Yesterday / This week / Earlier; "↑/↓ move the selection, Return opens, ⌫ deletes with toast". From the approved page (meetings.html §B): rows are calm blocks and nothing overlaps anything. That page does not draw the list row, so the spec table gives the row anatomy. I found no conflict between the artifact and this item.

## Captures (the integrator runs these; coders may not compile)

    node scripts/meetings-row-shots.mjs --label after          # this branch: must exit 0
    # BEFORE: build origin/main's desktop to a scratch dir, then
    node scripts/meetings-row-shots.mjs --dist <origin-main-dist> --label before   # expected to exit 1

| File | Proves acceptance line |
|---|---|
| after-idle.png, after-hover.png, after-pressed.png, after-selected.png, after-focus-visible.png | (5); together with the "row states" log line, (2) |
| before-idle.png (and the other before-*) | (5) the before shot |
| script checks C1 | (1) click target rect, elementFromPoint grid, click opens that meeting |
| script checks C2 | (2) distinct, token-true states |
| script check C3 | (3) cursor pointer |
| script check C4, plus vitest MeetingsPanes.test.tsx / meetingsLayout.test.ts | (4) keyboard, one Tab stop, no nested interactive element |
