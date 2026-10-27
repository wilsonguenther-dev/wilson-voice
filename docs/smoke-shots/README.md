# docs/smoke-shots/

The capture root of `scripts/smoke-windowed.sh` (`npm run smoke:windowed`).
Every run writes `docs/smoke-shots/<run>/` and points `LATEST` at it. Runs are
**gitignored**: they are evidence of one run on one machine, and committing
them on every run would grow the repo without bound.

One run holds two fixture states and the pill matrix:

- `seeded-<view>-<W>x<H>.png` — `scripts/smoke-fixtures-daily.json` (history,
  a dictionary entry, a scratchpad note, meetings)
- `empty-<view>-<W>x<H>.png` — `scripts/smoke-fixtures.json` (nothing yet)
- `<view>` is each of the seven Nav views plus `settings-<tab>` for all eight
  SettingsTabs; sizes are 980x700 and 720x520
- `pill/` — every LivePhase x `pill_position` bottom/left/right x both
  characters (`scripts/pill-matrix.mjs`), with `contact-sheet.png`
- `result-seeded.json`, `result-empty.json`, `pill/result.json` — what
  `--check-only` re-asserts

The ONE committed reference run is `docs/loop/smoke-reference/`; the baseline
and what each structural condition means are in `docs/loop/SMOKE-BASELINE.md`.
