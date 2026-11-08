# yap-1107-sidebar-version-line-fits — evidence

Produced by `node scripts/sidebar-shots.mjs --dist desktop/dist --before-dist <origin-main>/desktop/dist`
(the integrator runs it; coders may not build). All PNGs are 2x (deviceScaleFactor 2).

| File | Proves acceptance line |
|---|---|
| `after-loop3-{1190x773,900x600}.png`, `after-worst40-*.png` + the script's `ok` lines | (1) the version line and the whole rail stay inside the sidebar for test versions |
| `brand-after-worst40-*.png` + the `title=` field the script prints | (2) the clipped worst case keeps the full version in its title; "v0.9.4-loop3.8" is not clipped and has no title |
| `brand-before-release-*.png` vs `brand-after-release-*.png` + the `pixels identical` line | (3) shipped "0.9.4" renders exactly as before |
| `before-*.png` / `after-*.png` for all three versions and both sizes, `brand-artifact-home.png` | (4) before (origin/main) / after captures next to the approved Home artifact crop |

Measured before the change (Departure Mono, advance 0.636 em at 0.72rem = 10.08 px, so 6.42 px a cell):
the text column starts 64 px into the 220 px rail and the brand block's content ends at 201 px, so it holds
21 cells. "v0.9.4-loop3.8 · local · private" is 32 cells (205 px), ending at about 269 px: 49 px past the
sidebar. The shipped "v0.9.4 · local · private" is 24 cells (154 px), ending at about 218 px: inside the
sidebar's border (219 px) but 11 px into its right padding. It is kept exactly as it was, per acceptance (3).


## Evidence sweep 2026-11-08

Sweep 2026-11-08T04:44:48Z, main e18a12d61c7f4916335c841093e23a0d9ae878b7: measured a byte compare (cmp) of the committed shipped-version brand crops before vs after at 900x600 and 1190x773: identical 2 of 2 (sweep-cmp-2026-11-08.txt). A flaky compare can only add false differences, never a false match, so acceptance (3) PASSES; the full-window release shots differ 2 of 2 outside the brand crop and are not part of that check.
