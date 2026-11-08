# yap-1107-meetings-row-actions-no-overlay: evidence

## The spec row this matches (docs/design/YAP-UI-SPEC-2026-11.md §3.3)

> | Normal | Groups Today / Yesterday / This week / Earlier, each row: title, time, duration, chips | Header, tabs, content |
> | Delete | Row removes; toast "Meeting deleted." + Undo for 10 s; then files go | Same |
> | Keyboard | ↑/↓ move the selection, Return opens, ⌘F search, ⌫ deletes with toast | … Escape back to list |

The approved page (design-2026-11-06/meetings.html, section B, frames m1–m3) does not draw the list row itself. What it does say is that rows and nav are calm blocks and nothing overlaps anything. Both of those rules are kept.

## Fix direction: (a)

The list row no longer has per-meeting actions. Export ▾ and Delete meeting stay in the detail header, where the split layout already shows them for the selected meeting. Delete is also on ⌫ for the selected row (spec §3.3, unchanged) and in a new right-click menu on the row. That menu has Export Markdown…, Copy as Markdown, Show in Finder and Delete meeting, and it can also be opened with the keyboard context-menu key. The only buttons left on a row are status buttons: Retry on a failed row, and Cancel notes or Write notes while notes are being written. They sit in a fixed column (`.meeting-row-status`) that has the same layout in every state. Hover changes the background colour only. The whole row is now one button you can press anywhere, not just in the corner.

The Export list (`FloatingMenu`) is rendered into `<body>` at `position: fixed`. `placeMenu()` places it from the trigger's rect so the whole menu stays inside the viewport. It closes only on an outside press, Escape or a choice, so moving the pointer from the trigger into the menu keeps it open.

## Captures (the integrator runs these; coders may not compile)

    (cd desktop && npx vite build --outDir "$SCRATCH/dist")          # or let the script build to scratch
    node scripts/meetings-row-shots.mjs --label after                 # this branch, must exit 0
    # in an origin/main worktree (the before shot; EXPECTED to exit 1 with "overlap" lines):
    node scripts/meetings-row-shots.mjs --label before --out <this dir>

| PNG (2x, 1190x773) | Acceptance line |
|---|---|
| before-idle.png, before-hover.png | 5 (before, from origin/main) |
| after-idle.png | 1, 5 |
| after-hover.png | 1, 2, 5 |
| after-selected.png | 1, 5 |
| after-row-menu.png | 3, 4, 5 |
| after-export-open.png | 3, 5 |

## Approved sentences kept

- "Two panes: list on the left (date groups, people chips, state chip), detail on the right." The split layout is unchanged.
- "each row: title, time, duration, chips". These are now never covered and never hidden.
- "Detail header: title, date, duration, tracks chip, people chips". Unchanged, and Export ▾ and Delete meeting stay there.
- "Delete | Row removes; toast … + Undo for 10 s". Every Delete path goes through `requestDelete` → `schedulePendingDelete`.
- "↑/↓ move the selection, Return opens, ⌫ deletes with toast". Unchanged. While a menu is open, the menu takes the arrow keys and Escape, so the list does not move under it.
- "nothing overlaps anything" (meetings.html §B). The capture tool checks this on every row in four states.


## Evidence sweep 2026-11-08

Sweep 2026-11-08T04:44:48Z, main e18a12d61c7f4916335c841093e23a0d9ae878b7: FAIL, nothing measured. Missing since run #4 (batch 1 #362): Before-shot from origin/main and 2x PNGs not committed to docs/loop/evidence. Blocker: this sweep ran in a code-only coder seat (loop three no-compile rule, Wilson 2026-11-06): no Yap Test build, no Vite/dist, no headless capture and no app launch were allowed, so nothing new could be captured or measured here; it needs a frontend build plus a headless capture. Follow-up item: yap-1108-evidence-meetings-row-actions-no-overlay in add-evidence-fails-2026-11-08.json.
