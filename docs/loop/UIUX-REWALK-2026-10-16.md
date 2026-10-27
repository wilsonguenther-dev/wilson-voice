# Yap UI/UX re-walk — 2026-10-16 pass (item yap-uiux-2026-10-16-Z)

**Status: PARTIAL. The flush this item depends on has not happened.** Two of the seven
children are merged (HOME #243, INSIGHTS #245). Five are open and review-pending:
DICT #244, MEETINGS #246, SCRATCH #267, SETTINGS #268 (CONFLICTING with main) and
ONBOARD #269. This re-walk is of `origin/main` at `90581e8` plus this branch's smoke
harness fix. It must be re-run once all five have merged. Until then the
acceptance check `! grep -q '| open |' docs/loop/UIUX-WALK-2026-10-16.md` cannot pass on
main: the 13 open rows belong to those five PRs, and each PR flips its own rows to
`fixed`. This item does not copy their fixes.

Full walk (no `YAP_SMOKE_VIEWS`), headless, `desktop/dist` built from this branch, at
980x700 and 720x520, run 2026-10-27T00:02Z:

| persona | fixture set | capture dir (PNGs, out of git) | detector result |
|---|---|---|---|
| first-run | `scripts/smoke-fixtures-first-run.json` | `~/Obsidian/Wilson-Brain/Projects/Loop-Logs/yap-uiux-2026-10-16/rewalk-first-run/2026-10-27T00-02-15-367Z/` | 18 captures, 4 with findings (exit 1): onboarding-1 x2, settings x2, all `silent-empty` |
| daily | `scripts/smoke-fixtures-daily.json` | `~/Obsidian/Wilson-Brain/Projects/Loop-Logs/yap-uiux-2026-10-16/rewalk-daily/2026-10-27T00-02-24-014Z/` | 14 captures, 4 with findings (exit 1): scratchpad x2, settings x2, all `silent-empty` |

In the PNG column below, `FR/` and `DL/` stand for those two capture dirs.

## The flushed tree, measured (a probe, not shipped)

On a local scratch branch (deleted afterwards, never pushed), I merged main with four
of the five children. Dictionary, Meetings and Onboarding merged cleanly. Scratchpad
conflicted only in the ledger doc, so I took its side. **Settings #268 conflicts in
`desktop/src/views/Permissions.tsx` and `desktop/src/views/settings/Privacy.tsx` and was
left out**, because resolving code conflicts belongs to its owner. Then I rebuilt and
ran the full smoke:

- first-run: 18 captures, 2 findings, both `settings` `silent-empty`. onboarding-1 is
  clean, because #269 renders the welcome plan as an `<ol>`.
- daily: 14 captures, 2 findings, both `settings` `silent-empty`. scratchpad is clean,
  because #267 adds `data-row` and `data-empty-state`.

The only remaining smoke failure is Settings, which is T2. #268 adds `data-row` to
fix it, but it has to be rebased onto main first. Every ledger row is flipped by its
own child branch, which I checked with `git show` on each branch's
`UIUX-WALK-2026-10-16.md`.

## QA tools (yap-qa-harness), run 2026-10-27T00:00Z on this Mac

- `bash scripts/qa/perm-probe.sh --self-test`: exit 0, `self-test: pass`.
- `bash scripts/qa/hotkey-walk.sh --self-test`: exit 0, `self-test: pass (no events posted)`.
- `bash scripts/qa/perm-probe.sh`: exit 0. It reports `app_path=/Applications/Yap.app`,
  `bundle_id=com.wilsonguenther.wilson-voice`, `version=0.9.3-dev`, `team_id=VHYV8C2JNU`,
  `same_id_bundles=` (empty), `lsregister_count=5` (3 stale build paths) and `running_pids=` (empty).
  The full hotkey walk posts fn into the installed Yap. That is Wilson's walk step, and no agent ran it.

## Per screen: before, after, the PR, and the PNG

| row | screen | persona | before (W0) | after (this re-walk, on main) | PR | PNG |
|---|---|---|---|---|---|---|
| H1 | Home | first-run | confusing | fixed: the header reads "Welcome, Smoke" | #243 (merged) | `FR/home-980x700.png` |
| H2 | Home | first-run | confusing | fixed: the empty card leads with "Grant permissions to start dictating" | #243 (merged) | `FR/home-980x700.png` |
| H3 | Home | daily | confusing | fixed: search is above the fold at 720x520 | #243 (merged) | `DL/home-720x520.png`, `DL/home-980x700.png` |
| H4–H6 | Home | daily / both | confusing / uncontained | fixed per ledger, and the detector is clean at both sizes | #243 (merged) | `DL/home-980x700.png` |
| I1 | Insights | daily | confusing | fixed per ledger, and the detector is clean (rows=9) | #245 (merged) | `DL/insights-980x700.png` |
| D1, D2 | Dictionary | both / first-run | confusing / generic | **still open on main**; the detector is clean | #244 (open, review-pending) | `FR/dictionary-980x700.png` |
| M1–M3 | Meetings | daily / first-run | confusing / generic | **still open on main**; the detector is clean | #246 (open, review-pending) | `DL/meetings-980x700.png`, `FR/meetings-980x700.png` |
| S1, S2 | Scratchpad | daily / first-run | uncontained / generic | **still open on main**; daily `silent-empty` at both sizes | #267 (open, review-pending) | `DL/scratchpad-980x700.png` |
| T1–T3 | Settings / Permissions | both / first-run | confusing / generic | **still open on main**; `silent-empty` for both personas at both sizes | #268 (open, CONFLICTING) | `FR/settings-980x700.png`, `DL/settings-720x520.png` |
| O1–O3 | Onboarding | first-run | confusing | **still open on main**: the "Preparing your speech engine…" ribbon is still inside Home's content, and the "Settings saved" toast still shows after Skip | #269 (open) | `FR/home-980x700.png`, `FR/onboarding-1-980x700.png` |

## New findings on this re-walk

| # | screen | persona | verdict | evidence | fix | status |
|---|---|---|---|---|---|---|
| Z1 | smoke harness | both | instrument defect | When `smoke-windowed.sh` cleaned up, it killed the subshell running `npm run preview`, but npm does not forward SIGTERM to its node child. vite preview kept listening on the port after every run. This lane held a 96-minute-old orphan on 5273. The next run's `--strictPort` preview failed, but the curl readiness probe succeeded against the orphan, so the walk judged a server it did not start. | This branch: exec vite directly so `$!` is vite itself, and refuse to run (exit 2) when the port is already served. I verified it: after each of the four runs, no listener was left on 5273. | fixed (this PR) |
| Z2 | shell nav | daily | uncontained | `DL/home-720x520.png`: at 720 wide the top nav cuts off at "Scra…" behind the Dictate button, so Scratchpad and Settings are not visible without scrolling the nav. The detector does not flag it, because the nav is an `overflow-x: auto` scroller. | Spans `App.tsx` and `App.css` (the shell, not one view file), so it is DEFERRED #21 | deferred |
