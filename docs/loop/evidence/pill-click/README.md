# Every click on the Yappy pill registers — measurement note

Item yap-1107-pill-click-always-registers (Wilson 2026-11-07: "the Yappy pill doesn't always want to click").
Written by the coder from the code; the numbers marked INTEGRATOR come from the built app.

## Causes found (measured from code, before -> after)

| # | Cause | Before | After | Acceptance |
|---|-------|--------|-------|------------|
| 1 | The float window had no first-mouse. The pill is a non-activating panel that is almost never key (every stop refocuses the app you type in), so the FIRST click on an inactive pill only made the window key; the page never saw it. That is the "every other take" dead click. | `WebviewWindowBuilder` without `accept_first_mouse` (float_pill.rs `ensure_float`) | `.accept_first_mouse(true)` (built here first, then landed by the dependency yap-1106-fb-02-pill-first-click; the merge keeps fb-02's line) | 1, 4 |
| 2 | Hit rect rounding. `set_hitbox` rounded x, y, w, h separately, so the right/bottom clickable edge could sit up to 1 pt INSIDE the drawn edge (drawn 10.4 + 100.4 = 110.8, clickable 10 + 100 = 110). | clickable right/bottom edge 0 to -1 pt from drawn | `hitbox_cells`: left/top floor, right/bottom ceil, so clickable covers every drawn point and overshoots by under 1 pt per side; fb-02's `store_hitboxes` now rounds its bounding box through it, and the authoritative test is fb-02's per-rect `point_in_hitboxes` on the unrounded list | 1 |
| 3 | Click-through gap. The panel ignores the mouse until the hover watch sees the cursor on the capsule; the watch napped 30 ms between looks, so a quick flick-and-click could land on the app underneath. | up to 30 ms + main-thread hop | fb-02 (the dependency) wakes the watch while the cursor is still within `NEAR_MARGIN_PT` (24 pt) of any hit rect and treats a near cursor as hovering for the cadence; this branch's own 8 ms approach ring was the same fix and was dropped in the merge in favour of fb-02's | 1, 4 |
| 4 | No pressed state. Nothing changed on screen until the backend's recording event came back (IPC + microphone open). | first visual = backend `status`/`recording` event | `data-pill-pressed` set synchronously in the pointer-down handler; CSS scales the shell to 96 percent (pill.html motion table); latency logged as `pill press event=pressed_visual latency_ms=` | 2 |
| 5 | Busy guard dropped clicks. A click while the previous take was decoding logged `action=ignored reason=busy`; a click during the stop's refocus wait was `Ignore(Stopping)`. Both were lost. | dropped | queued as the next start (`pill_click::ClickQueue`), started by a waiter the moment the finish clears; a second click cancels it so start/stop alternate exactly; expires after 10 s with `action=queued_expired` | 3 |

Clicks during warm-up (recorder arming) were already honoured by the take machine (`StopWhenArmed`); the new tests pin that at 0, 50, 150 and 300 ms.

## Not changed, on purpose

The click rule stays as approved in WF-01: under 4 pt of travel and under 300 ms is a click. A still press held 300 ms or longer is still not a click; flagged for Wilson rather than changed.

## Rects in points (INTEGRATOR)

Run the built app, hover then click the pill at rest (no card showing), and copy the `pill hitbox` line from the log. `rects=` lists every published hit rect in top-left screen points, capsule first (drag.ts `sendCapsule`); `screen_rect=` is their union. Note that fb-02 pads any rect smaller than 24 pt to 24 pt (`MIN_HIT_PT`), so a capsule drawn under 24 pt tall would show a clickable rect taller than drawn: record it if so. Compare it with the drawn capsule measured from a 2x capture (`scripts/pill-matrix.mjs`, canvas capsule bounds / 2). Fill in:

| Rect | x | y | w | h |
|------|---|---|---|---|
| Window frame (outer, pt) | | | | |
| Drawn capsule (pt) | | | | |
| Clickable (hitbox, pt) before | | | | |
| Clickable (hitbox, pt) after | | | | |

Pass = every side within 1 pt. No `data-tauri-drag-region` exists in desktop/src (grep), and `set_ignore_cursor_events` is only driven by `update_interactive`.

## Pressed latency (INTEGRATOR)

Grep the webview console log for `pill press event=pressed_visual`; every line must read `over_budget=false` (under 50 ms).

## 50 clicks (INTEGRATOR)

`swift scripts/pill-click-50.swift --rect <first entry of rects= from the log> --front com.apple.TextEdit --log <Yap log> --seed 1107`
writes `clicks.csv` here (i, x, y, click_ms, state_before, state_after, latency_ms, ok). Pass = 50/50 ok.

## Captures (INTEGRATOR)

Pressed state before/after at 2x: `float.html?...&pressed=1` through scripts/pill-matrix.mjs (after) vs the base branch (before: no pressed state), next to the pill.html crop of the motion table row "Press anywhere on the pill — Shell scales to 96 percent while pressed."

## Approved sentences kept

- "Press anywhere on the pill — Shell scales to 96 percent while pressed." (pill.html motion table) — built.
- "Click the resting pill to start hands-free." — unchanged, and now also works on the first click after focus moved away.
- "Holding fn still works and is still the fastest." — hotkey path untouched.
- Exactly one pill surface; no new window or overlay.
