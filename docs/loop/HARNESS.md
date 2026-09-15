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
  and `GATE_CMDS` as the single source of the gate commands;
- **the DRY RUN (added 2026-09-14).** Every check above is structural — a grep, a parse, a byte
  count — and all of them passed on a build whose two parts each threw
  `ReferenceError: dir is not defined` the instant the Workflow tool loaded them: zero agents ran.
  The cause was a top-level `const` template literal interpolating `${dir}`, a variable that only
  exists inside a prompt builder. A parse cannot see that; only evaluation can. So the validator
  now EXECUTES each part and the parent in-process, wrapped exactly the way the Workflow runtime
  wraps them, with `agent`, `parallel`, `pipeline`, `phase`, `log`, `workflow`, `args` and `budget`
  stubbed (`agent()` resolves to a canned `{status, verdict, blocking:[], exits:[0], text, ...}`
  behind a Proxy that answers any other property with a benign value), and `Date`/`Math` frozen so
  the run is deterministic. `fetch`, `process` and `require` are shadowed with throwing stubs, so
  the dry run touches no git, no network and no filesystem — the generated scripts reach the
  outside world only through `agent()` prompts, which are strings. Both arg shapes are exercised,
  `{}` and `{mode:'review'}`, because most of the review pass is unreachable otherwise, and a
  20,000-call cap turns a non-terminating script into a named failure. A throw is reported with the
  mapped `part-NN.mjs` line and column, three lines of source around it, and the stack.

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

Run from `<worktree>/desktop`, with that lane's `CARGO_TARGET_DIR` exported. **All twelve exit 0 —
run separately, every exit code read bare, never through a pipe.**

MEASURED, NOT ESTIMATED. Every number below is from one run in a throwaway worktree
(`~/code/wilson-voice-loop/proof`, branch `loop/gate-proof`) at `9512ebe` with a **cold pinned
`CARGO_TARGET_DIR`** and a real `npm ci` — fresh-clone semantics. The worktree and its target dir
were deleted afterwards. The previous version of this table was never measured and two rows were
false.

| command | exit | wall (cold) |
| --- | --- | --- |
| `npm ci  (108 packages, fresh)` | **0** | 1s |
| `npx tsc --noEmit` | **0** | 2s |
| `npm test  (vitest, 25 tests)` | **0** | 1s |
| `npm run build  (tsc && vite build)` | **0** | 2s |
| `cargo build -p yap-polish --release` | **0** | 33s |
| `cargo build -p yap-diarize --release` | **0** | 9s |
| `stage BOTH binaries under their target triple` | **0** | 0s |
| `cargo test -p yap-polish --release` | **0** | 1s |
| `cargo test -p yap-diarize --release  (16 tests)` | **0** | 1s |
| `cargo fmt --all -- --check` | **0** | 1s |
| `cargo clippy --all-targets --features custom-protocol  (src-tauri)` | **0** | 38s |
| `cargo test --features custom-protocol  (src-tauri, 546 lib + 45 suites)` | **0** | 480s |

Total ~9.5 minutes cold; the target dir it leaves behind measured **7.5G**. Warm, the same
twelve run in well under a minute — which is why the target dirs are pinned outside the worktrees
and Recon pays the cold cost once per lane. `~/.cargo` was already populated on this machine, so a
truly cold machine also pays the crates.io downloads and the prebuilt onnxruntime tarball that
`sherpa-onnx-sys` fetches for `yap-diarize`.

**BOTH SIDECARS, AND STAGING COMES FIRST.** `bundle.externalBin` names `binaries/yap-polish` AND
`binaries/yap-diarize`, and `tauri-build` checks every entry on EVERY `cargo build` of the app. With
only `yap-polish` staged, commands 11 and 12 exit **101**:
`resource path 'binaries/yap-diarize-aarch64-apple-darwin' doesn't exist`. Measured both ways — and
those same two commands exit **0** on a WARM target dir, because the build script does not re-run.
That false green (recorded as a pass earlier the same day) is why this table is taken cold.

**clippy carries no `-- -D warnings`.** A mechanical `cargo clippy --fix` sweep landed (`0f413a7`);
16 lints survive it that need real refactors (very-complex-type, too-many-arguments, clamp-like
pattern, const assertions in tests). A loop item owns them. Do not add the flag, and do not silence
a lint to make the gate green.

**`cargo fmt --all -- --check` IS a conjunct now.** The tree was swept (`cargo fmt --all`, 152 hunks
/ 21 files, `b4b8d06`, recorded in `.git-blame-ignore-revs`) and it exits 0 on main.

Not in the gate: `npm run desktop:build` (the DMG) — minutes long plus notarization. It belongs to
the final smoke item, or to `mode: "review"` with `args: {dmg: true}`.

Two Yap-specific preconditions the harness spells out in every prompt:

1. **Stage both sidecars first** (see above). A cargo failure naming a missing resource path is an
   environment failure, not a code defect — and never a reason to edit `tauri.conf.json`.
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
worktrees so the (~9.5-minute, vendored-ggml) cold build is paid once in Recon and survives every
item, part and pass; a cold one measured **7.5 GB** (debug + release, three crates).

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
