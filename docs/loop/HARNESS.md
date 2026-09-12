# Yap CI/CD loop harness

Ported from the sibling project's loop harness on 2026-09-12. Same machinery (parts, two builder
lanes, the 3-agent cap, halt-on-null, the merge bar, Drain + Reflect); Yap's repo, Yap's work dirs,
Yap's gate.

## Layout

| Path | What it is |
| --- | --- |
| `scripts/loop/template.mjs` | THE HARNESS. Prose, seats, gates, Drain, Reflect. Edit this to change *how* the loop works. Not runnable by itself. |
| `scripts/loop/items/NN-<group>.mjs` | THE WORK. Each file is data: `ITEMS.push({...})` statements. Filename order **is** execution order. Edit these to change *what* gets built. |
| `scripts/loop/build.mjs` | The stamper + validator. Reads the template and the items, writes the parts and the parent, and refuses to write anything if a single check fails. |
| `scripts/loop/generated/part-NN.mjs` | GENERATED. One part per consecutive run of whole item files. Never hand-edit. |
| `scripts/cicd-loop-all.mjs` | GENERATED. The thin parent the Workflow tool runs. Holds no prompts; runs each part with `workflow({scriptPath}, args)`. Never hand-edit. |
| `scripts/loop/ci-mode.mjs` | Measures whether GitHub Actions can actually place a job on a runner. Prints `github` or `local`. |
| `scripts/loop/status.mjs` | Upserts the live status board (`~/Obsidian/Wilson-Brain/Projects/Loop-Logs/STATUS-yap.md`), one row per item, safe from two lanes at once. |

## Build the parts

```bash
cd ~/code/wilson-voice
node scripts/loop/build.mjs                 # stamp parts + parent
node scripts/loop/build.mjs --validate-only # validate only; writes nothing
# from desktop/: npm run loop:build  /  npm run loop:validate
```

Nothing is written unless **every** check passes. What it checks, beyond the sibling harness's list:

- every item's `branch` starts with `loop/` (the repo's own history uses `feat/*` and `fix/*`, and
  Drain must be able to tell them apart);
- an item file may interpolate only the template's pinned constants — `REPO`, `LOCAL_REPO`, `APP`,
  `WORKDIR`, `WORKDIR_B`, `CARGO_TARGET_A`, `CARGO_TARGET_B`, `PREVIEW_PORT`, `PREVIEW_PORT_B`,
  `LOG`, `TELEMETRY`, `NPM_CACHE`, `STATUS_BOARD`, `STATUS`, `LOOP_LABEL`, `LOCAL_GATE_TITLE`,
  `PART`, `SECURITY_IDS`, `SECURITY_PATHS`, `SPEC_SOURCES`. Anything else is a named error instead
  of a `ReferenceError` from inside `new Function`;
- wrong-repo / wrong-stack drift: a sibling-project path, repo or fixture, a QA-ack env var, an npm
  script `desktop/package.json` does not define (`npm run lint`, `npm run typecheck`,
  `npm run dead-code`, `npm run start`), or a web-stack surface Yap does not have;
- `let CI_MODE = 'local'` (the default here is local, not github) with both gate branches present,
  and `GATE_CMDS` as the single source of the gate commands.

### The item contract

Required: `id`, `prompt`, `branch` (`loop/...`), `title`, `gated` (`null` or `'panel'`),
`preflight`, `spec`, `acceptance`. Optional: `notes`.

`preflight` is the load-bearing field: the exact commands that, if they ALL pass on unmodified
`origin/main`, mean the item is already done. The build agent runs them on a detached head before
branching; all-pass short-circuits the item to `already-done` with no branch and no PR.
`acceptance` is commands, never prose — "done" has to become evidence.

## Launch

```
Workflow {scriptPath: "/Users/wilsonguenther/code/wilson-voice/scripts/cicd-loop-all.mjs", args: {mode: "build"}}
```

- **BUILD FIRST, ALL ITEMS.** `mode: "build"` (the default) dispatches builders only: two lanes,
  each landing whatever is already gate-green, pre-flighting, building, and opening one labelled
  `loop-build` PR whose body starts with `LOOP-BUILD: unreviewed — review pass pending`. No
  reviewer, no fix agent, no merge agent, and no waiting on CI or a release.
- **THEN REVIEW, ONCE.** `args: {mode: "review"}` runs part-01 only (the pass is PR-driven, so a
  second part would re-triage the same queue): triage → two review chains under a hard cap of three
  concurrent agents → land sweeper → main verify. Add `args: {dmg: true}` to make main verify build
  the bundle once.
- Panel-gated items need `args: {panelApproved: ["ID", ...]}` or they cost nothing and are skipped.
- Every part runs in order; a part that throws is logged and the run continues. A part that HALTS
  (session/usage limit, or `agent()` resolving to `null` three times) stops the run dead.

## Resume

A halt returns `{halted: true, at, reason}` and the parent launches no further part. The worktrees
and their warm cargo caches are deliberately left standing. After the limit resets, relaunch the
same script with `resumeFromRunId` — every merged PR is a durable checkpoint, so nothing already
landed is redone, and every item's pre-flight re-proves the rest from scratch.

## The gate

Run from `<worktree>/desktop`, with that lane's `CARGO_TARGET_DIR` exported. All eight must exit 0,
run separately, every exit code read bare (never through a pipe):

| command | on main, 2026-09-12 |
| --- | --- |
| `npx tsc --noEmit` | 0, ~1s |
| `npm test` (vitest, 25 tests) | 0, ~1s |
| `npm run build` (`tsc && vite build`) | 0, ~2s |
| `cargo build -p yap-polish --release` | 0, ~37s warm |
| stage `src-tauri/binaries/yap-polish-<triple>` | 0 |
| `cargo test -p yap-polish --release` | 0, ~1s |
| `cargo clippy --all-targets --features custom-protocol` (in `src-tauri`) | 0, ~4s warm |
| `cargo test --features custom-protocol` (in `src-tauri`) | 0, ~41s warm |

Informational, **never** a gate: `cargo fmt --all -- --check` exits **1** on unmodified main (CI
runs it with `|| true`). Never reformat the tree to silence it.

Not in the gate: `npm run desktop:build` (the DMG). Minutes long plus notarization — it belongs to
a final smoke item, or to `mode: "review"` with `args: {dmg: true}`.

Two Yap-specific preconditions the harness spells out in every prompt:

1. **Stage the sidecar first.** `bundle.externalBin` makes
   `desktop/src-tauri/binaries/yap-polish-<triple>` a precondition of *every* `cargo build` of the
   app, not just `tauri build`. A missing one is an environment failure, not a code defect.
2. **Never touch the bundle identifier or the data directory.** Renaming the bundle id resets macOS
   TCC (the user loses Microphone / Accessibility / Input Monitoring); renaming the data dir orphans
   the SQLite history.

### CI mode

`node scripts/loop/ci-mode.mjs --why` prints `github` or `local` plus its evidence. Actions is
disabled account-wide by a spending limit and the newest run on this repo is weeks old, so the
expected answer is `local`: UNKNOWN resolves to local on purpose, because the local gate is the
stronger gate. In local mode the acting agent pastes the whole gate on the PR under a comment
titled exactly `Local gate (CI unavailable)`, bound to the sha it measured, then merges with
`gh pr merge -R wilsonguenther-dev/wilson-voice <n> --squash --delete-branch --admin`. Every board
row and merge comment carries `ci=local`.

## Work dirs and teardown

| | lane A | lane B | review R1 | review R2 |
| --- | --- | --- | --- | --- |
| worktree | `~/code/wilson-voice-loop/lane-a` | `lane-b` | `review-a` | `review-b` |
| `CARGO_TARGET_DIR` | `~/code/wilson-voice-loop/target-a` | `target-b` | `target-r1` | `target-r2` |
| port | 5273 | 5274 | 5281 | 5282 |

Worktrees, never clones — they share one object store. The cargo target dirs live **outside** the
worktrees so the (minutes-long, vendored-ggml) cold build is paid once in Recon and survives every
item, part and pass; each is ~1.4 GB.

**Teardown is part of the contract.** Every part but the last is stamped `KEEP_WORKTREE = true`; the
last part's Drain removes both worktrees (`git worktree remove --force` + `git worktree prune`),
deletes the `yap/recon-a` / `yap/recon-b` branches, `rm -rf`s the target dirs, kills both port
listeners and any stray `tauri dev` / `yap-polish` process, and reports the bytes reclaimed. The
review pass's Drain does the same for `review-a`/`review-b` and `target-r1`/`target-r2` and leaves
the builder lanes alone. A halted run keeps everything on purpose, for the resume.

Manual cleanup, if a run died where Drain never ran:

```bash
cd ~/code/wilson-voice && git worktree list
git worktree remove --force ~/code/wilson-voice-loop/lane-a   # and lane-b, review-a, review-b
git worktree prune && git branch -D yap/recon-a yap/recon-b
rm -rf ~/code/wilson-voice-loop/target-a ~/code/wilson-voice-loop/target-b
df -h /System/Volumes/Data
```

## Known state of the repo (2026-09-12)

- `wilsonguenther-dev/wilson-voice`, PUBLIC, default branch `main`.
- Six open PRs predate this loop (`feat/yv127`…`feat/yv134`, opened 2026-08-16). They carry **no**
  `loop-build` label, so triage and every LAND step ignore them by design — Drain is the phase that
  settles them (rebase-and-merge, or close with the evidence that supersedes them).
- No pre-commit hook: `core.hooksPath` is unset and there is no `.githooks/`. Plain `git commit`.
  Never `--no-verify`.
- `.github/workflows/ci.yml` exists and is a real gate when Actions can run, but it treats clippy
  and rustfmt as informational (`|| true`) — so even a `github`-mode merge still owes a local clippy.
