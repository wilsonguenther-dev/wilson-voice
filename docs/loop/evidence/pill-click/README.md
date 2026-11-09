# Every click on the Yappy pill registers — measurement note

Item yap-1107-pill-click-always-registers (Wilson 2026-11-07: "the Yappy pill doesn't always want to click").
Written by the coder from the code; the measured numbers come from the commands under "How the evidence is made".

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

## How the evidence is made (batch 2 retry)

Batch 1 bounced because acceptances 1, 4 and 5 asked the merge seat to drive the built app by hand, which a headless seat cannot. Every line below is now produced by a command that needs only the bundle the merge seat already builds, no app window and no GUI:

```
T=$(mktemp -d)
node scripts/pill-matrix.mjs --pill-click --out "$T"; echo "exit $?"
cp "$T"/rects.md "$T"/rects.json "$T"/clicks.csv "$T"/pressed-*.png "$T"/artifact-press-row.png docs/loop/evidence/pill-click/
YAP_PILL_CLICK_MODEL_CSV="$PWD/docs/loop/evidence/pill-click/take-model.csv" cargo test --manifest-path desktop/src-tauri/Cargo.toml fifty_
rm -rf "$T"
```

Pass = `--pill-click` exits 0 and `rects.md` ends "Clicks: 50/50 registered ... Findings: 0", and both `fifty_` tests pass.

| File | Acceptance | What it shows |
|------|------------|---------------|
| `rects.md`, `rects.json` | 1 | Per dock (bottom, left, right), Yappy at rest: window frame, the DRAWN capsule (painted canvas pixels, alpha 128 or more, read at 2x), and the CLICKABLE rect the NSPanel gets (the published capsule rect through `pad_to_min` + `hitbox_cells`) before (old per-field rounding) and after, with the gap on every side in points. A side more than 1 pt off is a `rect-side-off` finding and fails the run. |
| `clicks.csv` | 2, 4 | 50 clicks at seeded points (seed 1107) inside the painted capsule: `i,x,y,click_ms,state_before,state_after,latency_ms,in_hitbox,toggles_sent,ok`. Each click must be inside the hit rects, set the pressed shell during the pointer-down itself, paint it by the next frame under 50 ms, and send exactly one `pill_click_toggle`, so idle and recording alternate. |
| `take-model.csv` | 3, 4 | The backend half: the same 50-click idea through the one take machine and the click queue against a simulated recorder (warm-up 180 ms, stop 120 ms, decoding 600 ms), clicks landing in every window. Every click is the next start or stop in order, none dropped. |
| `pressed-before__<dock>.png`, `pressed-after__<dock>.png` | 5 | The pill at 2x as a press painted it before this item (nothing changed: the rest pill) and after (`&pressed=1`, the shell at 96 percent). |
| `artifact-press-row.png` | 5 | The approved pill.html motion-table row "Press anywhere on the pill — Shell scales to 96 percent while pressed", cropped at 2x, to sit beside the two captures. |

What this does NOT prove: the scene is headless Chrome, not the app's WKWebView inside the NSPanel. The panel's own click-through and first-mouse are Rust (float_pill.rs `accept_first_mouse`, `update_interactive`) and are covered by its tests. For Wilson's own check on the real app there is still the on-device harness, `swift scripts/pill-click-50.swift --rect <first entry of rects= in the "pill hitbox" log line> --front com.apple.TextEdit --log <Yap log> --seed 1107` (CGEvent clicks, frontmost-app check before each); it needs a logged-in GUI session, so no loop seat runs it.

Notes for reading the table: fb-02 pads any rect smaller than 24 pt to 24 pt (`MIN_HIT_PT`), so a capsule drawn under 24 pt would show a taller clickable rect; the scene applies the same padding. No `data-tauri-drag-region` exists in desktop/src, and `set_ignore_cursor_events` is only driven by `update_interactive`.

## Approved sentences kept

- "Press anywhere on the pill — Shell scales to 96 percent while pressed." (pill.html motion table) — built.
- "Click the resting pill to start hands-free." — unchanged, and now also works on the first click after focus moved away.
- "Holding fn still works and is still the fastest." — hotkey path untouched.
- Exactly one pill surface; no new window or overlay.
