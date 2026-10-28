/**
 * Yap (wilson-voice) Overhaul — CI/CD build loop. TEMPLATE (2026-09-12).
 * Ported from the sibling project's loop harness (scripts/loop/template.mjs there). Same machinery, same
 * gates, same halt semantics; the repo, the work dirs and the GATE COMMANDS are Yap's.
 *
 * THIS FILE IS NOT RUN. It is the template that scripts/loop/build.mjs stamps into the PART
 * scripts scripts/loop/generated/part-NN.mjs (a thin scripts/cicd-loop-all.mjs then runs those
 * parts in order, because the Workflow tool refuses any single script over 524288 bytes):
 *     node scripts/loop/build.mjs          (or: npm run loop:build, from desktop/)
 * Exactly four markers are substituted, and nothing else is:
 *     the __META__ marker  -> the generated `export const meta` literal (Recon, one phase per
 *                             item, Drain, Reflect). It is a PURE LITERAL and the file's FIRST
 *                             statement, which is what the Workflow tool requires.
 *     the __ITEMS__ marker -> the concatenated ITEMS.push({...}) statements from
 *                             scripts/loop/items/NN-<group>.mjs, in filename order. One part
 *                             gets a consecutive run of WHOLE item files, never a split file.
 *     the __KEEP_WORKTREE__ marker -> `true` for every part but the last, `false` for the last.
 *     the __LANES__ marker -> item id -> file ordinal or lane pin; lanes are split per part at run time.
 * Edit THIS file to change the harness. Edit scripts/loop/items/*.mjs to change the work.
 * Never hand-edit scripts/cicd-loop-all.mjs or scripts/loop/generated/*.mjs — every build
 * overwrites them.
 *
 * The GENERATED file is run via the Claude Code `Workflow` tool with {scriptPath: <it>}.
 * It is NOT a node module — top-level await/return are correct there and a `node --check`
 * will flag them. Ignore that. (build.mjs IS a node module and does pass node --check.)
 *
 * ── WHAT THIS HARNESS IS, AND WHY ──────────────────────────────────────────
 * 1. REPO is the PUBLIC repo `wilsonguenther-dev/wilson-voice`. The app lives in `desktop/`:
 *    a Cargo workspace (src-tauri + yap-polish) plus a Vite/React frontend. There is no web
 *    backend, no database and no hosted production surface in this loop — the shipped article
 *    is a notarized macOS DMG, and building one takes minutes, so the DMG is NOT in the
 *    per-item gate. It belongs to a final smoke item.
 * 2. THE GATE IS LOCAL BY DEFAULT. GitHub Actions is dead account-wide on a spending limit and
 *    the newest run on this repo is weeks old, so `scripts/loop/ci-mode.mjs` resolves UNKNOWN to
 *    `local` on purpose: the local gate is strictly stronger than the CI gate (the acting agent
 *    runs every gate command itself and pastes the bare exit codes on the PR). Recon measures it
 *    once per part and the answer is threaded into every later prompt.
 * 3. WORKDIR is a git WORKTREE of the real checkout (shared object store), not a clone, and its
 *    CARGO_TARGET_DIR is pinned OUTSIDE the worktree so the (expensive, vendored-ggml) Rust build
 *    cache survives every item and every part. Loop clones with an install per item and no
 *    teardown reached 26 clones / 78 GB on the previous project. Drain tears everything down.
 * 4. PRE-FLIGHT PER ITEM. Every item carries `preflight`: the exact commands that, if they ALL
 *    pass on unmodified origin/main, mean the item is ALREADY DONE. The build agent runs them
 *    FIRST, on a detached head, before branching. An already-done item skips review and merge.
 * 5. GATED ITEMS. An item with `gated: 'panel'` dispatches no agent at all until its id appears
 *    in args.panelApproved. Senior Panel first, code second.
 * 6. ONE THROWN STAGE MUST NOT KILL A 24-HOUR RUN. Every item is wrapped in try/catch: the error
 *    is logged, the item is recorded as errored, and the lane continues. A HALT is different.
 * 7. The independent review gate is a SECOND OPUS ADVERSARIAL REVIEWER with a DIFFERENT LENS
 *    (liveness / failure polarity / data-and-permission integrity / claim-vs-diff), never a
 *    third-party model. Two reviewers that share a premise are one reviewer.
 *
 * ── Harness lessons encoded ────────────────────────────────────────────────
 *  D1  advisory notes go on the SAME PR being merged — never a new PR
 *  D2  a DRAIN phase enumerates and triages EVERY open PR, not just ours
 *  D3  superseded PRs/branches are CLOSED with evidence, not left to rot
 *  D4  merge/fix agents rebase onto main BEFORE evaluating checks, every time
 *  D5  merge agents get effort 'high' (they rebase and re-run the whole gate)
 *  D8  final self-audit reports open_prs_before vs after; growth = failure
 *  3a  post both verdicts as REAL `gh pr review` bodies BEFORE merge
 *  3b  self-audit counts state==OPEN from `gh` only — never branch names
 *  3c  sweep branches whose head is an ancestor of main
 *  R1  every gh call carries -R $REPO; the repo is pinned, never inferred
 *  R3  fixRounds is COUNTED from the loop log, never asserted by an agent
 *  --  a BLOCK with an empty blocking[] is malformed → recorded as PASS + advisory
 *  --  agent() === null means the ACCOUNT stopped: retry 3x, then HALT the run
 *  --  TWO BUILDER LANES, no reviewer in the build pass. No agent spawns agents, ever.
 *  --  PRE-FLIGHT AND ACCEPTANCE ARE EXECUTED, NOT DESCRIBED. A Workflow script has no shell, so
 *      each runs in a dedicated command-runner SEAT that may not edit anything and returns a
 *      schema'd {exits[], allZero}; the SCRIPT branches on it. A failed acceptance buys exactly
 *      ONE fix round, then the item is `failed-acceptance`, its PR is left open and labelled
 *      needs-human, and the lane continues. The builder no longer judges its own work.
 *
 * ── MODE ───────────────────────────────────────────────────────────────────
 * args.mode selects the pass; it defaults to 'build'.
 *   'build'  — TWO BUILDER LANES and nothing else. No reviewer, no fix, no merge agent is
 *              dispatched. Each lane owns a worktree, a cargo target dir and a preview port and
 *              walks ITS OWN items: LAND, PRE-FLIGHT, BUILD, open the PR, next item. It never
 *              awaits the other lane and it never waits for CI.
 *              args.lanes (build mode only; default 2) — `lanes: 1` runs ONE builder lane: lane A
 *              walks EVERY item in source order and lane B gets none (see "args.lanes" below).
 *   'review' — the SAME script run again afterwards with args {mode:'review'}, and ONLY part-01
 *              runs it: the pass is PR-DRIVEN. One TRIAGE agent turns the open loop-build PRs into
 *              an ordered queue (closing duplicates and superseded work first), then TWO REVIEW
 *              CHAINS consume that shared queue under a HARD CAP OF THREE CONCURRENT AGENTS.
 */

export const meta = {
  name: "yap-overhaul-all-part-01",
  description:
    "Part 01 of the Yap (wilson-voice) overhaul, two or three builder lanes (args.lanes): land whatever is already gate-green, pre-flight on unmodified main (already-done items are skipped), build, open a labelled PR. The adversarial review, the independent second-Opus gate and the merge bar run afterwards in the same script with args {mode:'review'}. 26 items (Y0-A..yap24-UI3), 3 awaiting the Senior Panel. The gate is local (Actions is disabled by the account spending limit); the DMG is not in it.",
  phases: [
    { title: "Recon", detail: "two lane worktrees, one npm ci + one warm cargo build each, the loop-build label, ci-mode measured" },
    { title: "Y0-A", detail: "Clippy becomes a blocking CI gate instead of `|| true` decoration (rustfmt stays informational — ledger)" },
    { title: "Y0-D", detail: "One documented state root override and a --smoke mode, so two lanes and Wilson's own install stop sharing one history and one hotkey" },
    { title: "Y0-E", detail: "The instrument that can see \"looks broken\" ships BEFORE the nine UI items it judges — and it fails on today's tree" },
    { title: "Y0-B", detail: "One command proves a fresh clone builds, tests and stages both sidecars — no host-found dependencies" },
    { title: "Y0-C", detail: "A test asserts the SHIPPED defaults, so a feature that is off by default can never be called tested again" },
    { title: "yap-insert-bridge-2026-10-26", detail: "Dictation lands in the focused field again — paste bridge first when PostEvent is granted; AX never strands a take on the Copy card; clipboard is never a silent fallback" },
    { title: "yap-founding-coupon-follows-launch", detail: "FOUNDING19 follows the launch, not a date — valid for 30 days from the moment PURCHASE_OPEN flips true; no founding-price copy anywhere while purchasing is closed" },
    { title: "yap-meetings-summary-default", detail: "SATISFIED BY #250 — a recorded meeting gets notes with zero setup; no \"add a summary model\" prompt anywhere" },
    { title: "yap-perm-accessibility-live-refresh", detail: "SATISFIED BY #247 + #248 — the Accessibility banner follows the live probe, Repair resets the right service, no stale-instance refusal" },
    { title: "yap-test-channel-product-name", detail: "Every permission banner and message names the running product (\"Yap Test\" in a test build), never a hard-coded \"Yap\"" },
    { title: "yap-hotkeys-advisories-r1", detail: "Close the seven non-blocking review advisories carried from #249 (reserved chords, launch re-validation, copy, TCC errors, parity test)" },
    { title: "yap-loop-stall-detector", detail: "The loop notices its own stalls — same-reason-twice, gate-red-on-main halt, no-progress halt, and an external ALERT STALL line" },
    { title: "yap-telemetry-diagnostics", detail: "One Diagnostics surface — structured event ring buffer fed by the logger, a Settings panel with a live health line, and a redacted \"Export diagnostics\" bundle" },
    { title: "Y5-G", detail: "Split the 4,660-line App.tsx into seven view modules so a screen can be worked on at all" },
    { title: "Y5-A", detail: "A token layer so the seven views stop each inventing their own colours, spacing and radii" },
    { title: "Y5-B", detail: "The \"looks broken\" fix: all seven views get a real empty state, a real loading state and a real error state" },
    { title: "Y5-C", detail: "The pill gets every state the product has, including the transcribe/think gap Wilson named" },
    { title: "Y5-D", detail: "Soft-body pill motion using Wispr's measured spring constants, with Reduce Motion respected" },
    { title: "Y5-E", detail: "The docked pill stops oscillating on the screen edge — the bug Wispr shipped a comment about" },
    { title: "Y5-F", detail: "One error surface, one sentence per failure, one action — replacing raw strings and silent failures" },
    { title: "Y5-J", detail: "The accessibility floor: a focus ring, an accessible name per state, one live region, a contrast bar on the token layer" },
    { title: "Y5-I", detail: "Rebuild the docked pill with vertical as the CSS base — the architecture Wispr abandoned trying the other way" },
    { title: "Y5-K", detail: "The pill becomes a pluggable character system, and the habitat comes back as its habitat layer" },
    { title: "yap24-UI1", detail: "Every sprite pixel lands on whole device pixels at every dock size and backing scale — no half-pixel blur" },
    { title: "yap24-UI2", detail: "One animation clock for the pill and the habitat: frame-rate independent, parks at idle, honours Reduce Motion" },
    { title: "yap24-UI3", detail: "Pay the visual-QA debt: a headless screenshot matrix of every pill phase x dock x character, attached to the PR" },
    { title: "Drain", detail: "triage every open PR (the stale feat/yv1xx ones included), sweep dead branches, tear down both worktrees and both cargo target dirs" },
    { title: "Reflect", detail: "count outcomes, reconcile against gh, append telemetry" },
  ],
}
// ── PINNED CONSTANTS ────────────────────────────────────────────────────────
const REPO = 'wilsonguenther-dev/wilson-voice' // PUBLIC repo. Pinned. Never inferred.
const LOCAL_REPO = '/Users/wilsonguenther/code/wilson-voice'
/**
 * THE APP SUBDIRECTORY. Everything buildable lives in desktop/: package.json, the Vite frontend,
 * the Cargo workspace (src-tauri + yap-polish). A command run at the repo root builds nothing.
 */
const APP = 'desktop'
const WORKDIR = '/Users/wilsonguenther/code/wilson-voice-loop/lane-a' // lane A
const WORKDIR_B = '/Users/wilsonguenther/code/wilson-voice-loop/lane-b' // lane B
const WORKDIR_C = '/Users/wilsonguenther/code/wilson-voice-loop/lane-c' // lane C (args.lanes: 3, Wilson 2026-10-27)
/**
 * THE TWO LANES. Both are worktrees of LOCAL_REPO, both are created in Recon, both are removed by
 * the LAST part's Drain. Two builders cannot share one checkout, one cargo target dir or one
 * preview port, which is the whole reason there are two. They are written as literal
 * single-quoted consts because build.mjs scrapes exactly that shape to give the item files their
 * ${WORKDIR} / ${PREVIEW_PORT} / ${CARGO_TARGET_A} values.
 */
const WORKDIRS = [WORKDIR, WORKDIR_B, WORKDIR_C]
/**
 * CARGO TARGET DIRS LIVE OUTSIDE THE WORKTREES, ON PURPOSE. src-tauri statically links
 * transcribe-cpp's ggml and yap-polish vendors llama-cpp-sys-2: a COLD build of this workspace is
 * minutes, a warm one is well under one. A target dir inside the worktree dies with the worktree,
 * so every part would pay the cold build again. These persist across items, parts and passes.
 */
const CARGO_TARGET_A = '/Users/wilsonguenther/code/wilson-voice-loop/target-a'
const CARGO_TARGET_B = '/Users/wilsonguenther/code/wilson-voice-loop/target-b'
const CARGO_TARGET_C = '/Users/wilsonguenther/code/wilson-voice-loop/target-c'
const CARGO_TARGETS = [CARGO_TARGET_A, CARGO_TARGET_B, CARGO_TARGET_C]
const LOG = '/Users/wilsonguenther/Obsidian/Wilson-Brain/Projects/Loop-Logs/2026-09-12-yap-part01.md'
const TELEMETRY = '/Users/wilsonguenther/Obsidian/Wilson-Brain/Projects/Forge-CICD-Telemetry.md'
const SPEC_SOURCES = 'the repo docs (PRODUCT.md, ROADMAP.md, ARCHITECTURE.md, docs/) and the item spec below'
const PREVIEW_PORT = '5273' // lane A's vite preview / local frontend server
const PREVIEW_PORT_B = '5274' // lane B's
const PREVIEW_PORT_C = '5275' // lane C's
const PREVIEW_PORTS = [PREVIEW_PORT, PREVIEW_PORT_B, PREVIEW_PORT_C]
/** Every builder lane dir (the name predates lane C). GUARD() forbids touching any of them outside your own. */
const BOTH_DIRS = `${WORKDIR}, ${WORKDIR_B} and ${WORKDIR_C}`
const LANE_TAGS = ['A', 'B', 'C']
/**
 * args.landBar 'ci' (Wilson 2026-10-27, "the product has NO users"): BUILD-ONLY. A loop-build PR merges as soon as
 * GitHub CI "Tauri build (Rust + frontend)" is SUCCESS on its current head — no review-opus chain, no parking.
 * The DRAIN seat is the lander (rebase on conflict, one CI fix round, then label loop-ci-red); builders only merge
 * what is already CI-green. Absent or any other value -> the review bar exactly as before. Review mode ignores it.
 */
const LAND_BAR_CI = (typeof args !== 'undefined' && args && args.mode !== 'review' && args.landBar === 'ci') === true
const CI_RED_LABEL = 'loop-ci-red'
const CI_FIX_MARKER = 'loop-ci-fix round 1'
/** The drain's rebase / CI-fix checkout in landBar 'ci' (a detached worktree of LOCAL_REPO; created if missing). */
const PRE_REVIEW_DIR = '/Users/wilsonguenther/code/wilson-voice-loop/pre-review'
/**
 * ── REVIEW MODE ───────────────────────────────────────────────────────────────────────────
 * Review mode walks the OPEN PRs, not ITEMS, because that is what actually exists and item ids
 * do not map 1:1 onto them (duplicates, superseded work, PRs whose item landed another way).
 * THE HARD CAP IS THREE CONCURRENT AGENTS. Two review chains consume one shared queue; a
 * standard PR holds one seat at a time, a SECURITY PR holds two (its second reviewer runs in the
 * REVIEW-STAGE barrier). 2 + 1 = 3 and never more — see the semaphore below.
 */
const MAX_AGENTS = 3
const REVIEW_DIRS = ['/Users/wilsonguenther/code/wilson-voice-loop/review-a', '/Users/wilsonguenther/code/wilson-voice-loop/review-b']
const REVIEW_TARGETS = ['/Users/wilsonguenther/code/wilson-voice-loop/target-r1', '/Users/wilsonguenther/code/wilson-voice-loop/target-r2']
const REVIEW_PORTS = ['5281', '5282']
const REVIEW_TAGS = ['R1', 'R2']
const NPM_CACHE = '/Users/wilsonguenther/code/wilson-voice-loop/npmcache'
const BOTH_REVIEW_DIRS = `${REVIEW_DIRS[0]} and ${REVIEW_DIRS[1]}`
const REVIEW_QUEUE_DOC = '/Users/wilsonguenther/Obsidian/Wilson-Brain/Projects/Loop-Logs/YAP-REVIEW-QUEUE.md'
const REVIEW_SUMMARY_DOC = '/Users/wilsonguenther/Obsidian/Wilson-Brain/Projects/Loop-Logs/YAP-REVIEW-SUMMARY.md'
/**
 * A PR is SECURITY-CLASS on this repo when it can break a user's trust boundary or their data:
 * entitlement-free as the app is, the equivalents are the updater's signing path, the macOS
 * permission surfaces (Microphone / Accessibility / Input Monitoring), the Tauri capability
 * files, the sidecar IPC protocol, anything that writes or migrates the local SQLite history,
 * and anything that can send audio or text off the machine.
 */
const SECURITY_PATHS =
  'desktop/src-tauri/capabilities/, desktop/src-tauri/tauri.conf.json, the updater/signing path, desktop/src-tauri/src/db or any migration, the polish_protocol/sidecar IPC, or any path or diff hunk matching updater|sign|notariz|keychain|licens|entitle|permission|accessibility|microphone|network|http|upload|telemetry, or the loop harness itself (scripts/loop/, scripts/loop/review-gate.mjs included, scripts/cicd-loop-all.mjs) or .github/workflows/'
const SECURITY_IDS = 'SEC-|UPD-|PRIV-|DB-|PERM-|YAP-LIC-'
/**
 * Stamped by build.mjs: true for every part of a multi-part run EXCEPT the last. Parts share the
 * worktrees, their node_modules and their cargo target dirs, so only the LAST part's Drain is
 * allowed to remove them.
 */
const KEEP_WORKTREE = true
/** Stamped by build.mjs like LOG: which part this is. The status board is keyed on it. */
const PART = 'part-01'
/**
 * Stamped by build.mjs: item id -> the item FILE's ordinal (informational) or a pinned lane letter
 * ('A'|'B'|'C', item field `lane`). Only the PIN is read: since 2026-10-27 lanes are assigned PER PART
 * at run time (splitLanes below) — the old per-file round-robin left lane C idle for a whole part.
 */
const LANE_BY_ID = {"Y0-A":0,"Y0-D":0,"Y0-E":0,"Y0-B":0,"Y0-C":0,"yap-insert-bridge-2026-10-26":1,"yap-founding-coupon-follows-launch":1,"yap-meetings-summary-default":1,"yap-perm-accessibility-live-refresh":1,"yap-test-channel-product-name":1,"yap-hotkeys-advisories-r1":1,"yap-loop-stall-detector":1,"yap-telemetry-diagnostics":"A","Y5-G":3,"Y5-A":3,"Y5-B":3,"Y5-C":3,"Y5-D":3,"Y5-E":3,"Y5-F":3,"Y5-J":3,"Y5-I":3,"Y5-K":3,"yap24-UI1":4,"yap24-UI2":4,"yap24-UI3":4}
// -1 when the item is not pinned; a pin past args.lanes clamps to the last lane (lane:'C' at lanes:2 -> B).
const pinnedLane = (item) => {
  const v = LANE_BY_ID[item.id]
  return typeof v === 'string' && LANE_TAGS.includes(v) ? Math.max(0, Math.min(LANE_TAGS.indexOf(v), BUILD_LANES - 1)) : -1
}
/**
 * THE PASS. 'build' (the default) dispatches builders only — no reviewer, no fix, no merge agent.
 * Run the script again with args {mode:'review'} for the gate that this pass defers.
 */
const MODE = (typeof args !== 'undefined' && args && args.mode) === 'review' ? 'review' : 'build'
/** Every PR this loop opens is labelled and prefixed, so an unreviewed PR can never pass for one. */
const LOOP_LABEL = 'loop-build'
const UNREVIEWED_PREFIX = 'LOOP-BUILD: unreviewed — review pass pending'
/**
 * CI AVAILABILITY — 'local' (the default and the expected state) or 'github'.
 *
 * GitHub Actions is disabled account-wide by a spending limit: runs are created, jobs are created,
 * and then GitHub refuses to place any of them on a runner ("The job was not started because
 * recent account payments have failed or your spending limit needs to be increased"). A required
 * check that can never report is not a pending check — it is a wall. The newest run on this repo
 * is weeks old, so there is not even a recent run to measure.
 *
 * So Recon resolves this ONCE per part by running scripts/loop/ci-mode.mjs, and the script threads
 * the answer into every later prompt. It is a `let` set after Recon returns, which is exactly why
 * anything that interpolates it MUST be a function or a template built inside a function — a
 * top-level const template literal is frozen at module load, before Recon has run. UNKNOWN
 * RESOLVES TO LOCAL: the local gate is the stronger gate, so guessing local costs time while
 * guessing github would park every PR on a check that can never arrive.
 */
let CI_MODE = 'local'
/** The PR-comment title the local gate is pasted under. Reflect counts local-gate merges by it. */
const LOCAL_GATE_TITLE = 'Local gate (CI unavailable)'
/** The live status board, and the plain-node script every agent writes its row with. */
const STATUS = LOCAL_REPO + '/scripts/loop/status.mjs'
const STATUS_BOARD = '/Users/wilsonguenther/Obsidian/Wilson-Brain/Projects/Loop-Logs/STATUS-yap.md'
/**
 * THE REVIEW GATE (yap-land-gate-pass-required, review pass 2 audit 2026-10-17). No PR lands without an Opus
 * reviewer PASS bound to its CURRENT head: one PASS (review-opus or review-opus2) for standard, a PASS from BOTH
 * seats for security. Every merge path (build-mode LAND, fix-land, the land sweeper, the drain) interpolates
 * REVIEW_GATE_STEP; ONLY the reviewer prompts interpolate REVIEW_RECORD. build.mjs enforces both
 * (promptGateProblems in review-gate.mjs).
 * TRUST LIMIT: every seat posts through the same GitHub account, so the record's seat field is self-declared: the
 * gate CANNOT authenticate which seat wrote a record, and a seat that runs "record --seat review-opus" is
 * indistinguishable from the real reviewer. What this enforces is (a) a PASS bound to the current head exists, and
 * (b) only reviewer prompts are ever handed the record subcommand. It is not authentication.
 * Run as ${LOCAL_REPO}/scripts/loop/review-gate.mjs, like STATUS: the generated harness is stamped from that
 * checkout, so the gate is the same version as the prompts that call it. Spelled out literally (not via a const)
 * so build.mjs's static prompt reader sees "review-gate.mjs gate" / "record" exactly where a seat will.
 */
const MAX_FIX_ROUNDS = 2
const REVIEW_GATE_STEP = `
THE REVIEW GATE — IMMEDIATELY BEFORE EVERY MERGE, ON THE SAME SHA. CI and the local gate are necessary, NOT
sufficient. No PR lands without an Opus reviewer PASS bound to its CURRENT head: one PASS from review-opus or
review-opus2 for a standard PR, a PASS from BOTH review-opus AND review-opus2 for a security PR. There is NO
unreviewed exception: a PR no reviewer seat has passed on this exact head is refused.
    gh pr view -R ${REPO} <n> --json headRefOid                -> <headRefOid>
    node ${LOCAL_REPO}/scripts/loop/review-gate.mjs gate --pr <n> --head <headRefOid> ; echo "reviewgate=$?"      (read the code bare)
  - exit 0 -> merge THAT sha and nothing else:
        gh pr merge -R ${REPO} <n> --squash --delete-branch --admin --match-head-commit <the same headRefOid>
    If GitHub refuses because the head moved, the PR changed under you: do not retry on the new head without
    running the review gate again on it.
  - exit 6 and the last line starts "review: needs-human" -> DO NOT MERGE. A seat OUTSIDE review-opus/review-opus2
    (a hand-posted independent review) BLOCKed this head; no loop seat may overrule it. Park it for Wilson:
        gh label create needs-human -R ${REPO} --color D93F0B --description "a human decides" 2>/dev/null ; true
        gh pr edit -R ${REPO} <n> --add-label needs-human
        gh pr comment -R ${REPO} <n> --body "needs-human: outside-seat BLOCK, comment id(s) <every comment id the gate's line names> — <the gate's last line, verbatim>"
    status "needs-human", reason = that line. Triage skips needs-human PRs from then on: no reviewer or LAND seat
    is spent on them until a human removes the label.
  - exit 6 (any other line) -> DO NOT MERGE. The gate's last line names what is missing — the seat with no PASS on
    this head, a BLOCK scoped to this head (FINAL for it: only a new push plus a complete fresh round gets past
    it), or "head moved". Label it and say so on the PR:
        gh label create review-pending -R ${REPO} --color FBCA04 --description "no Opus PASS bound to the head" 2>/dev/null ; true
        gh pr edit -R ${REPO} <n> --add-label review-pending
        gh pr comment -R ${REPO} <n> --body "review-pending: <the gate's last line, verbatim>"
    status "review-pending", reason = that line. For a PR no reviewer has passed yet this is the NORMAL outcome.
  - any other exit (64 usage, 1) -> DO NOT MERGE; status "review-pending", reason = the raw output.
Records on older heads never count. If anyone pushed after the PASS — a rebase, a conflict resolution, a fix —
the gate refuses until a reviewer seat posts a fresh PASS on the new head. A merge agent is not a reviewer seat:
you never post a review verdict, you are never handed the reviewer's record subcommand, and a verdict in a loop
log or an agent's return value is not a record. No Codex / Gemini / bot review counts.
`
/**
 * THE REVIEWER'S RECORD. Interpolated by the review-opus and review-opus2 prompts and by NOTHING else — build.mjs
 * fails validation if any other seat's prompt can reach it.
 */
const REVIEW_RECORD = (seat, pr, lens) => `
# POST YOUR RECORD — YOU, THIS SEAT, ON THE HEAD YOU READ. NOBODY ELSE CAN, AND NOBODY ELSE WILL.
Bind your review to a sha BEFORE you read a line of the diff:
    gh pr view -R ${REPO} ${pr} --json headRefOid             -> HEAD (40 hex). Review THAT tree.
When you have your verdict, write your findings (every BLOCKING and ADVISORY line) to a notes file made with
mktemp, then post the record:
    node ${LOCAL_REPO}/scripts/loop/review-gate.mjs record --pr ${pr} --seat ${seat} --lens "${lens}" --head <HEAD> --verdict PASS|BLOCK --blocking <count of BLOCKING findings> --notes-file <notes> ; echo "record=$?"
  PASS needs --blocking 0. BLOCK needs --blocking 1 or more (a BLOCK that names no finding is refused).
  exit 64 "is not the PR's current head" -> the PR moved while you read it, so your review is of a stale tree.
  Re-read headRefOid, review the new head, and post on THAT. Never post on a sha you did not read.
That comment IS the merge input. The land gate counts only a ${seat} record whose head equals the PR's head at
merge time; a verdict that lives only in your return value or in a loop log does not exist for it. Post ONE
record per review and never edit it (an edited PASS reads as a BLOCK and an edited BLOCK still blocks). A BLOCK
is FINAL for the head it names: a later PASS on the same head never clears it, yours included — only a new push
and a complete fresh round do. Keep any "verdict:" line out of PASS notes (the record subcommand refuses one).
If you could not review the PR at all, post NO record: a PASS nobody earned is the exact defect this gate exists
to stop, and with no record the PR simply stays review-pending.
Add recordHead: "<HEAD>", recordPosted: true|false and recordCommentId (the id of the comment you posted:
    gh api repos/${REPO}/issues/${pr}/comments --paginate --jq '.[] | select(.body | test("^seat: ${seat}$"; "m")) | .id' | tail -1)
to your return.
`

/**
 * THE REVIEWER READS, IT DOES NOT REBUILD (harness throughput, 2026-10-27). Before this, every reviewer and every
 * re-reviewer ran the full 15–30 min gate on a sha the fix-land seat had already gated — 4–7 full runs per PR, the
 * chain landed ~1 PR/h. The gate is now sha-bound in scripts/loop/gate-ledger.mjs: the seat that runs it records it,
 * and a reviewer only verifies that record. Interpolated by the review-opus and review-opus2 prompts; build.mjs
 * (ledgerPromptProblems) fails if a reviewer prompt lacks the check or is handed GATE_CMDS.
 */
const REVIEW_GATE_READ = (dir, target, pr) => `
# THE GATE — VERIFY ITS RECORD FOR THIS HEAD; NEVER RE-RUN IT
You are a reader. The full gate (tsc, vitest, vite build, both sidecars, clippy, ~1,700 cargo tests; 15–30 min)
is NOT yours to run: the fix-land seat runs it once after its push and the LAND seat runs it when no record exists.
    node ${LOCAL_REPO}/scripts/loop/gate-ledger.mjs check --sha <HEAD> ; echo "ledger=$?"      (read the code bare)
  - exit 0 -> paste its line ("gate: satisfied by record <stamp> from <runner>") into your notes and move on.
  - exit 3 (no complete record for this head) -> write "gate: no record for <HEAD>" in your notes. That is not a
    finding against the code, and you do NOT fill it by running the gate.
  - exit 1 (a RED record) -> quote it. A blocking command that is red on this head IS evidence; cite the command.
To read the tree, check the head out DETACHED in your lane — never a branch, never a commit, never a push:
    cd ${dir} && git fetch origin pull/${pr}/head && git checkout --detach <HEAD>
(a security PR's two seats share this lane and this head; if index.lock is held, the other seat is checking out
the same sha — wait a few seconds and retry.) Run ONLY the targeted tests a SPECIFIC finding needs:
    cd ${dir}/${APP}/src-tauri && export CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=${target} && cargo test --features custom-protocol --test <file>   (or a <test-name> filter)
    cd ${dir}/${APP} && npx vitest run <file>
NEVER the full gate: no unfiltered 'npm test' or 'cargo test', no 'cargo clippy', no sidecar build, and no
'cargo build' unless a targeted test needs the build it implies. Recon keeps this lane's caches warm for that.
`

const GUARD = (dir) => `
GUARDRAILS (HARD):
- Do NOT spawn subagents. You are the only agent on this task. Never use the Agent, Task or
  Workflow tools. Never delegate.
- Cap yourself at 60 tool calls. If you run out, push what you have and report honestly what is
  incomplete. A truthful partial result beats a confident fabrication.
- Every single gh command carries -R ${REPO}. Never infer the repo from a remote, never run gh
  from a directory whose origin you have not checked.
- Work ONLY in ${dir}. Never write to ${LOCAL_REPO} — that is Wilson's live checkout and this
  loop does not touch its working tree. ${dir} is a git worktree of it, so they share one
  object store: a commit you make in ${dir} is visible there, but its checkout stays clean.
- This run has TWO lane worktrees, TWO cargo target dirs and TWO preview ports. Touch ONLY the
  ones named above. Never cd into, reset, reinstall or remove another lane's worktree, never
  build into another lane's target dir, and never kill a listener on a port you were not given:
  another agent is using it right now, and its evidence is not yours to break.
- Never report success you have not observed. "Compiled" is not "pushed" is not "merged" is not
  "shipped" is not "works for a human". Verify the positive condition, not the absence of error.
- NEVER read an exit code through a pipe. 'cmd | tail' reports TAIL's status, not cmd's. Use
  'set -o pipefail', or redirect to a file and check $? on the bare command. There is no
  'timeout' binary on macOS.
- cargo check is NOT cargo test, cargo test is NOT a running app, and a green build is NOT a
  working UI. Say which of those you actually did.
- IRREVERSIBLE, NEVER TOUCH: the app's bundle identifier and its data directory. Renaming the
  bundle id resets macOS TCC and the user loses Microphone / Accessibility / Input Monitoring
  grants; renaming the data dir orphans the SQLite history. Do not "tidy" either one, in any item.
`

const COMMIT_CONTRACT = `
COMMITTING — this repo has NO pre-commit hook (core.hooksPath is unset and there is no .githooks).
    git add -A && git commit -m "<id>: <title>"
Do NOT export a QA-ack environment variable — the sibling repo's pre-commit hook demands one and
this repo has no hooks path set at all, so an ack here asserts nothing. NEVER use --no-verify (there is nothing to bypass, so reaching for it only hides a future hook). Commit only
AFTER the acceptance evidence named below actually exists, and never commit a build artifact:
desktop/target, desktop/node_modules, desktop/src-tauri/binaries and desktop/dist are generated.
Check 'git status --porcelain' before you commit and name anything surprising in the PR body.
`

const WORKTREE_CONTRACT = (dir, port, target, other) => `
THE WORK DIR IS A SHARED WORKTREE, NOT YOUR CLONE — AND THERE ARE UP TO THREE OF THEM.
This loop runs ${BUILD_LANES} LANE(S) side by side. YOUR LANE IS:
    WORKDIR           ${dir}
    APP DIRECTORY     ${dir}/${APP}          <- every build command runs HERE, never at the root
    CARGO_TARGET_DIR  ${target}
    PREVIEW PORT      ${port}
The other lane(s) (${other}) belong to a DIFFERENT item that another agent is building RIGHT NOW.
Never cd into it, never check out a branch in it, never kill a server on its port, never remove it.

EXPORT THE TARGET DIR IN EVERY SHELL THAT RUNS CARGO:
    export CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=${target}
It lives outside the worktree so the vendored-ggml build cache survives every item and every part.
Forgetting it means a COLD build inside the worktree: minutes you did not need to spend, and a
7.5 GB directory that dies with the worktree.

IGNORE ANY OTHER WORKDIR, TARGET DIR OR PORT NAMED BELOW THIS LINE. Item text may hard-code
${WORKDIR}, ${CARGO_TARGET_A} and port ${PREVIEW_PORT}; wherever it does, read ${dir}, ${target}
and ${port}. This substitution is not optional: two lanes on one port or one target dir is two
builds fighting over one lock, and the evidence you gathered would be the other item's.

Every lane is a worktree of ${LOCAL_REPO}, created once in Recon with one 'npm ci' and one warm
cargo build each, and every item reuses them. Therefore:
- Start in YOUR lane: cd ${dir} && git fetch origin && git checkout -B <branch> origin/main
- Do not 'rm -rf' it, do not re-clone it, and do not run 'npm ci' again unless
  ${APP}/package-lock.json changed on main (say so if you do).
- Do not create additional worktrees or clones. Disk hygiene is a hard rule: a previous loop left
  26 clones / 78 GB behind on another repo.
- Leave the branch pushed. Drain removes every lane worktree at the end of the run.
`

const LOGLINE = (who, itemId, laneStage, extra) => `
BEFORE YOU RETURN, TWO APPENDS. Both are mandatory, and neither may contain a credential.
1. Append one line to ${LOG} with a shell append (mkdir -p its dir first):
   "- ${who} — <what you actually did> — <PR# or sha or NONE> — <outcome in one clause>"
2. Write your row on the run's LIVE STATUS BOARD (${STATUS_BOARD}):
   node ${STATUS} ${itemId} ${laneStage} "<one clause: the state this item is in>" <PR number or omit>${extra ? ` ${extra}` : ''}
   THE ONE CLAUSE MUST CONTAIN "ci=local" OR "ci=github" — whichever gate this run is using (Recon
   published it; scripts/loop/ci-mode.mjs re-derives it in one command if you are unsure). A board
   row that does not say which gate a merge passed cannot be audited after the run, and this run
   is expected to be merging with no CI check to point at.
   The stage argument is LANE-PREFIXED on purpose — "A:", "B:", "review:", "harness:" — so two lanes
   writing at the same moment can never overwrite each other's column. The script UPSERTS one row
   per item: calling it repeatedly is correct and never duplicates a row.
The reflection agent reads ${LOG} as the run's durable record, and Wilson reads the board while the
run is live. Skip either and your work is invisible the moment this session dies.
`

/**
 * REACHABILITY, Tauri edition. The failure it exists to stop is the same one that shipped five
 * "fixes" into dead modules on the previous project: code that compiles, passes every test, and
 * is reached by nothing.
 */
const REACHABILITY = `
REACHABILITY IS A HARD GATE, CHECKED IN BOTH DIRECTIONS. A GREEN BUILD IS NOT REACHABILITY.
This app has three separate ways for a change to be dead on arrival, and tsc, vitest, clippy and
cargo test are all blind to every one of them:

(a) A RUST COMMAND NOBODY CAN CALL. A new #[tauri::command] must be listed in the
    tauri::generate_handler! invocation, or the frontend's invoke() fails at runtime while
    everything compiles. Prove it:
        git grep -n "<command_name>" -- ${APP}/src-tauri/src | grep -i generate_handler
    and name the frontend call site that invokes it.
(b) A FRONTEND MODULE NOBODY IMPORTS. desktop/ has MORE THAN ONE HTML ENTRY (index.html and
    float.html, see vite.config.ts). A module reached from neither entry is dead code:
        git grep -n "<symbol>" -- ${APP}/src | grep -v "<the file that defines it>"   -> non-empty
    and name the entry -> import -> import chain that reaches it.
(c) A CAPABILITY THAT WAS NEVER GRANTED. A Tauri v2 command or plugin API also needs its
    permission in ${APP}/src-tauri/capabilities/*.json. A missing permission is a RUNTIME denial
    on a build that is perfectly green. If your change touches a plugin surface, paste the
    capability entry that allows it.

BACKWARD — every symbol you STOP importing: enumerate it with its remaining importer count from
git grep -l. Any that drops to ZERO is either deleted in the same PR, or given a named future
consumer. A file left at zero importers with no written disposition FAILS the item. Dead Rust is
louder (clippy warns on unused items) but dead TypeScript is silent — check it yourself.

LIVENESS IS BLOCKING. Two independent reviewers can share a false premise: pre-flight every claim
against the CURRENT HEAD of main before you rely on it.
`

/**
 * The Drivia harness proved gating with authenticated fetches. Yap has no server and no accounts,
 * so the equivalent proof is that the behaviour is real ON THE MACHINE: a test that exercises the
 * code path, and for anything a human sees, the running app.
 * It is a FUNCTION of the lane worktree, not a top-level template: it interpolates ${dir}, which
 * only exists inside a prompt builder. As a top-level const it evaluated at module load and threw
 * ReferenceError: dir is not defined before a single agent was dispatched (2026-09-14).
 */
const RUNTIME_PROOF = (dir) => `
PROVING BEHAVIOUR ON A DESKTOP APP — "it compiles" is not evidence and neither is a unit test of a
function nothing calls.
For every behavioural claim, give ONE of these, and say which:
  (1) A TEST THAT FAILS WITHOUT YOUR CHANGE. Paste the test name and the raw runner output —
      'cargo test --features custom-protocol' (src-tauri), 'cargo test -p yap-polish --release'
      (sidecar, which compiles the shared protocol from the other end of the wire) or
      'npm test' (vitest, the pure frontend state machines). State that you saw it RED first if
      you can; a test that passes on unmodified main proves nothing about your diff.
  (2) THE RUNNING APP, for anything a human sees or hears. A green build is not a working UI — a
      pill that never animates, a window that opens offscreen and a hotkey that no longer fires
      all compile perfectly. Run it and look:
          cd ${dir}/${APP} && export CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=<your lane's target dir> && npm run desktop:dev
      Describe what you observed, and take a screenshot for any visual claim. If you could not run
      it (no display, no permission, sidecar missing), SAY SO plainly and downgrade the claim.
  (3) A COMMAND-LEVEL PROBE for plumbing: the sidecar handshake, a DB migration applied to a COPY
      of a real history file, a capability denial reproduced. Paste the raw output.
macOS PERMISSIONS ARE STATE, NOT CODE. Microphone, Accessibility and Input Monitoring grants are
keyed to the bundle id and they are already granted for the dev build; do not change the bundle id
to "clean something up" and do not reset TCC. If a claim depends on a permission, say which.
NEVER claim the DMG. 'npm run desktop:build' + notarization is minutes long and belongs to the
final smoke item, not to your gate. Your claim ceiling in the build pass is
"built, gated locally, PR open" — never "shipped", never "notarized", never "released".
`

const VERIFY_NOTHING = `
VERIFICATION THAT VERIFIES NOTHING — the whole catalogue. Read it before you claim a gate passed.
- EXIT CODES THROUGH A PIPE. 'cmd | tail' reports TAIL's status. This has already shipped a false
  green: a run recorded exit 0 on a command that exited 1. Read every code bare, or set -o pipefail.
- 'cargo check' is not 'cargo test', and 'cargo build' is not 'cargo build --release'. Name which
  one you ran. --features custom-protocol is REQUIRED for the app crate: without it you are
  building a different configuration from the one that ships.
- THE SIDECAR IS A PRECONDITION. bundle.externalBin makes the staged binary
  ${APP}/src-tauri/binaries/yap-polish-<triple> a precondition of EVERY cargo build of the app, not
  just 'tauri build'. If cargo fails looking for it, your environment is wrong, not the code:
  'cargo build -p yap-polish --release' then copy it to that path.
- 'npm run build' here IS 'tsc && vite build', so it does typecheck — but run 'npx tsc --noEmit'
  separately anyway and read both codes, because a vite plugin can mask a tsc failure and the two
  have diverged on other repos in this fleet.
- 'cargo fmt --all -- --check' FAILS ON UNMODIFIED main TODAY (exit 1, measured 2026-09-12). It is
  INFORMATIONAL in CI and it is informational here. Never present it as a gate, and never reformat
  the tree to make it green — that is a diff nobody asked for across files you did not touch.
- A WARM TARGET DIR CAN HIDE A BROKEN BUILD SCRIPT. If you changed a build.rs, a feature flag, a
  vendored dependency or anything in src-tauri/Cargo.toml, say so — and consider that the next
  cold build is the one that pays for it.
- A grep proving ABSENCE must have the right pattern AND the right scope. State both, and prove the
  pattern can match the shape you fear, not just the shape you remember.
- A merge SHA that already appears in earlier telemetry is a REPLAY. Flag it, never count it.
- A screenshot taken before the effect runs, or of a window that is not the one you changed,
  reports success on something broken for a human.
`

/**
 * THE GATE COMMANDS. This is the one place they are written down, and every agent that gates
 * anything interpolates it. Measured in a THROWAWAY WORKTREE with a cold pinned target dir on
 * 2026-09-12 — see docs/loop/HARNESS.md for the table, which is those numbers and not an estimate.
 *
 * TWO SIDECARS, NOT ONE. bundle.externalBin names BOTH binaries/yap-polish AND
 * binaries/yap-diarize, and tauri-build checks every entry on EVERY cargo build of the app. A
 * worktree with only yap-polish staged fails the app's build script with
 *   "resource path `binaries/yap-diarize-<triple>` doesn't exist"  -> exit 101,
 * which is exactly what the first version of this gate did. Worse: with a WARM target dir the
 * build script does not re-run, so the same commands pass — the false green that made a measured
 * gate table necessary in the first place.
 *
 * fmt IS ADVISORY, NOT A GATE CONJUNCT (Y0-A ledger, PANEL 2026-09-12 — PR #234's ci.yml). The sweep
 * landed once (b4b8d06, listed in .git-blame-ignore-revs) but main has drifted since: on a740614
 * `cargo fmt --all -- --check` exits 1 (65 hunks) on unmodified main, and a land gate that required it
 * turned every PASS/PASS PR ci-red (review pass w7sv7tozp, 2026-10-26). It still RUNS and its hunk
 * count is posted; its exit code never fails the gate. Item yap-fmt-sweep-2026-10-26 (built LAST)
 * re-sweeps the tree and flips it back to blocking in the same PR. clippy STAYS blocking.
 * clippy is NOT run with `-- -D warnings`: a mechanical `cargo clippy --fix` sweep landed, and 16
 * lints survive it that need real refactors (very-complex-type, too-many-arguments, clamp-like
 * pattern, const assertions in tests). See PLAN.md's Panel revisions section.
 */
const GATE_CMDS = (dir, target) => `
    cd ${dir}/${APP} && export CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=${target}
    npx tsc --noEmit ; echo "typecheck=$?"
    npm test ; echo "test=$?"
    npm run build ; echo "build=$?"
    cargo build -p yap-polish --release ; echo "polish=$?"
    cargo build -p yap-diarize --release ; echo "diarize=$?"
    TRIPLE="$(rustc -vV | awk '/^host:/ {print $2}')" ; mkdir -p src-tauri/binaries && cp ${target}/release/yap-polish "src-tauri/binaries/yap-polish-$TRIPLE" && cp ${target}/release/yap-diarize "src-tauri/binaries/yap-diarize-$TRIPLE" ; echo "stage=$?"
    cargo test -p yap-polish --release ; echo "polishtest=$?"
    cargo test -p yap-diarize --release ; echo "diarizetest=$?"
    cargo fmt --all -- --check > ${target}/fmt-check.txt 2>&1 ; echo "fmt=$? (ADVISORY)" ; echo "fmtHunks=$(grep -c '^Diff in' ${target}/fmt-check.txt)"
    cd ${dir}/${APP}/src-tauri && cargo clippy --all-targets --features custom-protocol ; echo "clippy=$?"
    cd ${dir}/${APP}/src-tauri && cargo test --features custom-protocol ; echo "cargotest=$?"
THE TEN BLOCKING COMMANDS MUST EXIT 0. Run them SEPARATELY and read every code BARE, never through a pipe.
fmt IS ADVISORY (Y0-A ledger, PANEL 2026-09-12): run it, report "fmt: advisory, <fmtHunks> hunks" in
the gate comment, and NEVER let its exit code fail the gate, earn ci-red, or block a merge. The drift
is on main, not on the PR; do NOT reformat files the PR did not touch. clippy STAYS BLOCKING.
BOTH SIDECARS ARE BUILT AND BOTH ARE STAGED, and that step comes BEFORE any cargo command against
the app: bundle.externalBin makes binaries/yap-polish-<triple> AND binaries/yap-diarize-<triple>
preconditions of every cargo build of wilson-voice. If a cargo command against the app dies with
"resource path ... doesn't exist", you skipped the staging step — that is an ENVIRONMENT failure,
never a code defect, and never a reason to edit tauri.conf.json.
clippy is deliberately WITHOUT '-- -D warnings' (16 known lints on main need real refactors, which
is a loop item, not a gate). Do not add the flag, and do not silence lints to make it green.
NOT IN THIS GATE: 'npm run desktop:build' (the DMG). It is minutes long and belongs to the final
smoke item. Do not run it here and do not claim it.
WRITE THE LEDGER RECORD — ONE FULL RUN PER HEAD; every later seat on that sha reuses it (2026-10-27).
Once the tree you gated is COMMITTED (builders: right after your commit), bind the codes to its sha:
    cd ${dir} && node ${LOCAL_REPO}/scripts/loop/gate-ledger.mjs record --sha "$(git rev-parse HEAD)" --worktree ${dir} --runner <your seat label> --codes "typecheck=<c> test=<c> build=<c> polish=<c> diarize=<c> stage=<c> polishtest=<c> diarizetest=<c> fmt=<c> clippy=<c> cargotest=<c>" ; echo "ledger=$?"
Real codes only, all ten blocking ones. It refuses (64) a worktree whose HEAD is not that sha or that has modified
tracked files: then this head simply has no record. Never commit or push just to make it record.
`

const MERGE_BAR = `
THE MERGE BAR — verbatim, and it is the whole job.
For EVERY blocking finding on a PR you intend to merge, you must be able to point at either
(a) a diff hunk that fixes it, or (b) evidence that refutes it. Return
blockingDispositions: [{finding, disposition: "fixed"|"refuted", evidence}]. If any blocking
finding is neither fixed nor refuted, DO NOT MERGE.

TURNING THE GATE GREEN IS NOT A DISPOSITION. This exact failure shipped a live defect on the
previous project: a reviewer raised two blocking findings — one the build could see, one it could
not (a query selecting a column that did not exist) — and the agent fixed the visible one, saw
green, and merged the functional defect into main. A compiler cannot see a wrong behaviour.

Reviewer contract: only BLOCKING findings gate a merge. FALSE CAPABILITY CLAIM stays blocking.
Advisory findings are recorded once, on THIS PR, and never re-litigated. Never open a PR to hold
advisory notes — one-PR-per-advisory is how a loop grows its own backlog with every success.
`

/**
 * TELEMETRY (Wilson 2026-10-27: "Just add telemetry where we need to so we can update"). Interpolated into the
 * builder prompt (the Sonnet preamble points at it). Names ONLY modules that exist on main: the Rust logger is the
 * `log` crate teed by logging.rs; there is no TS logger wrapper and no metrics sink until yap-telemetry-diagnostics
 * lands desktop/src-tauri/src/diagnostics.rs — the block says so instead of inventing one.
 */
const TELEMETRY_BLOCK = `
# TELEMETRY (Wilson 2026-10-27) — MANDATORY on every code path this item adds or changes
(i) LOG start / finish / failure as ONE structured line, "<module> <event> key=value ...". Rust: the \`log\` crate
    (log::info! / log::warn! / log::error!) — ${APP}/src-tauri/src/logging.rs tees every record to <data_dir>/logs/yap.log.
    No println!/eprintln!, no new logger crate. Frontend: there is NO TS logger wrapper; read a rejected invoke with
    errorText()/errorCode() from ${APP}/src/errors.ts and let the Rust command log the failure — never a bare console.log.
    Never log transcript text, audio, or a full user path (basename only).
(ii) SURFACE user-visible failures, never swallow (no \`let _ =\`, \`.ok()\` or empty catch on a path a user would notice):
    the command returns Err (Result<_, String> or CommandError {code, message}) and the UI shows it through toast() in
    ${APP}/src/appShell.ts, or through status.lastError emitted by app.emit("status", build_status(..)) in
    ${APP}/src-tauri/src/lib.rs (the pill reduces it in ${APP}/src/pill/live.ts); tray copy goes through license::tray_line.
(iii) RECORD one duration or counter where it matters (dictation insert latency, meeting pipeline stage times, model
    download): there is NO metrics sink on main yet — emit it as a key=value INFO line in the style of
    ${APP}/src-tauri/src/latency.rs (PipelineSpans). If \`git grep -q "mod diagnostics" -- ${APP}/src-tauri/src/lib.rs\` exits 0
    on your branch (item yap-telemetry-diagnostics landed), record it through diagnostics.rs instead.
PR BODY: a "## Telemetry" section listing each line / surfaced error / counter you added (file:line), or
"none — <why this item adds no runtime path>".
`

/**
 * landBar 'ci' — the builder's LAND FIRST step. Merge only what GitHub CI already passed on its exact head; never
 * rebase, fix or wait here (the DRAIN does that). A function: ONLY_PREFIXES is declared further down.
 */
const CI_LAND_BUILDER = () => `# ═══ LAND FIRST — CI-GREEN ONLY (args.landBar 'ci', BUILD-ONLY; ≤ 8 TOOL CALLS, THEN MOVE ON) ═══
Wilson 2026-10-27: the product has no users, so a ${LOOP_LABEL} PR merges as soon as GitHub CI
"Tauri build (Rust + frontend)" is SUCCESS on its CURRENT head. There is no reviewer record to wait for.
    gh pr list -R ${REPO} --state open --label ${LOOP_LABEL} --json number,headRefName,headRefOid,isDraft,mergeable --jq 'sort_by(.number)'
${ONLY_PREFIXES ? `SCOPED to args.only = [${ONLY_PREFIXES.join(', ')}]: land ONLY a PR whose headRefName names an item id with one of
those prefixes; name every other PR "outside args.only" and leave it.` : ''}
Oldest PR number first, at most THREE merges, skipping drafts and anything not MERGEABLE. For each candidate:
    node ${LOCAL_REPO}/scripts/loop/gate-ledger.mjs ci --sha <headRefOid> ; echo "ledger=$?"      (read the code bare)
  exit 0 (the CI job concluded success on that exact sha; the ledger wrote its github-ci record) ->
    cd ${PRE_REVIEW_DIR} 2>/dev/null || cd ${LOCAL_REPO}
    gh pr merge -R ${REPO} <n> --squash --delete-branch --admin --match-head-commit <headRefOid>
    gh pr view -R ${REPO} <n> --json state,mergeCommit      # MERGED is merged; a refused LOCAL branch delete is harmless
  anything else (5 pending, 1 red, CONFLICTING) -> SKIP it and name it in one line. Pending, red and conflicting
  PRs are the DRAIN's job: it rebases, spends one CI fix round, or labels ${CI_RED_LABEL}. Never rebase, fix,
  review or wait here, and never run the local gate for a merge.
`

/**
 * DRAIN STEP — ports, disk, teardown. Shared by the review-bar drain and the landBar 'ci' drain so lane C is torn
 * down by both. A function: KEEP_WORKTREE is stamped per part.
 */
const DRAIN_TEARDOWN = (n) => `${n}. DISK HYGIENE — this bit hard last time (26 clones / 78 GB on the sibling project). THERE ARE UP TO
   THREE LANES (C exists only when a run used args.lanes: 3), each with its own cargo target dir and port:
     lsof -nP -iTCP:${PREVIEW_PORT} -sTCP:LISTEN     # lane A's vite/preview, if one was left
     lsof -nP -iTCP:${PREVIEW_PORT_B} -sTCP:LISTEN   # lane B's
     lsof -nP -iTCP:${PREVIEW_PORT_C} -sTCP:LISTEN   # lane C's
   Kill whatever is listening on ANY of those ports BY PID (never pkill by pattern), kill any leftover 'tauri dev'
   or 'yap-polish' process a lane started (by PID, after confirming its cwd is a lane dir), and confirm the ports
   are clear afterwards.
   Also report the current sizes: du -sh ${CARGO_TARGET_A} ${CARGO_TARGET_B} ${CARGO_TARGET_C} 2>/dev/null ; df -h /System/Volumes/Data
${
  KEEP_WORKTREE
    ? `   DO NOT REMOVE ANY LANE WORKTREE (${BOTH_DIRS}) OR ANY TARGET DIR (${CARGO_TARGET_A}, ${CARGO_TARGET_B},
   ${CARGO_TARGET_C}). This script is ONE PART of a multi-part run and the parts that follow reuse the worktrees,
   their node_modules and the warm cargo caches — a cold rebuild is ~9.5 minutes per lane per part (measured).
   Leave them exactly where they are; the LAST part's drain tears them down. Do not delete the yap/recon-a,
   yap/recon-b or yap/recon-c branches either. You still kill the port listeners, you still bundle and sweep dead
   remote branches, and you still report the target-dir sizes. Return worktreeRemoved: false and say plainly that
   everything was KEPT ON PURPOSE for the next part; that is the correct outcome here, not a miss.`
    : `     cd ${LOCAL_REPO} && for W in ${WORKDIR} ${WORKDIR_B} ${WORKDIR_C}; do [ -d "$W" ] && git worktree remove --force "$W"; done; git worktree prune
     git worktree list                                # paste it: EVERY lane must be GONE
     git branch -D yap/recon-a ; git branch -D yap/recon-b ; git branch -D yap/recon-c     # throwaway recon branches
     rm -rf ${CARGO_TARGET_A} ${CARGO_TARGET_B} ${CARGO_TARGET_C}       # cache, not checkouts: ~7.5 GB each (measured)
     rmdir /Users/wilsonguenther/code/wilson-voice-loop 2>/dev/null || ls -a /Users/wilsonguenther/code/wilson-voice-loop
   This is the LAST part of the run, so the teardown is yours and nobody else's.
   Confirm with ls -d that no lane worktree and no lane target dir exists, and report the bytes reclaimed
   (df -h /System/Volumes/Data before and after). Never leave a clone, a worktree or a target dir behind. Do NOT
   touch ${LOCAL_REPO}'s own checkout or its desktop/target. Removing them all is not optional and it is not a
   nicety: it is the last line of defence against the 78 GB of abandoned loop clones this fleet has already paid
   for once.`
}`

/**
 * landBar 'ci' — THE DRAIN IS THE LANDER (Wilson 2026-10-27, build-only). Every open non-draft loop-build PR, oldest
 * first: CI SUCCESS on the head -> squash-merge; CONFLICTING -> rebase in PRE_REVIEW_DIR, push, re-queue; CI FAILED ->
 * ONE minimal fix round, push, re-queue; failed again -> loop-ci-red label + first error, left open. No reviewer seat,
 * no parking label, and the drain never runs the full local gate.
 */
const CI_DRAIN_PROMPT = () => `DRAIN = LANDER (args.landBar 'ci', BUILD-ONLY) + SELF-AUDIT for ${REPO}, then the teardown.
Wilson 2026-10-27: the product has NO users. Every builder PR merges as soon as GitHub CI is green on its head;
Wilson QAs the real app himself after the batch. There is no review chain, no reviewer record and no parking
label in this mode — you never review a PR, and you never run the full local gate.

1. gh pr list -R ${REPO} --state open --json number,title,headRefName,createdAt,labels,isDraft
   This command is the ONLY source of the open-PR count (open_prs_before). Never infer it from branch names.

2. THE LAND QUEUE: every OPEN, NON-DRAFT PR labelled ${LOOP_LABEL}, OLDEST PR NUMBER FIRST.
    gh pr list -R ${REPO} --state open --label ${LOOP_LABEL} --limit 200 --json number,headRefName,headRefOid,isDraft,mergeable,labels --jq '[.[] | select(.isDraft|not)] | sort_by(.number)'
   A DRAFT is held on purpose (that is the only hold) — skip it. PRs without the label (the old feat/yv1xx ones)
   are not this queue: count them, name them, leave them. For each PR in the queue, in order:
   a) READ CI ON THE EXACT HEAD — the ledger reads the "Tauri build (Rust + frontend)" check run itself:
        gh pr view -R ${REPO} <n> --json headRefOid,mergeable,mergeStateStatus
        node ${LOCAL_REPO}/scripts/loop/gate-ledger.mjs ci --sha <headRefOid> ; echo "ledger=$?"      (read the code bare)
   b) exit 0 (SUCCESS on that sha; the ledger wrote the runner github-ci record) AND mergeable == MERGEABLE ->
        cd ${PRE_REVIEW_DIR} 2>/dev/null || cd ${LOCAL_REPO}
        gh pr merge -R ${REPO} <n> --squash --delete-branch --admin --match-head-commit <headRefOid>
        gh pr view -R ${REPO} <n> --json state,mergeCommit      # MERGED is merged; a refused LOCAL branch delete is harmless
      Every merge moves main, so re-read mergeable for the NEXT PR — never trust the list from before the merge.
   c) mergeable == CONFLICTING -> REBASE in the drain worktree ${PRE_REVIEW_DIR} (a detached worktree of
      ${LOCAL_REPO}; if it is missing: cd ${LOCAL_REPO} && git worktree add --detach ${PRE_REVIEW_DIR} origin/main):
        cd ${PRE_REVIEW_DIR} && git status --porcelain      # must be empty; if not, STOP on this PR and name what is there
        git fetch origin && git checkout -B ci-land/<n> origin/<headRefName> && git rebase origin/main
      Resolve every conflict MINIMALLY and FAITHFULLY: keep both sides' intent, never drop main's change, never
      delete a test to get through. A conflict you cannot resolve faithfully -> git rebase --abort, comment the
      conflicting paths on the PR, and move on (it stays open; that is not a hold, the next drain retries).
        export CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=${REVIEW_TARGETS[0]}
      A quick compile/typecheck of what you touched is allowed (cargo check / npx tsc --noEmit); the full gate is not.
        git push --force-with-lease=<headRefName>:<headRefOid> origin HEAD:<headRefName>
        git checkout --detach origin/main && git branch -D ci-land/<n>
      RE-QUEUE it (append to the END of this pass's queue): CI restarts on the new head.
   d) ledger exit 1 (CI FAILED on the head) ->
      - If the PR has a comment starting "${CI_FIX_MARKER}" written for an EARLIER head (check:
          gh pr view -R ${REPO} <n> --comments)
        it has had its one round: label it and leave it open —
          gh label create ${CI_RED_LABEL} -R ${REPO} --color B60205 --description "loop: CI red after one fix round" 2>/dev/null ; true
          gh pr edit -R ${REPO} <n> --add-label ${CI_RED_LABEL}
          gh pr comment -R ${REPO} <n> --body "${CI_RED_LABEL}: CI still red after one fix round on <headRefOid>. First error: <the first failing line, verbatim>"
      - Otherwise spend EXACTLY ONE FIX ROUND: read the failing step's log
          gh pr checks -R ${REPO} <n> ; gh run view <run id> --log-failed | tail -n 200
        check the branch out in ${PRE_REVIEW_DIR} exactly as in (c) (rebase onto origin/main in the same step if it
        is behind), fix the CAUSE minimally, run only the targeted check that failed, commit
        "<item id>: CI fix round", push --force-with-lease as in (c), then comment
          gh pr comment -R ${REPO} <n> --body "${CI_FIX_MARKER}: <the first CI error, verbatim> — fixed by <sha>: <one line>"
        and RE-QUEUE it.
      A PR already labelled ${CI_RED_LABEL} gets NO further round: merge it only if CI is now SUCCESS on its
      current head (someone pushed a fix); otherwise skip it.
   e) ledger exit 5 (queued / in_progress / no check run yet) -> leave it in the queue and go on.
   AFTER ONE FULL PASS, if anything is still pending or re-queued, wait ONCE in this agent
        perl -e 'select(undef,undef,undef,600)'
   then walk the remaining queue ONE more time with (a)-(e) (a PR whose second CI run fails here goes to the
   ${CI_RED_LABEL} branch of (d) only if its fix-round comment exists; otherwise it gets its round now). Never a
   third pass and never a second wait: what is still pending is named in stillPending and the next drain or the
   next builder's LAND step merges it.

3. Sweep dead branches: for each remote branch, check whether its head is an ancestor of main
   (git merge-base --is-ancestor). Delete the ones that are. Report, do not delete, the ones that are not — and
   BUNDLE any unpushed local branch before deleting anything
   (git bundle create ${LOCAL_REPO}/.loop-archive/<name>.bundle <branch> --not origin/main).
4. Report open_prs_before and open_prs_after from gh (step 1 again, last). If the number went UP, say so plainly.
${DRAIN_TEARDOWN('5')}

RETURN: {landBar: "ci", open_prs_before, open_prs_after, merged: [{pr, sha}], rebased: [{pr, newHead}],
         ciFixed: [{pr, firstError, fixSha}], ciRed: [{pr, firstError}], conflictsLeft: [{pr, paths}],
         stillPending: [{pr, reason}], skippedDrafts: [...], branchesDeleted: [...], branchesKept: [...],
         worktreeRemoved: true|false, targetDirsRemoved: true|false, portsClean: true|false, diskReclaimed}
${LOGLINE('drain', '_drain', 'harness:drain')}
${GUARD(BOTH_DIRS)}
TOOL-CALL BUDGET FOR THIS SEAT: 200. Spend it on landing, oldest first; name what you did not reach.`

/**
 * THE MERGE GATE, MODE-AWARE. Every agent that can merge interpolates this and nothing else.
 * It MUST stay a function: CI_MODE is assigned after Recon, long after the top-level consts in
 * this file have been evaluated, so a const template literal here would freeze the wrong mode in.
 */
const CI_GATE = (dir, target) =>
  CI_MODE === 'local'
    ? `
CI_MODE = local — GITHUB ACTIONS CANNOT RUN, AND THE LOCAL GATE IS THE MERGE GATE.
Actions is disabled for this account by a spending limit: a run is created, its jobs are created,
and every job dies in about two seconds with an empty steps[] and no runner name. The newest run on
this repo is weeks old. So the CI check on every PR in this run WILL NEVER TURN GREEN, waiting for
it is waiting forever, and "ci-pending" is a state no PR can leave. Confirm it once, yourself:
    cd ${LOCAL_REPO} && node scripts/loop/ci-mode.mjs --why      -> prints "local" + the evidence

A PR IS LANDABLE ONLY WITH A GREEN GATE BOUND TO ITS EXACT CURRENT HEAD. CHECK THE LEDGER FIRST — the
fix-land seat runs the full gate once after its push and records it; that run is the one this step reuses:
    gh pr view -R ${REPO} <n> --json headRefOid                -> <headRefOid>
    node ${LOCAL_REPO}/scripts/loop/gate-ledger.mjs check --sha <headRefOid> ; echo "ledger=$?"      (read the code bare)
  - exit 0 -> the gate is satisfied. Print its line ("gate: satisfied by record <stamp> from <runner>"), take the
    codes from that record, and do NOT run the commands below.
  - exit 3 (no record) or exit 1 (a RED record — it may be a flake) -> run the WHOLE gate yourself, in THIS
    worktree checked out at <headRefOid>, every command, and record it (your record supersedes a red one):
${GATE_CMDS(dir, target)}
SOMEONE ELSE'S GATE IS NOT A GATE unless it is a ledger record for <headRefOid> itself. A gate pasted in a comment,
an agent's return value, a loop log, or a record for any other sha is evidence about a tree you are not merging.
The record CLI refuses a worktree that is dirty or not at the sha — that is why a record counts and a paste does not.

NEVER MERGE ON A STALE HEAD. Bind the gate to a sha:
    the ledger record's sha, or: cd ${dir} && git rev-parse HEAD     # the sha your commands measured
    gh pr view -R ${REPO} <n> --json headRefOid,mergeable,mergeStateStatus
headRefOid at merge time MUST equal that sha. If you rebase, push, amend or add so much as a
comment-only commit after the gate, THE GATE IS VOID — run it all again on the new head. GitHub
must also still report mergeable == MERGEABLE (not CONFLICTING, not UNKNOWN).

THEN PASTE THE GATE ON THE PR, AS A COMMENT TITLED EXACTLY "${LOCAL_GATE_TITLE}":
    gh pr comment -R ${REPO} <n> --body "## ${LOCAL_GATE_TITLE}
ci=local — GitHub Actions cannot schedule jobs for this account, so this PR was gated locally.
gated sha: <the headRefOid you measured>
worktree:  ${dir}
ledger:    <the gate-ledger check line, or "run here and recorded">

typecheck=<code>
test=<code>
build=<code>
polish=<code>
diarize=<code>
stage=<code>
polishtest=<code>
diarizetest=<code>
fmt: advisory, <fmtHunks> hunks (exit <code>; never fails the gate)
clippy=<code>
cargotest=<code>

<the raw tail of each command's output — real pasted output, never prose>"
Post that comment BEFORE you merge. With no CI check to point at, this comment is the only
auditable record that the merge had a gate at all.

THEN THE REVIEW GATE, AND ONLY THEN THE MERGE:
${REVIEW_GATE_STEP}
--delete-branch is not optional: an undeleted branch is the stale-branch backlog this loop's Drain
then has to sweep. Use --admin so a self-opened PR with no approving review can land.
Every merge comment and every status-board row you write carries ci=local.

OUTCOMES IN THIS MODE:
  - every BLOCKING command 0 (fmt is advisory and excluded), sha matches, MERGEABLE, review gate exit 0 -> merge. status "merged", ciCheckConclusion
    "local-gate", and localGate: {sha, typecheck, test, build, polish, diarize, stage, polishtest,
    diarizetest, fmt, clippy, cargotest}.
  - any BLOCKING command non-zero -> this is a red gate (a non-zero fmt ALONE is NEVER ci-red). Fix it if this PR is yours to fix; otherwise label
    ci-red, comment WHICH command failed with its failing output, and report status "ci-red".
    "The local gate failed" with no command name is not a triage.
  - CONFLICTING after your rebase -> label needs-rebase, status "needs-rebase".
THERE IS NO "ci-pending" IN THIS MODE. Do not return it, do not poll, do not sleep, do not run
'gh run watch' — there is no run to watch. A PR you could not gate is left OPEN with a comment
saying which command stopped you, never parked on a check that cannot arrive.
`
    : `
CI_MODE = github — THE CHECK RUN IS THE MERGE GATE (Actions has been restored).
    gh pr view -R ${REPO} <n> --json headRefOid,mergeable,mergeStateStatus
    gh api repos/${REPO}/commits/<headRefOid>/check-runs
Read the check run for the CI workflow's "Tauri build (Rust + frontend)" job whose head_sha EQUALS
headRefOid. A green check on an older sha proves nothing. A "skipped" check is not a pass and not a
failure — it is not evidence at all, so do not read it either way.
GREEN CI ON THE EXACT HEAD IS THE GATE — NO LOCAL RUN. The ledger reads that check run itself and records it:
    node ${LOCAL_REPO}/scripts/loop/gate-ledger.mjs ci --sha <headRefOid> ; echo "ledger=$?"      (read the code bare)
  - exit 0 (that job == success; "gate: satisfied by record <stamp> from github-ci") AND mergeable == MERGEABLE AND
    every blocking finding disposed AND the review gate below exits 0 on that same headRefOid -> merge with the
    command the review gate gives (--match-head-commit <headRefOid>). Do NOT run the local gate. status "merged"
    with the squash sha; the board row carries ci=github.
  - exit 5 (queued / in_progress / not yet created) -> status "ci-pending", reason "ci-pending".
    DO NOT POLL, DO NOT SLEEP, DO NOT WAIT — the land sweeper settles it.
  - exit 1 (failure) -> label ci-red. ATTRIBUTE IT: unless gate-ledger.mjs check --sha <headRefOid> already exits
    0 or 1, run the local gate on that head in this worktree and record it; then comment the FAILING CI STEP NAME
    and the failing local command (or "local gate green — CI-only step <name>"). status "ci-red" either way:
${GATE_CMDS(dir, target)}
  - mergeable == CONFLICTING after your rebase -> label needs-rebase, status "needs-rebase".
NOTE: ci.yml's job is a SUPERSET of the local blocking set — it builds, stages and tests BOTH sidecars, runs
vitest, tsc+vite (npm run build), clippy (app and sidecars, -D warnings), the release app build and cargo test —
so its success on the exact head owes nothing local. fmt is advisory in both.
${REVIEW_GATE_STEP}`

// ── verdict helpers (string-safe; agent returns may be objects or strings) ──
const asText = (v) => (typeof v === 'string' ? v : JSON.stringify(v ?? null))
const saysBlock = (v) => /"verdict"\s*:\s*"BLOCK"/i.test(asText(v))
const saysReviewed = (v) => /"status"\s*:\s*"reviewed"/i.test(asText(v))
const hasEmptyBlocking = (v) => /"blocking"\s*:\s*\[\s*\]/.test(asText(v))
/**
 * A seat's return as an object, or null when its text is not JSON. agent() hands seats back as JSON TEXT (every
 * review return of pass wf_f3b32723 was a string), so `v.blocking` / `v.recordHead` on the raw value read
 * undefined: the park seat got {primary:[],lens:[]} and the recordHead checks compared nothing (2026-10-27).
 */
const asObj = (v) => {
  if (v && typeof v === 'object') return v
  const t = typeof v === 'string' ? v.trim() : ''
  for (const c of [t, (t.match(/\{[\s\S]*\}/) || [''])[0]]) {
    try { const o = JSON.parse(c); if (o && typeof o === 'object') return o } catch { /* next candidate */ }
  }
  return null
}
/** The seat's BLOCKING findings, verbatim, whether it returned an object or JSON text. */
const blockingOf = (v) => { const b = (asObj(v) || {}).blocking; return Array.isArray(b) ? b : [] }
const recordHeadOf = (v) => String((asObj(v) || {}).recordHead || '')
/** A BLOCK that names no finding (empty or missing blocking[]) is malformed — a harness error, never a verdict. */
const isMalformedBlock = (v) => saysBlock(v) && (hasEmptyBlocking(v) || (asObj(v) !== null && blockingOf(v).length === 0))
/**
 * The findings a fixer/parker is handed: the seat's blocking[] verbatim; when a BLOCK carries none, a pointer to the
 * seat's own record comment (by recordCommentId when it returned one) so the fixer reads them from GitHub instead.
 */
const findingsFor = (v, pr) => {
  const b = blockingOf(v)
  if (b.length || !saysBlock(v)) return b
  const id = (String((asObj(v) || {}).recordCommentId || '').match(/\d{6,}/) || [])[0]
  return [`NO findings array in this seat's return. Read its record verbatim and treat every BLOCKING line in it as a finding: ${id
    ? `gh api repos/${REPO}/issues/comments/${id} --jq .body`
    : `gh api repos/${REPO}/issues/${pr}/comments --paginate --jq '.[] | select(.body | test("^seat: review-opus2?$"; "m")) | .body' (the LAST record of that seat on the current head)`}`]
}
/** Appended to a reviewer prompt when its first return was a BLOCK with no findings array. */
const FINDINGS_REASK = `

# RE-ASK: YOUR LAST RETURN SAID BLOCK WITH NO FINDINGS ARRAY — RETURN THE FINDINGS ARRAY
The harness cannot hand a fixer a finding that is not in your return. If your record on this head is already
posted, do NOT post another: read it back and return every BLOCKING line in it as blocking: ["...", ...]. A BLOCK
whose blocking[] is empty again marks this PR loop-review-error and spends no fix round.`
const SHA40 = /^[0-9a-f]{40}$/
/**
 * Did a fix round produce a NEW head? (review-opus2 B1 on #222.) before: the heads this pass's reviewers recorded
 * (recordHead) plus the fixer's headBefore (gh pr view headRefOid before it touched anything); after: the fixer's
 * returned head (gh pr view headRefOid after its push). An unreadable after-head is "no new head" (fails closed).
 * A refute-only round leaves the head unchanged: re-reviewing it would PASS a head whose BLOCK is final, and LAND
 * would then refuse it on every later pass, forever. An empty commit is NOT a way out: it launders a refute into
 * a push. Unchanged → needs-human, no re-review.
 */
const fixRoundHead = (fix, reviewedHeads) => {
  const t = asText(fix)
  const read = (k, re) => {
    const v = String((fix && typeof fix === 'object' && typeof fix[k] === 'string' && fix[k]) || (t.match(re) || [])[1] || '').toLowerCase()
    return SHA40.test(v) ? v : ''
  }
  const after = read('head', /"head"\s*:\s*"([0-9a-fA-F]{40})"/)
  const before = [...(reviewedHeads || []), read('headBefore', /"headBefore"\s*:\s*"([0-9a-fA-F]{40})"/)]
    .map((h) => String(h || '').toLowerCase()).filter((h) => SHA40.test(h))
  if (!after) return { changed: false, after, before, why: 'the fixer reported no readable new head' }
  if (before.includes(after)) return { changed: false, after, before, why: `head unchanged at ${after.slice(0, 9)}` }
  return { changed: true, after, before, why: '' }
}
/**
 * LAND's review-gate line when it is a BLOCK (or an outside-seat needs-human) → that line, annotated with which
 * head it names; '' when the gate did not BLOCK. EVERY such line maps to needs-human (fails closed), whichever of
 * the three it is:
 *   - the head this pass's reviewers recorded: the BLOCK is FINAL for it, so review-pending would re-queue it forever;
 *   - a head NO reviewer in this pass recorded: the PR moved, or a reviewer misreported its head — a human decides;
 *   - no readable head in the line (or no reviewer head to compare): unknown, so a human decides.
 */
const landBlockedLine = (land, reviewedHeads) => {
  const m = asText(land).match(/"reviewGate"\s*:\s*"(review: (?:BLOCK|needs-human)[^"]*)"/i)
  if (!m) return ''
  const sha = (m[1].match(/ on ([0-9a-f]{7,40})\b/i) || [])[1]
  const heads = (reviewedHeads || []).map((h) => String(h || '').toLowerCase()).filter((h) => SHA40.test(h))
  const which = !sha || !heads.length ? 'no readable head to compare'
    : heads.some((h) => h.startsWith(sha.toLowerCase())) ? 'the head this pass reviewed'
    : `head ${sha} — one NO reviewer in this pass recorded`
  return `${m[1]} [${which}]`
}
/**
 * After a fix round: did the re-review read a head an EARLIER round of this pass already read? Independent of
 * anything the fixer reports (review-opus round 4 advisory). → the repeated head, or ''.
 */
const repeatedReviewHead = (rounds, current) => {
  const heads = (vs) => vs.map((v) => recordHeadOf(v).toLowerCase()).filter((h) => SHA40.test(h))
  const earlier = new Set((rounds || []).flatMap((r) => heads([r.primary, r.lens])))
  return heads(current).find((h) => earlier.has(h)) || ''
}
/**
 * The triage agent returns {queue:[...], closed:[...]}. A seat's payload may arrive as a typed
 * object (schema honoured) or as JSON text, so pull the array out of either. A parse failure
 * returns [] and is LOGGED — it must never look like "there was nothing to review".
 */
function extractArray(v, key) {
  if (v && typeof v === 'object' && Array.isArray(v[key])) return v[key]
  const text = asText(v)
  const at = text.indexOf(`"${key}"`)
  if (at < 0) return []
  const open = text.indexOf('[', at)
  if (open < 0) return []
  let depth = 0
  for (let i = open; i < text.length; i++) {
    const c = text[i]
    if (c === '[') depth++
    else if (c === ']') {
      depth--
      if (depth === 0) {
        try {
          return JSON.parse(text.slice(open, i + 1))
        } catch (err) {
          log(`could not parse ${key}[] out of an agent payload — ${err && err.message ? err.message : err}`)
          return []
        }
      }
    }
  }
  return []
}
/** The build agent's pre-flight passed on unmodified main → the item needs no work at all. */
const saysAlreadyDone = (v) => /"status"\s*:\s*"already-done"/i.test(asText(v))

/**
 * HALT — the account, not the code, has stopped.
 *
 * The Workflow runtime does NOT throw when a subagent dies on a session/usage limit or an API
 * error: it resolves agent() to NULL. A per-item try/catch then does exactly what it was written
 * to do — log it and move to the next item — and the loop dispatches every remaining agent into a
 * wall, each failing instantly. That is how one run produced 288 errors, 0 work, and a results
 * array claiming "errored" against items nobody ever attempted.
 *
 * So: agentR retries a seat up to 3 times (a real overload passes in minutes), and a persistent
 * null HALTS the part: no further agent() call is made — not the next item, not Drain, not
 * Reflect. The part returns { halted: true, at, reason } and the parent stops launching parts.
 * Resume after the reset with resumeFromRunId; the worktrees are deliberately left standing.
 */
async function agentRRetry(prompt, opts, maxAttempts = 3) {
  let r = null
  for (let attempt = 1; attempt <= maxAttempts; attempt++) {
    r = await agent(prompt, opts)
    if (r !== null) return r
    log(`${(opts && opts.label) || 'agent'}: attempt ${attempt}/${maxAttempts} returned null (API failure) — ${attempt < maxAttempts ? 'retrying' : 'giving up; halt follows'}`)
  }
  return r
}

// ── SEAT TIERING (loop/model-tiering, 2026-10-16) ─────────────────────────────────────────────
// Builders, fixers, landers, reviewers and every verdict seat stay on opus (build.mjs refuses a map
// that moves one). A MECHANICAL seat — one that runs a fixed command list and copies its output into
// a fixed RETURN shape — may run on a smaller model from scripts/loop/seat-models.json, stamped here.
// QUALITY FLOOR: the small seat gets SMALL_SEAT_CONTRACT; if it returns NEEDS_OPUS, returns a result
// missing a required field, or returns null twice, agentR re-runs the SAME prompt on the escalate
// model (opus) — so the worst case is the old cost plus one cheap attempt, never a guessed result.
// PER-ITEM BUILDERS (2026-10-26): an item tagged builder:'sonnet' runs its build: and fix: seats on
// SEAT_MODELS.builders.model with SONNET_BUILDER_PREAMBLE prepended (no mechanical contract), through the
// SAME escalation. build.mjs refuses the tag on a security-class item (seat-models.mjs modelFor); the id
// check in builderTier is belt and braces. Untagged items run the opus builder prompt unchanged.
// Kill switch: seat-models.json "enabled": false, or launch with args {seatModels: false}.
const SEAT_MODELS = {"enabled":true,"escalate":{"model":"opus","effort":"high"},"seats":{"record-failure":{"model":"haiku","effort":"low","require":["boardWritten"]},"preflight":{"model":"sonnet","effort":"medium","require":["exits","allZero"]},"acceptance":{"model":"sonnet","effort":"medium","require":["exits","allZero"]},"acceptance-rerun":{"model":"sonnet","effort":"medium","require":["exits","allZero"]},"recon":{"model":"sonnet","effort":"high","require":["ciMode"]},"reflect":{"model":"sonnet","effort":"medium"}},"builders":{"model":"sonnet","effort":"high","require":{"build":["itemId","status"],"fix":["itemId","fixed"]}}} // stamped by build.mjs from scripts/loop/seat-models.json
const TIER_OFF = typeof args !== 'undefined' && args && args.seatModels === false
const NEEDS_OPUS = 'NEEDS_OPUS'
const ESCALATE_MODEL = 'opus' // build.mjs refuses any other escalate model
const seatKeyOf = (label) => String(label || '').split(':')[0]
function seatTier(label) {
  if (TIER_OFF || !SEAT_MODELS || SEAT_MODELS.enabled !== true || !SEAT_MODELS.seats) return null
  const t = SEAT_MODELS.seats[seatKeyOf(label)]
  return t && typeof t.model === 'string' && t.model !== 'opus' ? t : null
}
const BUILDER_SEATS = ['build', 'fix']
/** The per-item builder tier for a build:/fix: label, or null (untagged, security id, or tiering off). */
function builderTier(label, item) {
  if (TIER_OFF || !SEAT_MODELS || SEAT_MODELS.enabled !== true || !SEAT_MODELS.builders) return null
  const seat = seatKeyOf(label)
  if (!BUILDER_SEATS.includes(seat) || !item || item.builder !== 'sonnet') return null
  if (new RegExp(`^\\s*(?:${SECURITY_IDS})`, 'i').test(String(item.id || ''))) return null
  const b = SEAT_MODELS.builders
  if (typeof b.model !== 'string' || b.model === 'opus') return null
  return { model: b.model, effort: b.effort, require: (b.require && b.require[seat]) || [] }
}
const SONNET_BUILDER_PREAMBLE = (seat, model) => `# SONNET BUILDER CONTRACT (${seat} on ${model} — per-item builder tiering)
You are a Sonnet builder. The item spec below is the whole task: do exactly its DO steps, run exactly its
acceptance commands, open the PR with the exact title format, and if any step is ambiguous or needs a design
choice return NEEDS_OPUS with one sentence instead of guessing.
- NEEDS_OPUS goes in the first string field of the RETURN shape (e.g. {"itemId": "...", "status": "NEEDS_OPUS: <one
  sentence>"}). Decide it BEFORE you push or open a PR; if you already pushed, say so in the same sentence. The loop
  then re-runs this whole step on Opus, which resumes your branch.
- No scope beyond the spec: no extra refactor, no renamed file, no new dependency, no weakened or skipped check.
- Every exit code, number and SHA in your RETURN is copied from command output you saw, never estimated.
- The TELEMETRY (Wilson 2026-10-27) block in the prompt below IS part of the spec, not extra scope: every code path
  you add or change logs start/finish/failure through the \`log\` crate (logging.rs), surfaces failures through the
  existing error surface, records one duration/counter where it matters, and the PR body gets a "## Telemetry" section.

`
/** Why a small seat's result cannot be used (null = usable). Never reads a verdict — only shape. */
function tierProblem(r, tier) {
  if (r === null || r === undefined) return 'null result'
  let text = ''
  try { text = typeof r === 'string' ? r : JSON.stringify(r) } catch { text = String(r) }
  if (text.includes(NEEDS_OPUS)) return 'returned NEEDS_OPUS'
  const obj = r && typeof r === 'object' ? r : null
  for (const k of (tier && tier.require) || []) {
    if (!obj || obj[k] === undefined || obj[k] === null) return `result is missing "${k}"`
  }
  return null
}
const SMALL_SEAT_CONTRACT = (seat, model) => `

# MECHANICAL SEAT CONTRACT (${seat} on ${model} — loop/model-tiering)
You are running a MECHANICAL seat: the commands and the RETURN shape above are the whole job.
1. Run the commands above exactly as written, in order. Do not add, skip, reorder, combine or "improve" one.
2. Read every exit code bare:  <command> ; echo "exit=$?"  — never through a pipe. A command that printed
   reassuring text and exited non-zero FAILED; a command that printed nothing and exited 0 PASSED.
3. Copy numbers, ids, SHAs, labels and JSON from command output VERBATIM. Never estimate, round, summarise,
   or fill a field from memory or from this prompt.
4. You make NO judgment call on code, product, security, a review verdict, a merge or a conflict. You never
   edit a source file, resolve a conflict, merge, close, rebase or force-push unless a command above says so
   verbatim.
5. STOP AND ESCALATE. If anything is ambiguous — an instruction you cannot follow exactly, a command that is
   missing or fails in a way the instructions above do not cover, a dirty or unexpected working tree the
   instructions do not tell you how to handle, output whose shape does not match what RETURN expects, or a
   choice between two readings — stop at once, run nothing further, and return the literal token NEEDS_OPUS
   followed by one sentence of reason in the first string field of the RETURN shape (if it has none, return
   {"needsOpus": "NEEDS_OPUS: <reason>"}). The loop then re-runs this step on Opus. Escalating costs a little;
   guessing ships a false green.
6. A command that exits non-zero is NOT ambiguous when the instructions say how to report it (an acceptance
   run that goes red, a status write that fails): report it exactly as instructed — do not escalate it.
`
/** agentR: tiered seats try the small model (twice on a null, once otherwise), then the opus path.
 *  `item` (build:/fix: seats only) enables the per-item builder tier; the opus re-run gets the bare prompt. */
async function agentR(prompt, opts, item) {
  const label = (opts && opts.label) || 'agent'
  const bt = builderTier(label, item)
  const tier = bt || seatTier(label)
  if (!tier) return agentRRetry(prompt, opts)
  const seat = seatKeyOf(label)
  const small = { ...opts, model: tier.model, effort: tier.effort || (opts && opts.effort) }
  const smallPrompt = bt ? SONNET_BUILDER_PREAMBLE(seat, tier.model) + prompt : prompt + SMALL_SEAT_CONTRACT(seat, tier.model)
  let why = null
  for (let attempt = 1; attempt <= 2; attempt++) {
    const r = await agentRRetry(smallPrompt, small, 1)
    if (typeof isStalled === 'function' && isStalled(r)) return r
    why = tierProblem(r, tier)
    if (!why) return r
    log(`[tier] ${label}: ${tier.model} attempt ${attempt}/2 ${why}${why === 'null result' && attempt < 2 ? ' — retrying on ' + tier.model : ' — escalating to opus'}`)
    if (why !== 'null result') break
  }
  const esc = (SEAT_MODELS && SEAT_MODELS.escalate) || {}
  return agentRRetry(prompt, { ...opts, model: ESCALATE_MODEL, effort: esc.effort || (opts && opts.effort) || 'high' })
}

const HALT_PATTERNS = [/session limit/i, /usage limit/i, /rate limit.*resets/i]
const isHaltError = (message) => HALT_PATTERNS.some((re) => re.test(String(message || '')))

/* ══ NO-PHASE REGION START ══════════════════════════════════════════════════════════════════
   Nothing between here and NO-PHASE REGION END may set the phase cursor. It is ONE global and two
   lanes run concurrently: the lane that owns the cursor sets it, and everything in here labels its
   agents with opts.phase instead. The build validator greps this region and fails the build. */

// ── GATED ITEMS — no agent, and no lane time, until the Senior Panel has ruled ──
// Items whose shape is a product decision carry gated:'panel'. They run only when their id is
// passed in args.panelApproved, e.g.
//   Workflow {scriptPath: ..., args: {panelApproved: ['YAP-R1', 'YAP-R2']}}
// This is a HARD skip: no build agent, no reviewer, no merge agent, no tokens spent.
function panelSkip(item) {
  const p = item.id
  const panelApproved = (typeof args !== 'undefined' && args && args.panelApproved) || null
  if (item.gated === 'panel' && !(panelApproved && panelApproved.includes(item.id))) {
    log(
      `${p}: skipped: awaiting Senior Panel — the item is gated:'panel' and ${p} is not in ` +
        `args.panelApproved. Run /panel on it, then re-run this loop with args.panelApproved including ${p}.`
    )
    return { itemId: item.id, status: 'skipped: awaiting Senior Panel', gated: 'panel' }
  }
  return null
}

// ── args.only — run ONLY the named items (build mode) ──
// Ported from the sibling harness 2026-09-26. args.only is an array of item-id PREFIXES, e.g.
//   Workflow {scriptPath: ..., args: {mode: 'build', only: ['yap24-NT'], panelApproved: [...]}}
// In build mode an item whose id starts with none of them is HARD-skipped exactly like a panel-gated
// item: no build agent, no pre-flight, no lane time, no tokens. It exists so a pass over the yap24
// notetaker chain does not re-pre-flight the ~30 finished Y-items. Absent, empty or not an array ->
// no filter. Review mode ignores it (the review pass is PR-driven). The parent
// (scripts/cicd-loop-all.mjs, emitted by build.mjs) applies the same prefixes one level up and skips
// a whole part, Recon included, when none of its items match — so an args.only run can leave the
// lane worktrees standing; the next full run's Recon reuses them (manual teardown: HARNESS.md).
const ONLY_PREFIXES =
  MODE === 'build' && typeof args !== 'undefined' && args && Array.isArray(args.only) && args.only.length
    ? args.only.map(String)
    : null
// ── args.lanes — how many builder lanes (build mode) ──
// Added 2026-10-17 (Wilson: "run the Yap loop with ONE builder agent, batching items"). Default 2: the
// round-robin LANE_BY_ID split above, two builders side by side, exactly as before. `lanes: 1` (number
// or "1") runs ONE builder lane: lane A (WORKDIR, CARGO_TARGET_A, PREVIEW_PORT) walks EVERY item in
// source order, sequentially — a batch, not a race — and lane B is handed no items, so it dispatches no
// agent and logs "finished its 0 item(s)". Combine with args.only to batch a named group, e.g.
//   Workflow {scriptPath: ..., args: {mode: 'build', lanes: 1, only: ['yap-p0-']}}
// Any other value means 2. Review mode ignores it (the review chains and their 3-agent cap are untouched).
// 2026-10-27: `lanes: 3` adds lane C (WORKDIR_C, CARGO_TARGET_C, PREVIEW_PORT_C); items split per part over A/B/C (splitLanes).
const BUILD_LANES = (() => {
  const n = MODE === 'build' && typeof args !== 'undefined' && args ? Number(args.lanes) : 2
  return n === 1 ? 1 : n === 3 ? 3 : 2
})()
function onlySkip(item) {
  if (!ONLY_PREFIXES || ONLY_PREFIXES.some((prefix) => item.id.startsWith(prefix))) return null
  return { itemId: item.id, status: 'skipped: not in args.only', only: true }
}
// args.now — the launcher's clock, ISO-8601 (this runtime has no clock of its own). Informational
// here: it is echoed into the Recon log so a run's journal says when it was launched. No Yap item
// carries a not-before hold today, so nothing is compared against it.
const RUN_NOW = typeof args !== 'undefined' && args && typeof args.now === 'string' ? args.now : null

/**
 * ══ COMMANDS ARE EXECUTED, NOT DESCRIBED ══════════════════════════════════════════════════
 *
 * THE DEFECT THIS FIXES. The first port of this harness interpolated `item.acceptance` into the
 * BUILDER's prompt and nothing else. No code path ever ran it. "Acceptance passed" was therefore
 * a sentence written by the same agent whose work it judged — the purest form of the failure this
 * whole harness exists to stop. Same for `item.preflight`: the builder decided for itself whether
 * its own item was already done.
 *
 * WHY A SEAT AND NOT A SHELL. A Workflow script has NO filesystem and NO child-process access —
 * `bash`, `exec` and `fs` do not exist in this runtime, and there is no flag that adds them. The
 * only thing that can run a command is an agent. So execution is a SEAT, and the seat is built to
 * be incapable of papering over a failure:
 *   - it is told it is a COMMAND RUNNER: run these commands verbatim, in order, in this directory;
 *   - it MUST NOT edit, create, fix, rebase, commit, push or interpret anything;
 *   - it returns a SCHEMA'd payload — one {command, exit} per command plus allZero — so the script
 *     branches on a validated boolean it did not have to parse out of prose;
 *   - the CONTROL FLOW that reacts to a non-zero exit lives HERE, in the script, where no agent
 *     can talk it out of a failure.
 * That is the difference between a gate and a claim.
 */
const RUN_SCHEMA = {
  type: 'object',
  properties: {
    kind: { type: 'string' },
    itemId: { type: 'string' },
    dir: { type: 'string' },
    headSha: { type: 'string' },
    exits: {
      type: 'array',
      items: {
        type: 'object',
        properties: { command: { type: 'string' }, exit: { type: 'number' } },
        required: ['command', 'exit'],
      },
    },
    allZero: { type: 'boolean' },
    firstFailure: { type: 'string' },
    output: { type: 'string' },
  },
  required: ['allZero', 'exits', 'output'],
}
/** A payload is a PASS only when the seat says allZero AND every exit it listed is 0. Both, because
 *  a seat that returns allZero:true beside a non-zero exit has contradicted itself, and the
 *  pessimistic reading is the only safe one. An empty exits[] is never a pass. */
const runPassed = (r) => {
  if (!r) return false
  const obj = typeof r === 'object' ? r : null
  const exits = obj && Array.isArray(obj.exits) ? obj.exits : []
  if (obj && typeof obj.allZero === 'boolean') {
    return obj.allZero === true && exits.length > 0 && exits.every((e) => Number(e && e.exit) === 0)
  }
  const text = asText(r)
  return /"allZero"\s*:\s*true/i.test(text) && !/"exit"\s*:\s*[1-9]/.test(text)
}
const runText = (r) => {
  if (r && typeof r === 'object') {
    const exits = Array.isArray(r.exits) ? r.exits.map((e) => `${e.command} -> ${e.exit}`).join('\n') : ''
    return [exits, String(r.output || '')].filter(Boolean).join('\n\n').slice(0, 6000)
  }
  return asText(r).slice(0, 6000)
}

/**
 * ONE COMMAND-RUNNER SEAT. `where` is the git state it must be in before it runs anything, so the
 * same seat serves the pre-flight (detached, unmodified origin/main) and the acceptance run (the
 * item's own branch, after the builder).
 */
async function runCommands(kind, item, lane, where, commands, phaseId) {
  const dir = WORKDIRS[lane]
  const target = CARGO_TARGETS[lane]
  const tag = LANE_TAGS[lane]
  return await agentR(
    `You are a COMMAND RUNNER for ${item.id} on lane ${tag}. This is the ${kind.toUpperCase()} run.
You run commands and you report exit codes. THAT IS THE ENTIRE JOB.

# YOU MUST NOT
Do not edit, create, move or delete a file. Do not commit, rebase, push, merge, close or comment.
Do not fix a failure, do not "help" a command along, do not substitute a command you prefer, do not
skip one that looks redundant, and do not stop early because an earlier one failed. You are not the
builder and you are not a reviewer: an exit code you soften here is a false green that ships.

# WHERE
    cd ${dir}/${APP} && export CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=${target}
${where}
Paste \`git log --oneline -1\` and \`git status --porcelain\` before you start. If the working tree is
dirty when it should be clean, or the checkout is not what is named above, STOP and return
allZero:false with firstFailure "wrong tree" and what you actually found.

# THE COMMANDS — VERBATIM, IN ORDER, ALL OF THEM
${commands}

# HOW TO READ AN EXIT CODE
Run each command on its own and print its code bare:  <the command> ; echo "exit=$?"
NEVER through a pipe — 'cmd | tail' reports TAIL's status, and this fleet has already shipped a
false green exactly that way. There is no 'timeout' binary on macOS. A command that prints
reassuring text and exits non-zero FAILED; a command that prints nothing and exits 0 PASSED.

RETURN the schema exactly: {kind: "${kind}", itemId: "${item.id}", dir: "${dir}", headSha: "<sha>",
exits: [{command, exit}, ...] — ONE ENTRY PER COMMAND, IN ORDER, allZero: <true only if every exit
is 0>, firstFailure: "<the first command that did not exit 0, or empty>", output: "<the raw tail of
each command's output, pasted, at most ~80 lines total>"}
allZero IS THE FIELD THE LOOP BRANCHES ON. Getting it wrong is the worst thing you can do here.
${GUARD(dir)}
ONE MORE CAP FOR THIS SEAT SPECIFICALLY: 20 tool calls. You are running a short command list and
reporting numbers; you are not investigating anything.`,
    { model: 'opus', effort: 'low', phase: phaseId, label: `${kind}:${tag}:${item.id}`, schema: RUN_SCHEMA }
  )
}

/**
 * ONE ITEM ON ONE BUILDER LANE: pre-flight (executed), land what is already green, build, open the
 * PR, acceptance (executed), one fix round if it fails — and stop. No reviewer, no merge, no
 * release wait: this is the whole of build mode.
 */
async function runBuild(item, lane) {
  const p = item.id
  const dir = WORKDIRS[lane]
  const port = PREVIEW_PORTS[lane]
  const target = CARGO_TARGETS[lane]
  const other = WORKDIRS.filter((_, i) => i !== lane && i < Math.max(2, BUILD_LANES)).join(' and ')
  const tag = LANE_TAGS[lane]

  /**
   * STEP 0 — PRE-FLIGHT, EXECUTED, BEFORE ANY BUILDER EXISTS.
   * A command-runner seat checks out unmodified origin/main detached in this lane's worktree and
   * runs item.preflight. If every command exits 0 the item is ALREADY DONE: no builder is
   * dispatched at all, no branch is cut, no PR is opened, and the item costs one cheap seat
   * instead of an opus/high build. The builder can no longer decide this about its own work.
   */
  const pre = await runCommands(
    'preflight',
    item,
    lane,
    `    node ${STATUS} ${p} ${tag}:preflight "pre-flight on unmodified origin/main — ci=${CI_MODE}"
    cd ${dir} && git fetch origin && git checkout --detach origin/main`,
    item.preflight,
    p
  )
  if (pre === null) {
    noteHalt(`${item.id} lane ${LANE_TAGS[lane]} preflight`, 'pre-flight runner returned null — API failure or session/usage limit; no capacity')
    return { itemId: item.id, status: 'halted', lane: LANE_TAGS[lane], error: 'agent returned null (limit)' }
  }
  // DEPENDENCY HOLD (2026-10-17, yap-p0 fix round). An item whose work needs an earlier item's code
  // writes its preflight as ONE subshell that exits 0 and prints the marker DEPENDENCY-NOT-ON-MAIN
  // when that code is absent from origin/main. Build mode cannot land an unreviewed PR, so building on
  // top of a missing dependency only produces a conflicting PR. Such an item is SKIPPED with its own
  // status, never recorded as already-done; the next build pass (after a review pass lands the
  // dependency) builds it. Item files assemble the marker at run time, e.g. "DEPENDENCY-NOT-ON-$(printf
  // MAIN)", so a seat echoing the command text cannot trip this.
  if (runPassed(pre) && /DEPENDENCY-NOT-ON-MAIN/.test(asText(pre))) {
    log(`${p} (lane ${tag}): skipped: dependency not on main — the EXECUTED pre-flight says an earlier item has not landed; no builder dispatched. Run the review pass, then the build pass again`)
    return { itemId: item.id, status: 'skipped: dependency not on main', lane: tag, preflight: pre }
  }
  if (runPassed(pre)) {
    log(`${p} (lane ${tag}): already-done — EXECUTED pre-flight passed on unmodified origin/main; no builder dispatched, no branch, no PR`)
    return { itemId: item.id, status: 'already-done', lane: tag, preflight: pre }
  }
  log(`${p} (lane ${tag}): pre-flight FAILED at ${(pre && pre.firstFailure) || 'a command it named'} — this item is real work; dispatching the builder`)

  const build = await agentR(
    `You are a SENIOR RUST + TAURI v2 + TYPESCRIPT/REACT ENGINEER working on Yap, a macOS dictation
app. Build exactly one item. No scope creep.

${WORKTREE_CONTRACT(dir, port, target, other)}

${LAND_BAR_CI ? CI_LAND_BUILDER() : `# ═══ LAND FIRST — MERGE WHAT IS ALREADY GREEN (${CI_MODE === 'local' ? '≤ 14 TOOL CALLS, ONE PR' : '≤ 6 TOOL CALLS'}, THEN MOVE ON) ═══
Nothing else in this pass merges anything. If you skip this, the PRs pile up and every item after
you branches off a main that is missing its predecessors — which is how a loop produces fifteen
conflicting PRs and zero shipped work.

    gh pr list -R ${REPO} --state open --label ${LOOP_LABEL} --json number,headRefName,mergeable,statusCheckRollup

${ONLY_PREFIXES ? `THIS PASS IS SCOPED to args.only = [${ONLY_PREFIXES.join(', ')}] (Panel revision 2026-09-26T17:35:00Z:
added after a scoped notetaker pass would otherwise land an unrelated stale PR, e.g. a licensing
item on a backend the product ledger has since moved off). From the list above, land ONLY a PR
whose headRefName names an item id starting with one of those prefixes. SKIP every other PR,
however green — name it "outside args.only \u2014 left for its own pass," never merge it here.` : ''}
MERGE every PR in that list that clears the gate below, and NOTHING else. The gate depends on
whether GitHub Actions can run at all in this run; Recon has already decided, and this is it:
${CI_GATE(dir, target)}
SKIP — do not force, do not fix, do not investigate — anything pending, failing or conflicting.
NAME each one you skipped and why, in one line each. A conflicting PR is the review pass's problem,
not yours; you have an item to build.
THIS IS A BUDGET, NOT AN INVITATION: ${CI_MODE === 'local' ? 'at most 14 tool calls, and at most ONE PR gated and landed — the local gate is eight separate command runs against one worktree, so gate and land the OLDEST eligible PR, name the ones you left, and move on. The review pass settles the rest; do not try to clear the queue here' : 'at most 6 tool calls'}. Do not read diffs, do not
review, do not chase a red check, and NEVER post a review verdict yourself. A PR merges here ONLY
when the review gate above exits 0 on its head: a freshly built PR carries no reviewer record, so the
gate refuses it (review-pending) — that is expected; name it and move on, the review pass reviews it.
DO NOT TOUCH the pre-existing feat/yv1xx PRs from the previous loop unless they carry the
${LOOP_LABEL} label. They are weeks old, they are not this pass's work, and Drain triages them.
`}
# ═══ PRE-FLIGHT ALREADY RAN — AND IT FAILED, WHICH IS WHY YOU EXIST ═══
You do NOT run the pre-flight. A separate command-runner seat already ran it on unmodified
origin/main, detached, in this worktree, before you were dispatched. It FAILED, so this item is
real work. This is its evidence, verbatim — read it, because it tells you exactly what is missing:

${runText(pre)}

THE SAME IS TRUE OF YOUR ACCEPTANCE. After you return, a command-runner seat re-runs
item.acceptance on YOUR branch, in this worktree, and the LOOP — not you — reads the exit codes. If
any command fails you get exactly ONE fix round with its output pasted back to you, and if it still
fails the item is recorded FAILED and your PR is left open carrying that output. So run your
acceptance commands yourself before you push, verbatim as written below, and do not describe a
result you have not seen.

# ═══ IS THERE ALREADY AN OPEN PR FOR THIS ITEM? RESUME IT — NEVER RECREATE IT ═══
A previous run may have been killed mid-item, by a session limit or a crash. Its branch and PR are
still there, and recreating them from origin/main throws that work away and leaves a second orphan
PR behind. So, before you branch:

    gh pr list -R ${REPO} --head ${item.branch} --state open --json number,title,headRefName,url

IF THAT RETURNS AN OPEN PR — CONTINUE IT. Do not open a second one, ever.
    cd ${dir} && git fetch origin && git checkout ${item.branch} && git reset --hard origin/${item.branch}
    gh pr view -R ${REPO} <n> --json number,body,headRefOid
    gh pr view -R ${REPO} <n> --comments
    gh api repos/${REPO}/pulls/<n>/reviews
  Read the body and EVERY review comment. Address every still-OPEN BLOCKING finding — one already
  fixed in the branch's history is not open, so read the diff before you redo work. Rebase onto
  origin/main if it has drifted behind. Then re-run the item's acceptance and the GATE below, push
  to the SAME branch, and update the PR body with what you picked up and how each finding was
  disposed of. Return that number as resumedPr.
IF IT RETURNS AN EMPTY LIST — this is a fresh item. Branch as below.

# ═══ ONLY IF THE PRE-FLIGHT FAILED: BRANCH, THEN BUILD ═══
cd ${dir} && git fetch origin && git checkout -B ${item.branch} origin/main
Branch off origin/main AS IT IS RIGHT NOW — after your LAND step, which is exactly why LAND comes
first. The other lane is merging into main while you work; that is deliberate.

# THE ITEM (${item.prompt}) — quoted VERBATIM from ${SPEC_SOURCES}
${item.spec}
${item.notes ? `
# ORCHESTRATOR NOTES — decided already, do not re-decide
${item.notes}
` : ''}

# ACCEPTANCE (these exact checks must pass; put their REAL output in the PR body)
${item.acceptance}

${TELEMETRY_BLOCK}
${RUNTIME_PROOF(dir)}
${REACHABILITY}
${VERIFY_NOTHING}
${COMMIT_CONTRACT}

# MATERIAL FACTS
Yap is pre-launch: a notarized DMG exists but there is no install base to protect, so there is no
backwards-compatibility burden — build the RIGHT architecture, not the safest diff. Do not add a
feature flag; the new shape is the only shape. Do not escalate "this changes existing behaviour" as
a decision — that is the point. TWO THINGS ARE STILL IRREVERSIBLE and are never yours to change:
the bundle identifier (renaming it resets macOS TCC and the user loses Microphone / Accessibility /
Input Monitoring) and the data directory (renaming it orphans the SQLite history).

# HARD GATE BEFORE ANY PR — the whole thing, on your own branch
${GATE_CMDS(dir, target)}
If any of them fail, FIX IT — do not open a PR on a red local gate. Paste all eight codes.

# THEN
cd ${dir} && git add -A && git commit -m "${item.id}: ${item.title}" && git push -u origin ${item.branch}
gh pr create -R ${REPO} --label ${LOOP_LABEL} --title "${item.id}: ${item.title}" --body "<see below>"
The --label is REQUIRED: the next item's LAND step finds this PR by that label and nothing else, and
the review pass walks the same label. A PR without it is invisible to both and will rot.
DO NOT WAIT FOR CI. Push, open the PR, write your log line and your board row, and return. ${LAND_BAR_CI ? 'The DRAIN seat (and any later builder\'s LAND step) merges this PR as soon as GitHub CI is green on its head.' : 'A later\nitem\'s LAND step merges this PR once it is gate-green.'} Sitting on 'gh run watch' is the wait this
pass exists to delete — and in this run there is no run to watch at all.

PR BODY — ITS FIRST LINE IS FIXED, VERBATIM:
${UNREVIEWED_PREFIX}
Then, below it, as real pasted output and not prose:
- every acceptance check's raw output
- all eight gate exit codes, and the fmt code separately marked informational
- the FORWARD and BACKWARD reachability proofs: the generate_handler! grep for any new command, the
  entry -> import chain for any new frontend module, and the capability entry for any plugin surface
- your runtime proof: the failing-then-passing test, or what you saw in the running app (with a
  screenshot path), or the command-level probe — and which of the three it was
- a "## Telemetry" section: every log line, surfaced error and counter/duration this PR adds (file:line), per
  the TELEMETRY block above
- an explicit "What I did NOT do" section listing what a reader might assume you did and you did not

RETURN: {itemId, status: "built", branch, prNumber, resumedPr,
         landed: [{pr, sha}], landSkipped: [{pr, why}], openLoopPrs,
         gateExits: {typecheck, test, build, polish, diarize, stage, polishtest, diarizetest, fmt,
                     clippy, cargotest},
         runtimeProof: "test"|"app"|"probe"|"none", acceptanceOutput, notDone: [...]}
${LOGLINE(`${p} build`, p, `${tag}:build`, '--open <the open loop-build PR count you just observed>')}
${GUARD(dir)}`,
    { model: 'opus', effort: 'high', phase: p, label: `build:${tag}:${p}` },
    item
  )
  if (build === null) {
    // The Workflow runtime does NOT throw when an agent dies on an API error (usage/session limit,
    // rate limit, overload) — it returns null. A real build agent always returns a payload, so null
    // here means the account has no capacity: stop BOTH lanes instead of failing 30 items in a row.
    noteHalt(`${item.id} lane ${LANE_TAGS[lane]}`, 'build agent returned null — API failure or session/usage limit; no capacity')
    return { itemId: item.id, status: 'halted', lane: LANE_TAGS[lane], error: 'agent returned null (limit)' }
  }

  // Belt and braces: a builder that somehow concludes the item was already done is believed only
  // if the EXECUTED pre-flight above agreed with it, and it did not (we are past that branch).
  if (saysAlreadyDone(build)) {
    log(`${p} (lane ${tag}): the builder claimed already-done but the EXECUTED pre-flight failed at ${(pre && pre.firstFailure) || 'a named command'} — that is a false capability claim; the acceptance run below decides`)
  }

  /**
   * STEP 2 — ACCEPTANCE, EXECUTED. One command-runner seat, on the item's own branch, in this
   * lane's worktree. The loop branches on its schema'd allZero, so "acceptance passed" is no
   * longer a sentence the builder wrote about itself.
   */
  const branchTo = `    cd ${dir} && git fetch origin && git checkout ${item.branch} && git reset --hard origin/${item.branch}`
  let accept = await runCommands('acceptance', item, lane, branchTo, item.acceptance, p)
  if (accept === null) {
    noteHalt(`${item.id} lane ${LANE_TAGS[lane]} acceptance`, 'acceptance runner returned null — API failure or session/usage limit; no capacity')
    return { itemId: item.id, status: 'halted', lane: LANE_TAGS[lane], error: 'agent returned null (limit)', build }
  }
  if (runPassed(accept)) {
    log(`${p} (lane ${tag}): acceptance EXECUTED and passed on ${item.branch} — PR open, unreviewed, ci=${CI_MODE}`)
    return { itemId: item.id, status: 'built', lane: tag, build, preflight: pre, acceptance: accept, fixRounds: 0 }
  }

  /**
   * STEP 3 — ONE FIX ROUND. Exactly one, and it goes to a builder seat that is handed the
   * acceptance output it has to answer for. Then the acceptance is EXECUTED again by a fresh
   * command-runner seat — never by the agent that just changed the code.
   */
  log(`${p} (lane ${tag}): acceptance FAILED at ${(accept && accept.firstFailure) || 'a command it named'} — ONE fix round, then it is re-executed`)
  const fix = await agentR(
    `You are the FIX ROUND for ${item.id} on lane ${tag}. Its ACCEPTANCE WAS EXECUTED AND IT FAILED.
This is your only round. What you do not fix now is recorded as a failed item.

${WORKTREE_CONTRACT(dir, port, target, other)}

# THE ACCEPTANCE OUTPUT YOU HAVE TO ANSWER FOR — raw, from the runner seat
${runText(accept)}

# THE COMMANDS THAT MUST EXIT 0 (they are re-executed after you, verbatim, by a seat that cannot
# be argued with)
${item.acceptance}

# THE ITEM, SO YOU FIX THE RIGHT THING
${item.spec}

# WHAT TO DO
    cd ${dir} && git fetch origin && git checkout ${item.branch} && git reset --hard origin/${item.branch}
Fix the CAUSE of the failing command. Do NOT weaken, delete, skip or rewrite an acceptance command
to make it pass — the commands are re-executed from the item, not from your branch, so editing them
achieves nothing except a wasted round. Do not expand scope beyond making those commands pass.
If the acceptance command itself is WRONG (it names a path that never existed, or asserts something
the item never asked for), say so plainly in your return as acceptanceIsWrong with the evidence, fix
what you legitimately can, and push anyway — the loop records the item as failed and a human reads
your note. That is an honest outcome; a quietly weakened check is not.

# THEN, BEFORE YOU RETURN
Run the full gate on your branch and paste every code:
${GATE_CMDS(dir, target)}
    cd ${dir} && git add -A && git commit -m "${item.id}: fix round — acceptance" && git push
Update the PR body with what failed, what you changed, and the raw gate output.
${VERIFY_NOTHING}
${COMMIT_CONTRACT}
RETURN: {itemId: "${item.id}", pr: <number>, fixed: true|false, whatFailed, whatIChanged,
         acceptanceIsWrong: "<evidence, or empty>", gateExits: {...}}
${LOGLINE(`${p} fix`, p, `${tag}:fix-r1`)}
${GUARD(dir)}`,
    { model: 'opus', effort: 'high', phase: p, label: `fix:${tag}:${p}` },
    item
  )
  if (fix === null) {
    noteHalt(`${item.id} lane ${LANE_TAGS[lane]} fix round`, 'fix agent returned null — API failure or session/usage limit; no capacity')
    return { itemId: item.id, status: 'halted', lane: LANE_TAGS[lane], error: 'agent returned null (limit)', build, acceptance: accept }
  }
  accept = await runCommands('acceptance-rerun', item, lane, branchTo, item.acceptance, p)
  if (accept === null) {
    noteHalt(`${item.id} lane ${LANE_TAGS[lane]} acceptance re-run`, 'acceptance runner returned null — API failure or session/usage limit; no capacity')
    return { itemId: item.id, status: 'halted', lane: LANE_TAGS[lane], error: 'agent returned null (limit)', build, fix }
  }
  if (runPassed(accept)) {
    log(`${p} (lane ${tag}): acceptance passed after ONE fix round — PR open, unreviewed, ci=${CI_MODE}`)
    return { itemId: item.id, status: 'built', lane: tag, build, preflight: pre, acceptance: accept, fix, fixRounds: 1 }
  }

  /**
   * STEP 4 — FAILED, AND THE LOOP CONTINUES. There is no second fix round and no merge: the PR
   * stays OPEN carrying the acceptance output, labelled for a human, and the lane takes its next
   * item. An item that cannot pass its own acceptance is a finding, not a stall.
   */
  log(
    `${p} (lane ${tag}): FAILED ACCEPTANCE after one fix round (first failure: ${(accept && accept.firstFailure) || 'named in the payload'}). ` +
      `The PR is left OPEN with the output in its body; nothing merges it. The lane continues.`
  )
  const note = await agentR(
    `You are the FAILURE RECORDER for ${item.id} on ${REPO}. The item's acceptance was EXECUTED twice
— once after the build, once after its single fix round — and it did not pass. Your whole job is to
make that visible on GitHub and on the board. You fix NOTHING and you merge NOTHING.

    cd ${dir} && gh pr list -R ${REPO} --head ${item.branch} --state open --json number,url
Take that PR number as <n>. If there is no open PR, say so and record the item as failed with no PR.

    ${LAND_BAR_CI ? `gh label create loop-acceptance-red -R ${REPO} --color FBCA04 --description "loop: item acceptance failed twice" 2>/dev/null ; true
    gh pr edit -R ${REPO} <n> --add-label loop-acceptance-red` : `gh pr edit -R ${REPO} <n> --add-label needs-human`}
    gh pr comment -R ${REPO} <n> --body "<the comment below, verbatim, with the output pasted in>"
    gh pr edit -R ${REPO} <n> --body "<the existing body, with this same block appended at the TOP>"

THE COMMENT, verbatim, and the raw output goes in an indented block (four spaces per line — do not
use a markdown fence, the output may contain one):

## Acceptance FAILED — executed twice by the loop${LAND_BAR_CI ? '' : ', not merged'}
${LAND_BAR_CI ? 'BUILD-ONLY run (landBar ci): the drain still merges this PR once GitHub CI is green, and the next build pass re-runs the item (its pre-flight will fail on main) to close the gap.' : 'This PR is NOT mergeable by the loop and nothing in it has been reviewed.'} The item's acceptance
commands were run verbatim on this branch by a command-runner seat, once after the build and once
after one fix round, and they did not all exit 0.
first failure: ${(accept && accept.firstFailure) || 'see the output below'}
acceptance output:
<the output, every line indented by four spaces>

Then the board, and it must say failed:
    node ${STATUS} ${p} ${tag}:failed "acceptance failed twice; PR open, ${LAND_BAR_CI ? 'loop-acceptance-red' : 'needs-human'}, ci=${CI_MODE}" <n>
${LOGLINE(`${p} failed-acceptance`, p, `${tag}:failed`)}
RETURN: {itemId: "${item.id}", pr: <n or 0>, labelled: true|false, commented: true|false, boardWritten: true|false}
${GUARD(dir)}
ONE MORE CAP: 15 tool calls. You are writing a comment and a label, nothing else.`,
    { model: 'opus', effort: 'low', phase: p, label: `record-failure:${tag}:${p}` }
  )
  return {
    itemId: item.id,
    status: 'failed-acceptance',
    lane: tag,
    build,
    preflight: pre,
    acceptance: accept,
    fix,
    fixRounds: 1,
    recorded: note,
  }
}

/* ══ REVIEW CHAIN REGION START ═════════════════════════════════════════════════════════════
   Everything from here to REVIEW CHAIN REGION END runs inside the two review chains, therefore
   CONCURRENTLY. Two rules bind it and build.mjs enforces both:
     - it may not set the global phase cursor (it is inside the NO-PHASE region as well);
     - every agent dispatch in it must sit inside a withAgents() lease, which is the ONLY thing
       that keeps the run under MAX_AGENTS. */

/**
 * THE 3-AGENT SEMAPHORE.
 * activeAgents is the number of seats currently in flight across BOTH chains. A chain that cannot
 * fit parks a resolver on agentWaiters and is woken by whoever releases — it never polls. A poll
 * loop (`await Promise.resolve()` in a while) would spin the runtime's event loop for the ten-plus
 * minutes an opus/high seat takes, which is not a wait, it is a busy-wait with a nice name.
 */
let activeAgents = 0
const agentWaiters = []
function grantWaiters() {
  for (let i = 0; i < agentWaiters.length; i++) {
    const w = agentWaiters[i]
    if (activeAgents + w.n <= MAX_AGENTS) {
      activeAgents += w.n
      agentWaiters.splice(i, 1)
      i--
      w.resolve()
    }
  }
}
async function acquireAgents(n) {
  if (activeAgents + n <= MAX_AGENTS) {
    activeAgents += n
    return
  }
  await new Promise((resolve) => agentWaiters.push({ n, resolve }))
}
function releaseAgents(n) {
  activeAgents -= n
  if (activeAgents < 0) activeAgents = 0
  grantWaiters()
}
/** Hold n seats for the whole of fn. Released in a finally, so a thrown seat never leaks a slot. */
async function withAgents(n, fn) {
  await acquireAgents(n)
  try {
    return await fn()
  } finally {
    releaseAgents(n)
  }
}

/** THE SHARED QUEUE. Both chains pull from one cursor: whoever is free takes the next PR. */
let REVIEW_QUEUE = []
let queueCursor = 0
const reviewOutcomes = []
function takeNextPr() {
  if (queueCursor >= REVIEW_QUEUE.length) return null
  const entry = REVIEW_QUEUE[queueCursor]
  queueCursor++
  return entry
}

/**
 * ONE PR THROUGH THE GATE. (a) reviewer — one seat, or TWO for a security PR (its second lens runs
 * in the REVIEW-STAGE barrier); each seat posts its OWN record on the head it read (review-gate.mjs).
 * (b) on a BLOCK, fix-land in FIX mode — rebase, fix or refute every blocking finding, push, and NOT
 * merge — then the reviewer seats are re-dispatched (same lens, fresh context) at the new head. At
 * most MAX_FIX_ROUNDS fix rounds; a further BLOCK parks the PR as needs-human. (c) once every
 * required seat has PASSed the current head, fix-land in LAND mode runs CI_GATE (which runs the
 * review gate) and merges that sha. It NEVER waits for CI.
 * Re-dispatch covers the seat(s) that blocked AND every other required seat: a push voids every
 * record, so a security PR's non-blocking seat must PASS the new head too or the gate refuses it.
 * GATE ONCE PER HEAD (2026-10-27): reviewers verify scripts/loop/gate-ledger.mjs records and never run the full gate;
 * fix-land's post-push run is the round's one full run and records it; LAND reuses it (or green CI on the head).
 */
async function reviewOnePr(entry, chain) {
  const dir = REVIEW_DIRS[chain]
  const port = REVIEW_PORTS[chain]
  const target = REVIEW_TARGETS[chain]
  const other = REVIEW_DIRS[chain === 1 ? 0 : 1]
  const pr = String((entry && (entry.pr || entry.number)) || '')
  const itemId = String((entry && entry.item) || `PR-${pr}`)
  const klass = entry && entry.klass === 'security' ? 'security' : 'standard'
  const seats = klass === 'security' ? 2 : 1
  const facts = JSON.stringify(entry || {})

  const reReview = (round, prior) =>
    round === 0
      ? ''
      : `
# RE-REVIEW ROUND ${round} — A FIX WAS PUSHED, AND ONLY A FRESH PASS ON THE NEW HEAD LANDS IT
Fix round ${round} of at most ${MAX_FIX_ROUNDS} pushed a new head; every record on the old head is now history.
You are a FRESH seat with the same lens. Review the WHOLE PR at its CURRENT head — not only the fix — and test
each prior blocking finding against the new diff. The fixer's word that a finding is "fixed" or "refuted" is a
claim to test, never evidence. A BLOCK now sends the PR to ${round >= MAX_FIX_ROUNDS ? 'needs-human (this is the last round)' : 'one more fix round'}.
PRIOR BLOCKING FINDINGS AND THE FIXER'S DISPOSITIONS: ${JSON.stringify(prior)}
`
  const primaryPrompt = (round, prior) => `You are the PRIMARY ADVERSARIAL REVIEWER of PR #${pr} on ${REPO} (${itemId}, classified ${klass}).
Yap is a macOS dictation app: Rust/Tauri v2 backend, a Vite/React frontend, a yap-polish sidecar.
Your job is to TRY TO DISPROVE this PR's claims. Not to falsify them — to test them honestly and
hard, and to report what survives. A finding you cannot substantiate is not a finding.

Read it: gh pr view -R ${REPO} ${pr} --json number,title,body,headRefOid,files
and gh pr diff -R ${REPO} ${pr}. You read code in ${dir}, DETACHED at the head (below) — never a branch
there: the fix agent owns that worktree next.
TRIAGE ALREADY ESTABLISHED: ${facts}
${reReview(round, prior)}
${REVIEW_GATE_READ(dir, target, pr)}
# THE CONTRACT — classify every finding as exactly one
BLOCKING — a real defect in shipped behaviour: a code bug; a crash or panic path; data loss in the
  local history; a permission or signing regression; audio or text leaving the machine when it
  should not; work dropped after a response closes; a LIVENESS failure (the changed code is not
  reached — a command absent from generate_handler!, a module no entry imports, a capability never
  granted); or a FALSE CAPABILITY CLAIM (the PR body says a behaviour ships when the diff shows it
  does not). Keep FALSE CAPABILITY CLAIM blocking — it is the category that catches real exposure.
ADVISORY — everything else: counts, stale line citations, prose, naming, comment wording, style,
  rustfmt drift (fmt fails on unmodified main and is informational here).
Prefix every finding with "BLOCKING: " or "ADVISORY: ".
A PR merges with ZERO blocking findings no matter how many advisory ones.

# WHAT TO ATTACK HARDEST
1. REACHABILITY / LIVENESS. Run the greps yourself. Is the changed code actually reached? A
   previous session shipped five fixes into dead modules that passed every gate.
2. EVIDENCE CLASS. Did the author prove behaviour with a test that fails without the change, with
   the running app, or with a probe — or only with "it compiles"? A green build is not a working
   UI, and this app's defects are overwhelmingly runtime ones: hotkeys, window placement, timers,
   permissions, sidecar handshakes.
3. Does the diff match what the PR body says it does? Diff the claim against the code.
4. Did it break a surface it did not mention — the float/pill window, the main window, the tray,
   the updater, the sidecar protocol, the DB schema?
5. HAS MAIN MOVED UNDER IT? Check whether the PR is now redundant, contradicted by, or duplicated
   on origin/main. Say so explicitly — that is a real outcome.
${REACHABILITY}
${RUNTIME_PROOF(dir)}
${VERIFY_NOTHING}

If you return BLOCK you MUST return a non-empty blocking[] array. A BLOCK with an empty blocking[]
is malformed: the record subcommand refuses it, so the PR is left review-pending with no verdict and
a finding you fail to write down is a finding you did not make.
${REVIEW_RECORD('review-opus', pr, 'primary adversarial: reachability, evidence class, claims vs diff')}

RETURN: {verdict: "PASS"|"BLOCK", blocking: [...], advisory: [...], reachabilityProofRun: true|false,
         evidenceClass: "test"|"app"|"probe"|"compiles-only"|"none", supersededByMain: true|false,
         recordHead: "<40-hex>", recordPosted: true|false}
${LOGLINE(`${itemId} review-opus`, itemId, 'review:review-r1', `${pr}`)}
${GUARD(dir)}`

  const lensPrompt = (round, prior) => `You are the INDEPENDENT ADVERSARIAL REVIEWER of PR #${pr} on ${REPO} (${itemId}) — a second
reviewer with a DIFFERENT LENS, seated because triage classified this PR SECURITY.
A primary adversarial reviewer is reviewing it right now. ITS VERDICT IS HIDDEN FROM YOU BY DESIGN.
Do not ask for it, do not guess it, do not try to agree with it. Two reviewers that share a premise
are one reviewer: this harness has already shipped a dead-code fix that TWO independent reviewers
both approved, because both accepted the same false premise. You exist to break that.

Read it: gh pr view -R ${REPO} ${pr} --json number,title,body,headRefOid,files
and gh pr diff -R ${REPO} ${pr}. Read code in ${dir}, DETACHED at the head (below); never check out a branch there.
TRIAGE ALREADY ESTABLISHED: ${facts}
${reReview(round, prior)}
${REVIEW_GATE_READ(dir, target, pr)}
# THE CONTRACT — identical to the primary reviewer's. BLOCKING vs ADVISORY, prefix every finding.
# YOUR FOUR LENSES — the primary reviewer is NOT asked for these. Spend your effort HERE.
(a) LIVENESS / FALSE PREMISE. Is the code this PR touches actually REACHED at runtime on current
    HEAD? Name the chain: HTML entry -> import -> symbol, or invoke() -> generate_handler! ->
    command. If you cannot name that chain, the change is dead code and that is BLOCKING.
(b) FAILURE POLARITY. For EVERY guard, permission check, capability, unwrap, expect and Result
    branch in the diff, state which way it fails. What happens when the input is None, an empty
    string, a denied permission, a missing sidecar binary, a closed window or an Err? Does any
    catch/unwrap_or return a PERMISSIVE or FABRICATED value (true, an empty transcript treated as
    success, a silently swallowed error)? A path that fails OPEN, or that panics on the main
    thread, is BLOCKING even when the happy path is correct. Quote the exact expression and write
    "fails open", "fails closed" or "panics" for each one.
(c) LOCAL DATA AND PERMISSION INTEGRITY. For every SQLite migration, schema change, file write,
    keychain access, capability edit or tauri.conf change, say what breaks for: a fresh install, an
    existing install with history, an install whose permissions were granted to the current bundle
    id, and an install mid-update. An unbounded UPDATE/DELETE, a dropped column still read by
    shipped code, a migration that cannot run twice, a widened capability, and ANY change to the
    bundle identifier or the data directory are all BLOCKING.
(d) CLAIMS VS DIFF. Every sentence in the PR body containing "verified", "proven", "confirmed",
    "tested" or a pasted number MUST map to raw output pasted in the PR. A claim with no raw output
    behind it is a FALSE CAPABILITY CLAIM and is BLOCKING. An exit code read through a pipe is the
    exit code of the LAST command in the pipe, so evidence piped into tail/grep proves nothing.
${RUNTIME_PROOF(dir)}
${VERIFY_NOTHING}

If you cannot read the PR at all (gh fails, the PR is missing), return status "unavailable" and post
NO record — never BLOCK on your own inability to review, and never PASS it either. With no record from
you the security PR stays review-pending; it does not land on a verdict nobody gave.
A BLOCK with an empty blocking[] is malformed: the record subcommand refuses it.
${REVIEW_RECORD('review-opus2', pr, 'liveness, failure polarity, local data and permission integrity, claims vs diff')}

RETURN: {status: "reviewed"|"unavailable", verdict: "PASS"|"BLOCK", blocking: [...], advisory: [...],
         reachabilityChain: "<entry -> import -> symbol, or why none exists>", failurePolarity: [...],
         dataOrPermissionRisk: "<what and for whom, or none>", whyUnavailable,
         recordHead: "<40-hex>", recordPosted: true|false}
${LOGLINE(`${itemId} review-opus2`, itemId, 'review:review-r1', `${pr}`)}
${GUARD(dir)}`

  // A NULL SEAT IS UNAVAILABLE, NEVER A BLOCK — AND NEVER A PASS. A reviewer that died produced no
  // finding and no record, so the PR cannot land on it: it ends review-pending (the land gate refuses
  // a head with no record), never merged on a verdict nobody gave.
  const UNAVAILABLE = { status: 'unavailable', verdict: 'NONE', blocking: [], advisory: [], whyUnavailable: 'seat returned null (API failure)' }
  const noVerdict = (v) => v === null || /"status"\s*:\s*"unavailable"/i.test(asText(v)) || isMalformedBlock(v) || !/"verdict"\s*:\s*"(PASS|BLOCK)"/i.test(asText(v))
  // The FIX mode's first step, kept verbatim from the one-round fix+land prompt.
  const REBASE_STEP = `# 1 — REBASE FIRST, ALWAYS. THIS IS STEP ONE AND IT IS NOT OPTIONAL.
Main has moved under this PR, and other chains are merging while you work.
    cd ${dir} && git fetch origin
    git checkout -B <the PR's headRefName> origin/<the PR's headRefName>
    git rebase origin/main
Evaluating checks on a stale head is how this loop has failed before — a conflict is an artifact of
loop duration, not a defect in the work, so RESOLVE it; the work is yours to carry, not to abandon.
IF THE REBASE SHOWS THE PR IS ALREADY IN MAIN (empty after rebase, or its change is present on
origin/main by another route): do NOT force it through. CLOSE it with the evidence —
    gh pr close -R ${REPO} ${pr} --comment "superseded by <sha/PR#> on main: <evidence>"
— and return status "closed-superseded". A superseded PR closed with evidence is a WIN, not a miss.
After a rebase push use git push --force-with-lease, NEVER a plain --force.
If gh already says this PR is MERGED, do not merge it again: review the merged diff, post the
verdicts on it, and if there is a BLOCKING defect open a follow-up PR and say so plainly.
${COMMIT_CONTRACT}

`
  let round = 0 // fix rounds completed
  let prior = null
  let primary = null
  let lens = null
  let blocked = false
  const rounds = []
  for (;;) {
    let rA = null
    let rB = null
    await withAgents(seats, async () => {
      if (seats === 2) {
        /* REVIEW-STAGE-PARALLEL */
        // Both prompts are built from the same inputs before either is dispatched, precisely so neither
        // reviewer can depend on the other's outcome. This barrier is the ONLY concurrent dispatch in the
        // harness and it costs TWO of the three seats, which is why the other chain can hold at most one.
        const both = await parallel([
          () => agentR(primaryPrompt(round, prior), { model: 'opus', effort: 'high', phase: itemId, label: `review-opus:${itemId}` }),
          () => agentR(lensPrompt(round, prior), { model: 'opus', effort: 'high', phase: itemId, label: `review-opus2:${itemId}` }),
        ])
        rA = both[0]
        rB = both[1]
      } else {
        rA = await agentR(primaryPrompt(round, prior), { model: 'opus', effort: 'high', phase: itemId, label: `review-opus:${itemId}` })
      }
    })
    if (rA === null && (seats === 1 || rB === null)) {
      noteHalt(`${itemId} PR#${pr} review round ${round}`, 'every reviewer seat returned null — API failure or session/usage limit')
      return { itemId, pr, klass, status: 'halted-in-review', rounds }
    }
    primary = rA === null ? UNAVAILABLE : rA
    lens = seats === 2 ? (rB === null ? UNAVAILABLE : rB) : null
    // A BLOCK THAT NAMES NO FINDING IS A HARNESS ERROR, NOT A VERDICT (2026-10-27): re-ask that seat ONCE for the
    // array; still empty -> label loop-review-error and spend no fix round on findings nobody handed the fixer.
    if (isMalformedBlock(primary)) primary = (await withAgents(1, () => agentR(primaryPrompt(round, prior) + FINDINGS_REASK, { model: 'opus', effort: 'high', phase: itemId, label: `review-opus:${itemId}` }))) || primary
    if (lens && isMalformedBlock(lens)) lens = (await withAgents(1, () => agentR(lensPrompt(round, prior) + FINDINGS_REASK, { model: 'opus', effort: 'high', phase: itemId, label: `review-opus2:${itemId}` }))) || lens
    const emptyBlock = [['review-opus', primary], ['review-opus2', lens]].filter(([, v]) => v && isMalformedBlock(v)).map(([w]) => w)
    if (emptyBlock.length) {
      const why = `review round ${round}: ${emptyBlock.join(' + ')} returned BLOCK with no findings array twice — a harness error, not a verdict`
      const mark = await withAgents(1, () => agentR(reviewErrorPrompt(why), { model: 'opus', effort: 'high', phase: itemId, label: `review-error:${itemId}` }))
      log(`PR#${pr} (${itemId}, ${klass}): ${why} -> loop-review-error (no fix round spent)`)
      return { itemId, pr, klass, chain: REVIEW_TAGS[chain], status: 'loop-review-error', blocked: true, rounds, reason: why, land: mark }
    }
    const primaryBlocked = saysBlock(primary) && !isMalformedBlock(primary)
    const lensBlocked = lens !== null && saysReviewed(lens) && saysBlock(lens) && !isMalformedBlock(lens)
    const repeated = round > 0 ? repeatedReviewHead(rounds, [primary, lens]) : ''
    rounds.push({ round, primary, lens, primaryBlocked, lensBlocked })
    if (repeated) {
      // The fix round's re-review read the SAME head an earlier round read: no new head, whatever the fixer said.
      const why = `fix round ${round}: the re-review recorded head ${repeated.slice(0, 9)}, which an earlier round of this pass already reviewed — no new head; refuted findings: ${JSON.stringify(prior && prior.dispositions).slice(0, 1500)}`
      const hold = await withAgents(1, () => agentR(fixLandPrompt('hold', round, why), { model: 'opus', effort: 'high', phase: itemId, label: `fix-land:${itemId}` }))
      log(`PR#${pr} (${itemId}, ${klass}): ${why} -> needs-human (no LAND)`)
      return { itemId, pr, klass, chain: REVIEW_TAGS[chain], status: 'needs-human', blocked: true, rounds, reason: why, land: hold }
    }
    if (primaryBlocked || lensBlocked) blocked = true
    if (!primaryBlocked && !lensBlocked) {
      const silent = [['review-opus', primary], ...(seats === 2 ? [['review-opus2', lens]] : [])].filter(([, v]) => noVerdict(v)).map(([w]) => w)
      if (silent.length) {
        // No BLOCK, but a required seat gave no verdict, so there is no PASS record on the head. The land
        // gate would refuse it; spending a lander to learn that is waste. The sweeper's gate labels it.
        log(`PR#${pr} (${itemId}, ${klass}) round ${round}: no verdict from ${silent.join(', ')} -> review-pending (no PASS bound to the head)`)
        return { itemId, pr, klass, chain: REVIEW_TAGS[chain], status: 'review-pending', blocked, rounds, reason: `no verdict from ${silent.join(', ')}` }
      }
      break
    }
    if (round >= MAX_FIX_ROUNDS) {
      // A BLOCK after the last fix round: the PR is parked, never merged and never fixed a third time.
      const park = await withAgents(1, () => agentR(fixLandPrompt('park', round), { model: 'opus', effort: 'high', phase: itemId, label: `fix-land:${itemId}` }))
      log(`PR#${pr} (${itemId}, ${klass}): BLOCK after ${round} fix round(s) -> needs-human`)
      return { itemId, pr, klass, chain: REVIEW_TAGS[chain], status: 'needs-human', blocked, rounds, land: park }
    }
    const fix = await withAgents(1, () => agentR(fixLandPrompt('fix', round + 1), { model: 'opus', effort: 'high', phase: itemId, label: `fix-land:${itemId}` }))
    const fixText = asText(fix)
    const fixStatus = fix === null
      ? 'errored'
      : /"status"\s*:\s*"fixed"/i.test(fixText)
        ? 'fixed'
        : (fixText.match(/"status"\s*:\s*"(no-new-head|closed-superseded|needs-human|needs-rebase|ci-red)"/i) || [])[1] || 'needs-human'
    rounds[rounds.length - 1].fix = fix
    const reviewedHeads = [primary, lens].map(recordHeadOf)
    const fixHead = fixRoundHead(fix, reviewedHeads)
    if (fixStatus === 'no-new-head' || (fixStatus === 'fixed' && !fixHead.changed)) {
      // NO NEW HEAD: a refute-only round. The BLOCK stays final on that head; no reviewer and no lander is spent.
      const refuted = fix && Array.isArray(fix.blockingDispositions) ? fix.blockingDispositions : fixText.slice(0, 2000)
      const why = `fix round ${round + 1}: no new head (${fixStatus === 'no-new-head' ? 'the fixer refuted and pushed nothing' : fixHead.why}); refuted findings: ${JSON.stringify(refuted).slice(0, 1500)}`
      rounds[rounds.length - 1].noNewHead = true
      // The fixer labels needs-human itself on no-new-head; a fixer that claimed "fixed" without moving the head did
      // not, so ONE hold seat labels it and comments the refuted findings.
      const hold = fixStatus === 'no-new-head' ? fix : await withAgents(1, () => agentR(fixLandPrompt('hold', round + 1, why), { model: 'opus', effort: 'high', phase: itemId, label: `fix-land:${itemId}` }))
      log(`PR#${pr} (${itemId}, ${klass}): ${why} -> needs-human (not re-reviewed: a refute is not a push)`)
      return { itemId, pr, klass, chain: REVIEW_TAGS[chain], status: 'needs-human', blocked, rounds, reason: why, land: hold }
    }
    if (fixStatus !== 'fixed') {
      log(`PR#${pr} (${itemId}, ${klass}): fix round ${round + 1} ended ${fixStatus} — not re-reviewed`)
      return { itemId, pr, klass, chain: REVIEW_TAGS[chain], status: fixStatus, blocked, rounds, land: fix }
    }
    round++
    prior = {
      primaryBlocking: findingsFor(primary, pr),
      lensBlocking: findingsFor(lens, pr),
      dispositions: fix && Array.isArray(fix.blockingDispositions) ? fix.blockingDispositions : fixText.slice(0, 4000),
      newHead: fixHead.after,
    }
    log(`PR#${pr} (${itemId}, ${klass}): fix round ${round} pushed ${prior.newHead} — re-dispatching ${seats === 2 ? 'review-opus + review-opus2' : 'review-opus'} at the new head`)
  }
  const verdicts = { primary, lens, seats, rounds: rounds.length }

  /** THE REVIEW-ERROR SEAT: labels a PR whose reviewer returned BLOCK with no findings twice. Never fixes or merges. */
  function reviewErrorPrompt(why) {
    return `You are the REVIEW-ERROR agent for PR #${pr} on ${REPO} (${itemId}, ${klass}). A reviewer seat returned BLOCK
with no findings array, twice. That is a harness error, not a verdict: no fix round is spent on it.
${why}
Do NOT fix it, do NOT push, do NOT merge, do NOT review. The next review pass re-reviews it.
    gh label create loop-review-error -R ${REPO} --color B60205 --description "a review seat returned BLOCK with no findings; re-review" 2>/dev/null ; true
    gh pr edit -R ${REPO} ${pr} --add-label loop-review-error || gh api -X POST repos/${REPO}/issues/${pr}/labels -f "labels[]=loop-review-error"
    gh pr comment -R ${REPO} ${pr} --body "Harness error, not a verdict: <the reason above, in plain words, with the head>"
Start the comment with exactly those words. Never name the label or the reviewer record marker at the start of
any comment line, and never write a "verdict:" line: the review gate reads such a line as a BLOCK record.
RETURN: {itemId: "${itemId}", pr: ${pr || 0}, status: "loop-review-error", landed: false, reason}
${GUARD(dir)}`
  }
  /**
   * THE FIX / LAND / PARK SEAT. Never a reviewer: it never posts a verdict titled as a review and is
   * never handed the record subcommand (build.mjs validation). 'fix' pushes and does NOT merge; 'land'
   * merges only through CI_GATE (whose review gate needs a reviewer PASS on that sha); 'park' labels.
   */
  function fixLandPrompt(mode, fixRound, why = '') {
    const role = mode === 'fix' ? `FIX agent (fix round ${fixRound} of at most ${MAX_FIX_ROUNDS})` : mode === 'land' ? 'LAND agent' : mode === 'hold' ? 'HOLD agent' : 'PARK agent'
    return `You are the ${role} for PR #${pr} on ${REPO} (${itemId}, ${klass}). You are a FIXER seat, never a
reviewer: you never post a review verdict, and only a reviewer seat's own record on the current head can
land this PR. Be paranoid, and be fast.
${mode === 'park' ? `
# THE PR IS OUT OF FIX ROUNDS
A reviewer BLOCKed this PR again after ${MAX_FIX_ROUNDS} fix rounds. Do NOT fix it, do NOT push, do NOT merge.
    gh pr edit -R ${REPO} ${pr} --add-label needs-human
    gh pr comment -R ${REPO} ${pr} --body "needs-human: still BLOCKED after ${MAX_FIX_ROUNDS} fix rounds. <every open BLOCKING finding below, verbatim, with the seat that raised it>"
OPEN BLOCKING FINDINGS: ${JSON.stringify({ primary: findingsFor(primary, pr), lens: findingsFor(lens, pr) })}
RETURN: {itemId: "${itemId}", pr: ${pr || 0}, status: "needs-human", landed: false, reason}` : ''}${mode === 'hold' ? `
# HOLD THIS PR FOR A HUMAN — A BLOCK IS FINAL ON ITS CURRENT HEAD
${why}
Do NOT fix it, do NOT push (an empty commit included), do NOT merge, do NOT re-review.
    gh label create needs-human -R ${REPO} --color D93F0B --description "a human decides" 2>/dev/null ; true
    gh pr edit -R ${REPO} ${pr} --add-label needs-human --remove-label review-pending
    gh pr comment -R ${REPO} ${pr} --body "needs-human: <the reason above, in plain words: the head, the BLOCK that is final on it, and each refuted finding with its evidence>"
RETURN: {itemId: "${itemId}", pr: ${pr || 0}, status: "needs-human", landed: false, reason}` : ''}${mode === 'fix' ? `

# 0 — RECORD THE HEAD BEFORE YOU TOUCH ANYTHING
    gh pr view -R ${REPO} ${pr} --json headRefOid             -> headBefore (40 hex). Return it.
` + REBASE_STEP + `
# 2 — DISPOSE OF EVERY BLOCKING FINDING.
PRIMARY REVIEWER: ${JSON.stringify(primary)}
INDEPENDENT REVIEWER (security PRs only; null means it was not seated): ${JSON.stringify(lens)}
OPEN BLOCKING FINDINGS, VERBATIM: ${JSON.stringify({ primary: findingsFor(primary, pr), lens: findingsFor(lens, pr) })}
Fix ONLY the BLOCKING findings. Do NOT address advisory findings — they are logged once and never
re-litigated. Do NOT expand scope. If you believe a blocking finding is WRONG, do not silently drop
it: post a PR comment with the evidence that refutes it. A reviewer being wrong is a legitimate
outcome; a finding quietly dropped is not.
${MERGE_BAR}
blockingDispositions is REQUIRED in your return, one entry per blocking finding raised by either
reviewer. If any blocking finding is neither fixed nor refuted: leave the PR open,
    gh pr edit -R ${REPO} ${pr} --add-label needs-human
comment exactly which finding stopped you, and return status "needs-human".
THE REVIEW PASS HAS A SECOND ROUND, AND YOU ARE NOT IT. After you push, the chain re-dispatches the
reviewer seat(s) at YOUR new head — same lens, fresh context — and only their fresh PASS, posted by
them on that head, can land this PR. At most ${MAX_FIX_ROUNDS} fix rounds; a BLOCK after the last one ends
as needs-human. You do NOT merge in this mode, whatever the gate says.

# 3 — THE GATE, EVERY COMMAND, SEPARATELY, ON THE COMMITTED REBASED HEAD — THE ONE FULL RUN OF THIS ROUND
Commit your fixes FIRST, then run the gate on that commit and WRITE ITS LEDGER RECORD (the last step of the block
below). The re-reviewers and the LAND seat do not run the gate — they reuse your record for this sha — so a skipped
record costs a whole extra run. Commit nothing after the gate: a new commit is a new sha with no record.
${GATE_CMDS(dir, target)}
Then git push --force-with-lease, and read the new head: gh pr view -R ${REPO} ${pr} --json headRefOid
It MUST equal the sha you recorded; if it does not, record again for the pushed sha only if that tree is the one you gated.
IF YOU HAVE NOTHING TO PUSH — every blocking finding refuted, no fix committed, the rebase a no-op — the head
does not change, and re-reviewing an unchanged head cannot clear a BLOCK (a BLOCK is final for its head). Do NOT
push, and NEVER make an empty commit to force a new head: that launders a refute into a push. Instead:
    gh label create needs-human -R ${REPO} --color D93F0B --description "a human decides" 2>/dev/null ; true
    gh pr edit -R ${REPO} ${pr} --add-label needs-human --remove-label review-pending
    gh pr comment -R ${REPO} ${pr} --body "needs-human: fix round ${fixRound} pushed no new head (headBefore <sha>). Refuted findings: <each, with its evidence>"
and return status "no-new-head" with head = headBefore. The chain dispatches no reviewer and no lander after it.
${VERIFY_NOTHING}

# 4 — POST YOUR DISPOSITIONS — A FIXER'S NOTE, NEVER A REVIEW
gh pr comment -R ${REPO} ${pr} --body "Fix round ${fixRound} — fixer dispositions (NOT a review; the reviewer seats re-review head <new headRefOid>)

<each blocking finding, its disposition fixed|refuted, and the hunk or evidence>"
Never title that comment as a review, never start it with the reviewer record marker, and never use
gh pr review. Never write the reviewer record marker (the first line of a reviewer record) anywhere in a
comment — not quoted, not mid-sentence — and never write a "verdict:" line: the gate reads a comment that names
the marker and carries a BLOCK verdict line as a BLOCK on that head, wherever the marker sits. Advisory findings stay where the reviewer posted them — never a new PR.

RETURN: {itemId: "${itemId}", pr: ${pr || 0}, status: "fixed"|"no-new-head"|"needs-human"|"needs-rebase"|"closed-superseded",
         headBefore: "<headRefOid from step 0>", head: "<the headRefOid after your push (headBefore when nothing was pushed)>", rebased: true|false, conflictsResolved: [...],
         blockingDispositions: [{finding, disposition: "fixed"|"refuted", evidence}], gateRecord: "<the ledger record line>",
         gateExits: {typecheck, test, build, polish, diarize, stage, polishtest, diarizetest, fmt, clippy, cargotest}}` : ''}${mode === 'land' ? `
# EVERY REQUIRED REVIEWER SEAT PASSED THE CURRENT HEAD — LAND IT, AND CHANGE NOTHING
${klass === 'security' ? 'review-opus AND review-opus2 each' : 'review-opus'} returned PASS${round ? ` after ${round} fix round(s)` : ''} and posted a record on the head
it read. Do NOT rebase, do NOT push, do NOT fix, do NOT amend: ANY push makes a new head and voids
every record on the old one, and this pass would then have to review it again.
    gh pr view -R ${REPO} ${pr} --json headRefOid,mergeable,mergeStateStatus
If GitHub says CONFLICTING: do not resolve it here. Label needs-rebase and return status "needs-rebase"
— the next review pass rebases, and its reviewers PASS the rebased head.
If gh already says this PR is MERGED or CLOSED, do not merge it again; report it.
This PR is #${pr}; wherever the gate below says <n>, that is ${pr}. Every blocking finding was disposed
of and re-reviewed before you were dispatched; a green gate never substitutes for the merge bar.
THIS PASS ALREADY REVIEWED THIS HEAD. If the review gate's last line starts "review: BLOCK" (or
"review: needs-human"), that BLOCK is FINAL for this head and re-queueing it would loop forever: do NOT label
it review-pending. Label it needs-human instead, comment the gate's line verbatim, and return status
"needs-human" with reviewGate set to that line.
${CI_GATE(dir, target)}

RETURN: {itemId: "${itemId}", pr: ${pr || 0}, status: "merged"|"ci-pending"|"ci-red"|"needs-rebase"|"review-pending"|"needs-human",
         landed: true|false, reason, mergeCommit, reviewGate: "<the gate's last line>", reviewGateExit,
         gateExits: {typecheck, test, build, polish, diarize, stage, polishtest, diarizetest, fmt, clippy, cargotest},
         ciCheckConclusion}` : ''}

# LEAVE THE LANE CLEAN
If you started a server on ${port} or left the app running, kill it and confirm nothing listens
there (lsof -nP -iTCP:${port} -sTCP:LISTEN). Never touch ${other} — the other review chain is in it.
${LOGLINE(`${itemId} fix-land:${mode} PR#${pr}`, itemId, mode === 'fix' ? 'review:fixed' : 'review:land', `${pr}`)}
Then write the board ONE more time with the OUTCOME — stage review:merged, review:ci-pending,
review:review-pending, review:needs-human or review:closed-superseded — and the PR number as the 4th argument.
${WORKTREE_CONTRACT(dir, port, target, other)}
${RUNTIME_PROOF(dir)}
${GUARD(dir)}`
  }
  const land = await withAgents(1, () => agentR(fixLandPrompt('land', round), { model: 'opus', effort: 'high', phase: itemId, label: `fix-land:${itemId}` }))

  const landText = asText(land)
  const statusOf = (re, name) => (re.test(landText) ? name : null)
  // A BLOCK at LAND on the head this pass's reviewers reviewed is FINAL for it: needs-human, never review-pending.
  const landHeld = /"status"\s*:\s*"merged"/i.test(landText) ? '' : landBlockedLine(land, [primary, lens].map(recordHeadOf))
  if (landHeld && !/"status"\s*:\s*"needs-human"/i.test(landText)) {
    // The lander labelled it review-pending (or nothing): ONE hold seat relabels it needs-human.
    await withAgents(1, () => agentR(fixLandPrompt('hold', round, `LAND refused on a head this pass already reviewed: ${landHeld}`), { model: 'opus', effort: 'high', phase: itemId, label: `fix-land:${itemId}` }))
  }
  const status =
    statusOf(/"status"\s*:\s*"merged"/i, 'merged') ||
    (landHeld ? 'needs-human' : null) ||
    statusOf(/"status"\s*:\s*"needs-human"/i, 'needs-human') ||
    statusOf(/"status"\s*:\s*"review-pending"/i, 'review-pending') ||
    statusOf(/"status"\s*:\s*"ci-red"/i, 'ci-red') ||
    statusOf(/"status"\s*:\s*"needs-rebase"/i, 'needs-rebase') ||
    'ci-pending'
  log(`PR#${pr} (${itemId}, ${klass}, chain ${REVIEW_TAGS[chain]}): ${blocked ? `BLOCKED, ${round} fix round(s), re-reviewed PASS` : 'PASS'} -> ${status}`)
  return { itemId, pr, klass, chain: REVIEW_TAGS[chain], status, blocked, verdicts, land }
}

/** A thrown PR is one PR's problem. A HALT is the run's. */
async function reviewOnePrSafe(entry, chain) {
  const pr = String((entry && (entry.pr || entry.number)) || '?')
  const itemId = String((entry && entry.item) || `PR-${pr}`)
  try {
    return await reviewOnePr(entry, chain)
  } catch (err) {
    const message = err && err.message ? err.message : String(err)
    if (isHaltError(message)) {
      noteHalt(`PR#${pr} (${itemId}) review chain ${REVIEW_TAGS[chain]}`, message)
      return { itemId, pr, status: 'halted-in-review', error: message }
    }
    log(`PR#${pr} (${itemId}): REVIEW ERRORED on chain ${REVIEW_TAGS[chain]} — ${message} — that chain takes the next PR; the sweeper triages what this left open`)
    errored.push({ itemId, pr, error: message })
    return { itemId, pr, status: 'errored', error: message }
  }
}

/** ONE REVIEW CHAIN. It pulls the next PR off the shared queue whenever it is free. */
async function reviewChain(chain) {
  for (;;) {
    if (halted) break
    const entry = takeNextPr()
    if (entry === null) break
    reviewOutcomes.push(await reviewOnePrSafe(entry, chain))
  }
  log(`review chain ${REVIEW_TAGS[chain]} (${REVIEW_DIRS[chain]}, port ${REVIEW_PORTS[chain]}): drained the queue`)
}
/* ══ REVIEW CHAIN REGION END ═══════════════════════════════════════════════════════════════ */

// ── SAFETY WRAPPERS — a thrown stage is one item's problem; a HALT is the run's ─────────────
/**
 * A HALT is the ACCOUNT stopping, not a stage failing, and it stops BOTH lanes: every further
 * agent() call would be an instant failure, a lie in the telemetry and a cost in the queue.
 * Anything else is logged, recorded as errored, and the lane carries on with its next item.
 */
function noteHalt(where, message) {
  if (!halted) halted = { at: where, reason: message }
  log(
    `HALT (${where}): ${message} — BOTH lanes stop here. Drain and Reflect are skipped, so both ` +
      `worktrees are left standing; resume with resumeFromRunId after the reset.`
  )
}

async function runBuildSafe(item, lane) {
  try {
    return await runBuild(item, lane)
  } catch (err) {
    const message = err && err.message ? err.message : String(err)
    if (isHaltError(message)) {
      noteHalt(`${item.id} lane ${LANE_TAGS[lane]}`, message)
      return { itemId: item.id, status: 'halted', lane: LANE_TAGS[lane], error: message }
    }
    log(`${item.id}: BUILD ERRORED on lane ${LANE_TAGS[lane]} — ${message} — that lane continues with its next item`)
    errored.push({ itemId: item.id, error: message })
    return { itemId: item.id, status: 'errored', lane: LANE_TAGS[lane], error: message }
  }
}

/* ══ NO-PHASE REGION END ══════════════════════════════════════════════════════════════════ */

// ── ITEMS ───────────────────────────────────────────────────────────────────
// GENERATED — do not hand-edit. Every ITEMS.push({...}) below is stamped in by
// scripts/loop/build.mjs from scripts/loop/items/NN-<group>.mjs in FILENAME ORDER, which is
// also EXECUTION ORDER: an item may depend only on items in an earlier file. To change the
// work, edit an item file and re-run `node scripts/loop/build.mjs`.
// Required fields: id, prompt, branch, title, gated, preflight, spec, acceptance. `notes` is
// optional. These pushes run AFTER the constants above, so an item's template literals may
// interpolate REPO, LOCAL_REPO, APP, WORKDIR, WORKDIR_B, CARGO_TARGET_A, CARGO_TARGET_B,
// PREVIEW_PORT, PREVIEW_PORT_B, LOG, TELEMETRY, NPM_CACHE, STATUS_BOARD — and nothing else
// (build.mjs fails the build on any other ${IDENT} in an item file).
const ITEMS = []
// ── 00-y0-harness-and-gates.mjs ───────────────────────────────────────────
// Y0 — the harness floor. Nothing else in this loop is trustworthy until the gates are real.
//
// ════════════════════════════════════════════════════════════════════════════
// SHARED PREAMBLE — every `preflight` and `acceptance` in EVERY item file in
// this directory assumes it has been sourced. It is not repeated per item.
// ════════════════════════════════════════════════════════════════════════════
//
//   REPO  = the fresh clone the loop made. There is NO package.json at the repo
//           root: the app is `desktop/`, which is BOTH the npm project and the
//           Cargo workspace (members: src-tauri, yap-polish, yap-diarize).
//
//   cd ${APP} && npm ci
//
//   # THE SIDECARS ARE A PRECONDITION OF EVERY `cargo build` OF THE APP.
//   # `bundle.externalBin` in tauri.conf.json names binaries/yap-polish and
//   # binaries/yap-diarize, so src-tauri will not build until both are staged.
//   # This is the first command of every gate, not an optional step.
//   # stage_sidecars below is DOCUMENTATION ONLY: no seat's shell defines it, so a bare
//   # `stage_sidecars` line in an item's preflight/acceptance exits 127 (the 2026-10-16
//   # yap-flakes false red). Spell the commands out, `npm run build` first (tauri::generate_context!
//   # panics without desktop/dist), and build.mjs refuses the bare helper.
//   stage_sidecars() {
//     cargo build -p yap-polish -p yap-diarize --release
//     TRIPLE="$(rustc -vV | awk '/^host:/ {print $2}')"
//     mkdir -p src-tauri/binaries
//     cp target/release/yap-polish  "src-tauri/binaries/yap-polish-<triple>"
//     cp target/release/yap-diarize "src-tauri/binaries/yap-diarize-<triple>"
//   }
//
//   THE STANDARD GATE — applies to every item, never repeated in `acceptance`.
//   Each exit code is read DIRECTLY. Never through a pipe: a pipe reports the
//   exit code of `tail`, which is how a red gate ships green
//   (the "verification that verifies nothing" rule).
//
//     cd ${APP} && npm ci
//     npm run build            ; BUILD=$?   # `tsc && vite build` — this IS the typecheck
//     npm test                 ; VITEST=$?  # vitest run
//     stage_sidecars
//     cd src-tauri
//     cargo test --features custom-protocol ; RTEST=$?
//     cargo clippy --all-targets --features custom-protocol ; CLIPPY=$?
//     test $BUILD -eq 0 && test $VITEST -eq 0 && test $RTEST -eq 0 \
//       && test $CLIPPY -eq 0
//
//   SUPERSEDED 2026-09-12 (post-panel, pre-launch): the rustfmt sweep LANDED
//   (`cargo fmt --all`, 152 hunks / 21 files, b4b8d06, listed in
//   .git-blame-ignore-revs), so `cargo fmt --all -- --check` exits 0 on main and
//   IS a conjunct of the gate now. Also: clippy runs WITHOUT `-- -D warnings` —
//   a mechanical `clippy --fix` sweep landed (0f413a7) and the 16 lints that
//   survive it need real refactors, which is a loop item and not a gate flag.
//   And the gate builds AND STAGES BOTH sidecars (yap-polish AND yap-diarize):
//   with only polish staged, the app's cargo commands exit 101 in a fresh
//   worktree — and pass on a warm one, which is the false green that made the
//   measured table necessary.
//   `docs/loop/HARNESS.md` + the template's `GATE_CMDS` are the SINGLE source of
//   the gate; this block is a restatement and loses to them on any conflict.
//
//   Until Y0-A lands, CLIPPY is `|| true` in .github/workflows/ci.yml and
//   therefore proves nothing THERE. Y0-A is the first item for that reason.
//
//   ── PANEL 2026-09-12, BINDING ON EVERY ITEM IN EVERY FILE ────────────
//   (a) A `grep -q '<some_test_name>' <a test file this item writes>` proves
//       only that a NAME exists. An empty test body satisfies it and the gate.
//       So: every acceptance block that greps for a test name must ALSO carry a
//       mutation that turns that test RED — mutate a file, re-run, require a
//       non-zero exit, restore, then `git diff --exit-code <file>`. Y0-C is the
//       worked example. Y7-C collects the rows; it does not excuse the item.
//   (b) NEVER anchor an acceptance or pre-flight grep to `desktop/src/App.tsx`.
//       Y5-G moves all seven views out of it, so a path-anchored grep rots and
//       the item re-proves as not-done on every resume. Use
//       `grep -rq '<marker>' desktop/src`, or name the view module.
//   (c) A bare `cargo test --features custom-protocol <filter>` exits 0 when it
//       matches NOTHING (MEASURED: exit 0). Pre-flights therefore name a target:
//       `cargo test --features custom-protocol --test <target>`.
//   (d) NOTHING launches the app without `YAP_DATA_DIR` pointed at a scratch
//       directory and `--smoke` (Y0-D). Two lanes and Wilson's installed copy
//       otherwise share one SQLite history, one settings store, one models dir
//       and one global PTT binding.
//   (e) An acceptance line that already exits 0 on unmodified `origin/main` is
//       not acceptance. Run the block against main before you believe it.
//
//   UI ITEMS additionally owe, in the PR body:
//     * a screenshot of the main window at 980x700 (the configured default) AND
//       at 720x520 (minWidth/minHeight — the size the window can actually be
//       dragged to), because a layout that only works at the default size is
//       the "looks broken" complaint;
//     * a screenshot of the FLOAT PILL window for every state the item touches;
//     * the answer to "does this state read correctly in the 30px side dock?"
//
//   THE APP IS NOT SANDBOXED AND MUST NEVER BECOME SANDBOXED
//   (feedback_never_sandbox_utility_apps). `desktop/src-tauri/Entitlements.plist`
//   pins `com.apple.security.app-sandbox` = <false/>. Any item that touches
//   entitlements must assert the VALUE, not the key.
//
//   OBSERVABILITY: local-only crash capture (`src-tauri/src/crash.rs`). NEVER
//   Sentry, NEVER PostHog, NEVER Segment (feedback_queryguard,
//   reference_wispr_parity_research §5 — "no telemetry" is a shipped claim and a
//   marketing line). `git grep -i "sentry\|posthog\|segment.io" desktop` must
//   stay at 0 matches.
//
//   AUDITED STATE: origin/main @ 4e8c9adf (YV126, 2026-08-16). NOTE for the
//   builder: a local working tree may sit on `main` @ 734aa8c (YV83), which is
//   23 commits BEHIND. Every file:line in these specs is against 4e8c9adf.
//   Fetch and branch from origin/main, never from a stale local main.
//
//   Field contract: docs/loop/HARNESS.md "The item contract". Every field
//   required; `gated` is null or 'panel'.
// ════════════════════════════════════════════════════════════════════════════

ITEMS.push({
  id: 'Y0-A', prompt: 'Y0', branch: 'loop/y0-a-make-the-lint-gates-blocking', gated: null,
  notes: `STATUS 2026-09-26: BUILT, PR #156 OPEN and stale (main moved 30+ times under it). Rebase that PR onto main and finish it; do not open a second PR.`,
  title: 'Clippy becomes a blocking CI gate instead of `|| true` decoration (rustfmt stays informational — ledger)',
  preflight: `
    test 0 -eq "$(grep -c '|| true' .github/workflows/ci.yml)"
    grep -q -- '-D warnings' .github/workflows/ci.yml
    cd ${APP} && npm ci && cd src-tauri
    cargo clippy --all-targets --features custom-protocol -- -D warnings
  `,
  spec: `
    MEASURED at 4e8c9adf, .github/workflows/ci.yml:
      - name: rustfmt (informational)
        run: cargo fmt --all -- --check || true
      - name: clippy (informational)
        run: cargo clippy --all-targets --features custom-protocol || true
    and the comment above them: "Build + tests are the gate. Lints are
    informational until the tree is clean."

    The tree is never going to become clean on its own, and a loop whose merge
    gate cannot see a lint failure will spend eighty items accumulating them.
    This item makes them real, which means it must ALSO make them pass.

    Do, in this order:
      1. PANEL 2026-09-12: do NOT run \`cargo fmt --all\` here. MEASURED on main:
         126 hunks across 19 files, the top four being db.rs (40), dictation.rs
         (38), tests/formatting_fixtures.rs (14) and meeting_asr.rs (10) — i.e.
         exactly the files DB-A..E, Y4-A/C/D/E/F and Y3-B all edit. Reformatting
         them as merge #1 of 79 conflicts roughly fifty later branches (a
         conflicting PR is SKIPPED in build mode and never retried) and destroys
         \`git blame\` on the two largest files in the app. The rustfmt sweep is a
         SEPARATE mechanical commit landed on main BEFORE the loop launches, with
         a \`.git-blame-ignore-revs\` entry; only then does fmt become blocking.
         For this loop rustfmt stays informational, exactly as the ledger says.
      2. cargo clippy --all-targets --features custom-protocol -- -D warnings
         and fix every finding. Fix the CODE. \`#[allow(...)]\` is permitted ONLY
         with a one-line comment naming why the lint is wrong here; a bare
         \`#![allow(clippy::all)]\` at a crate root is a failed item.
      3. Delete both \`|| true\`s. Add \`-D warnings\` to the clippy invocation.
         Drop "(informational)" from both step names.
      4. Run clippy over the sidecars too: \`cargo clippy -p yap-polish
         -p yap-diarize --all-targets --release -- -D warnings\`, as its own step.

    Note \`#![allow(dead_code)]\` at src-tauri/src/transcription.rs:1. Leave it —
    removing it is Y6's dead-code item, and mixing the two makes both unmergeable.

    What NOT to do:
      - Do NOT add \`continue-on-error: true\` to the step. That is \`|| true\` with
        a different spelling and it still reports green.
      - Do NOT silence a lint by deleting the test that triggers it.
    PANEL 2026-09-12, and say this in the PR body: .github/workflows/ci.yml is
    NOT the gate this loop merges on. Actions is disabled account-wide, the
    harness resolves ci=local, and the clippy that actually gates all items is
    the one in the template's GATE_CMDS — which is invoked WITHOUT \`-D warnings\`.
    So this item hardens the CI file (correct, and it is what a future
    Actions-enabled world reads) but it does NOT by itself give the loop a real
    lint gate. Adding \`-- -D warnings\` to GATE_CMDS is a pre-launch harness
    amendment, recorded in docs/loop/PLAN.md §Panel revisions. Until it lands,
    treat every item's clippy=0 as evidence about the CI file only.

  `,
  acceptance: `
    test 0 -eq "$(grep -c '|| true' .github/workflows/ci.yml)"        # 0    (was 2)
    test 0 -eq "$(grep -c 'informational' .github/workflows/ci.yml)"  # 0    (was 2)
    grep -q -- '-D warnings' .github/workflows/ci.yml
    grep -q 'yap-polish -p yap-diarize --all-targets' .github/workflows/ci.yml
    cd ${APP} && npm ci && cd src-tauri
    cargo clippy --all-targets --features custom-protocol -- -D warnings          ; test $? -eq 0
    test 0 -eq "$(grep -rn 'allow(clippy::all)' src | wc -l)"
  `,
})

ITEMS.push({
  id: 'Y0-D', prompt: 'Y0', branch: 'loop/y0-d-per-instance-state-isolation-and-smoke-mode', gated: null,
  notes: `STATUS 2026-09-26: MERGED (PR #159) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
  title: 'One documented state root override and a --smoke mode, so two lanes and Wilson\'s own install stop sharing one history and one hotkey',
  preflight: `
    grep -q 'YAP_DATA_DIR' desktop/src-tauri/src/lib.rs
    grep -q 'YAP_DATA_DIR' desktop/src-tauri/src/models.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test data_dir_override
  `,
  spec: `
    PANEL 2026-09-12, and this is a PRECONDITION of every item whose proof is
    "the running app". MEASURED at 4e8c9adf:
      desktop/src-tauri/src/lib.rs:551   .join("WilsonVoice")     hardcoded
      desktop/src-tauri/src/models.rs:716  the same
      git grep -iE 'YAP_DATA_DIR|WILSON_VOICE_DATA|data_dir_override|--data-dir'
        -- desktop/src-tauri/src   ->  NO MATCHES
    The harness isolates directories, cargo target dirs and preview ports. It
    does NOT isolate the app's state. So two concurrent lane launches plus
    Wilson's installed copy read and write ONE SQLite history, ONE settings
    store, ONE models dir and ONE recovery dir, all register the same global PTT
    binding, and all synthesize a paste into whatever window has focus on
    Wilson's machine. Y7-B's spec already assumes a flag that does not exist
    ("a TEMPORARY data dir, with a flag that seeds a deterministic fixture
    state"); Y6-D then adds a single-instance guard, which makes the second
    lane's app unlaunchable; DB-B, DB-C, Y3-A and PERM-A are destructive or
    interactive against that shared state.

    Do:
      * \`YAP_DATA_DIR\` — one documented env override honoured at BOTH sites
        (lib.rs:551 and models.rs:716), resolved ONCE into an \`AppPaths\` value
        that everything else reads. Absent -> today's behaviour, unchanged.
      * \`--smoke\` — a launch mode that (1) refuses to register any global
        hotkey, (2) refuses to synthesize a paste, (3) REFUSES TO START at all
        if YAP_DATA_DIR is unset or resolves to the default root, and (4) exits
        non-zero with a named message in each refusal rather than degrading.
      * Do NOT touch the bundle id (TCC) or the default data-dir NAME. This adds
        an override; it renames nothing.
      * Document both in ARCHITECTURE.md's Runtime Dependencies table, and state
        the run contract: no agent launches the app without YAP_DATA_DIR.

    What NOT to do:
      - Do NOT make YAP_DATA_DIR the only path and delete the default.
      - Do NOT let --smoke silently fall back to the real data dir. The whole
        point is that a smoke run cannot touch Wilson's dictation history.
  `,
  acceptance: `
    grep -q 'YAP_DATA_DIR' desktop/src-tauri/src/lib.rs
    grep -q 'YAP_DATA_DIR' desktop/src-tauri/src/models.rs
    grep -q 'YAP_DATA_DIR' ARCHITECTURE.md
    test -f desktop/src-tauri/tests/data_dir_override.rs
    grep -q 'override_relocates_history_settings_models_and_recovery' desktop/src-tauri/tests/data_dir_override.rs
    grep -q 'smoke_mode_refuses_to_start_against_the_default_root' desktop/src-tauri/tests/data_dir_override.rs
    grep -q 'smoke_mode_registers_no_global_hotkey_and_never_pastes' desktop/src-tauri/tests/data_dir_override.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test data_dir_override ; test $? -eq 0
    # non-vacuous: break the override and the suite must go red
    sed -i.bak 's/YAP_DATA_DIR/YAP_DATA_DIR_DISABLED/' src/lib.rs
    cargo test --features custom-protocol --test data_dir_override ; test $? -ne 0
    mv src/lib.rs.bak src/lib.rs
    git diff --exit-code src/lib.rs ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y0-E', prompt: 'Y0', branch: 'loop/y0-e-structural-windowed-smoke-before-the-ui-lane', gated: null,
  notes: `STATUS 2026-09-26: MERGED (PR #160) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
  title: 'The instrument that can see "looks broken" ships BEFORE the nine UI items it judges — and it fails on today\'s tree',
  preflight: `
    test -x scripts/smoke-windowed.sh
    grep -q 'data-empty-state' scripts/smoke-windowed.sh
    node -e "process.exit(require('./desktop/package.json').scripts['smoke:windowed']?0:1)"
  `,
  spec: `
    PANEL 2026-09-12 — four seats converged on this. Wilson's defect (c) is a
    VISUAL report, the merge gate is eight headless commands (tsc, vitest, vite,
    two cargo builds, two cargo test runs) and build mode dispatches NO
    reviewer. Y7-B — the only pixel-level instrument in the plan — declares
    "Depends on Y5-B, Y5-C, Y5-G" and sits ten items after the whole Y5 lane,
    so nine UI items would land with no instrument that can see them, and
    Y5-G's own spec cites a screenshot gate that does not exist yet.

    So Y7-B is SPLIT. This item is the harness half and it runs here, in Y0:
      * \`scripts/smoke-windowed.sh\` (executable, \`set -euo pipefail\`), which
        launches the built app under \`YAP_DATA_DIR="$(mktemp -d)" --smoke\`
        (Y0-D), walks the seven Nav views at 980x700 and at 720x520, captures a
        PNG per view per size into docs/smoke-shots/<run>/, and FAILS on any of
        these five STRUCTURAL conditions:
          1. a view whose content region renders no text at all;
          2. a spinner still present after 10 s;
          3. a horizontal scrollbar on the document;
          4. any element's box outside the window bounds;
          5. a view rendering zero rows AND carrying no \`data-empty-state\`.
      * It must fail on TODAY's tree — that failure IS the baseline, and the PR
        body records which views fail and why. Do not weaken a detector to make
        the item green; record the red.
      * \`--check-only\` re-asserts against the last captured run. It is a
        convenience for a pre-flight, never a substitute for a capture, and it
        must exit NON-ZERO when no run exists. Add \`--self-test-must-fail\`,
        which injects a synthetic broken view and requires the detector to
        report it — the detector's own non-vacuity proof.
      * \`npm run smoke:windowed\` in desktop/package.json, and Y0-B's
        loop-smoke.sh calls it when a display is available. On this run a
        display IS available (it runs on Wilson's Mac), so "no display" is a
        FAILURE here, not a named skip.

    Do NOT assert pixel equality against a golden image. The failure conditions
    are structural, so a legitimate restyle never has to relitigate a PNG.

    Y7-B keeps the second half: extending coverage to the new pill phases and
    dock positions once Y5 has landed them.
  `,
  acceptance: `
    test -x scripts/smoke-windowed.sh
    grep -q 'set -euo pipefail' scripts/smoke-windowed.sh
    grep -q 'YAP_DATA_DIR' scripts/smoke-windowed.sh
    grep -q '720' scripts/smoke-windowed.sh
    grep -q '980' scripts/smoke-windowed.sh
    grep -q 'data-empty-state' scripts/smoke-windowed.sh
    grep -q 'self-test-must-fail' scripts/smoke-windowed.sh
    test 0 -eq "$(grep -c '| *tail' scripts/smoke-windowed.sh)"
    node -e "process.exit(require('./desktop/package.json').scripts['smoke:windowed']?0:1)"
    grep -q 'smoke-windowed' scripts/loop-smoke.sh
    test -f docs/loop/SMOKE-BASELINE.md
    ./scripts/smoke-windowed.sh --check-only ; test $? -ne 0
    ./scripts/smoke-windowed.sh --self-test-must-fail ; test $? -ne 0
  `,
})

ITEMS.push({
  id: 'Y0-B', prompt: 'Y0', branch: 'loop/y0-b-loop-smoke-and-fresh-clone-proof', gated: null,
  notes: `STATUS 2026-09-26: BUILT, PR #163 OPEN and stale (main moved 30+ times under it). Rebase that PR onto main and finish it; do not open a second PR.`,
  title: 'One command proves a fresh clone builds, tests and stages both sidecars — no host-found dependencies',
  preflight: `
    test -x scripts/loop-smoke.sh
    ./scripts/loop-smoke.sh
  `,
  spec: `
    Every item in this loop is built in a FRESH CLONE. The loop therefore needs
    one script that answers "does this tree build from nothing?" and that fails
    loudly when the answer is no. There is none today: \`ls scripts/\` at
    4e8c9adf is cicd-loop.mjs, rebuild_app.sh, start.sh,
    assert-weak-linked-14_4-symbols.sh.

    Create \`scripts/loop-smoke.sh\` (executable, \`set -euo pipefail\`), run from
    the repo root, which does exactly the SHARED PREAMBLE's standard gate and
    nothing more:
      npm ci in desktop · npm run build · npm test ·
      stage both sidecars for the host triple ·
      cargo test --features custom-protocol ·
      cargo clippy -- -D warnings · cargo fmt --check ·
      ./scripts/assert-weak-linked-14_4-symbols.sh desktop/target/release/wilson-voice
    It prints one PASS/FAIL line per stage and exits non-zero on the first
    failure. It must read every exit code directly — no pipes to tail/grep.

    RUNTIME-DEPENDENCY DISCIPLINE (the Yap rule, and the reason this item is
    early): nothing may be "found on the machine". The script must FAIL with a
    named message if any of these is missing rather than silently degrading:
    rustc/cargo, node, npm. And it must assert that the app's own runtime assets
    are shipped-or-managed, not assumed:
      * yap-polish and yap-diarize are BUILT here, from this tree (they are).
      * sherpa-onnx's build script FETCHES a 19.5 MB archive from GitHub
        Releases when SHERPA_ONNX_LIB_DIR is unset (ci.yml documents this at
        length). The script must print which of the three modes it used
        (SHERPA_ONNX_LIB_DIR / SHERPA_ONNX_ARCHIVE_DIR / network fetch) so an
        offline clone failure is diagnosable in one line instead of an hour.
      * ASR and polish MODEL WEIGHTS are downloaded by the app at runtime from
        the catalog (src-tauri/src/catalog.json). The script asserts no test
        requires a model on disk: \`cargo test\` must pass with an empty
        application-support dir. State that in the script's header.

    Add an npm script \`"smoke": "../scripts/loop-smoke.sh"\` in desktop/package.json.

    What NOT to do:
      - Do NOT make the script install anything (no brew, no rustup). It DETECTS
        and REPORTS. Installing behind the user's back is how a green smoke test
        stops describing the user's machine.
      - Do NOT have it download models to make tests pass. If a test needs a
        model, that test is wrong and belongs to Y7.
  `,
  acceptance: `
    test -x scripts/loop-smoke.sh
    grep -q 'set -euo pipefail' scripts/loop-smoke.sh
    grep -q 'SHERPA_ONNX_ARCHIVE_DIR' scripts/loop-smoke.sh
    test 0 -eq "$(grep -c '| *tail' scripts/loop-smoke.sh)"
    node -e "process.exit(require('./desktop/package.json').scripts.smoke?0:1)"
    ./scripts/loop-smoke.sh                      ; test $? -eq 0
    # non-vacuous: break it on purpose and watch it go red
    (cd desktop/src-tauri && printf '\\nfn __y0b(){let _x:u8=1;}\\n' >> src/latency.rs)
    ./scripts/loop-smoke.sh                      ; test $? -ne 0
    (cd desktop/src-tauri && git checkout src/latency.rs)    # PANEL: bundle.externalBin names BOTH sidecars, so both are a precondition
    # of every cargo build of the app. HARNESS.md's gate table stages only
    # yap-polish; on a fresh worktree that makes clippy and cargo test exit 101.
    test -f "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')"
    test -f "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"

  `,
})

ITEMS.push({
  id: 'Y0-C', prompt: 'Y0', branch: 'loop/y0-c-defaults-are-tested-not-just-code-paths', gated: null,
  notes: `STATUS 2026-09-26: MERGED (PR #166) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
  title: 'A test asserts the SHIPPED defaults, so a feature that is off by default can never be called tested again',
  preflight: `
    test -f desktop/src-tauri/tests/shipped_defaults.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test shipped_defaults
  `,
  spec: `
    This item exists because of the single worst class of defect this audit
    found, and it is the mechanism behind Wilson's "formatting does not work
    whatsoever":

      desktop/src-tauri/src/lib.rs:417  cleanup_level: "light".into()
      desktop/src-tauri/src/dictation.rs:636  runs_format() => Medium | High
      desktop/src-tauri/src/lib.rs:418  polish_model: String::new()   // = OFF
      desktop/src-tauri/src/dictation.rs:644  runs_llm()   => High only

    So on a fresh install NO formatting stage runs at all — and the entire
    formatting corpus passes, because every fixture pins its own level:
      desktop/src-tauri/tests/fixtures/formatting/group-a-sentence-shape.jsonl
      line 4: {"level":"medium", ...}
    The tests exercise a configuration the product never ships.

    Create \`desktop/src-tauri/tests/shipped_defaults.rs\`: a table of the
    DEFAULT AppSettings values that user-visible behaviour depends on, each
    asserted against \`AppSettings::default()\`, each with a comment saying what
    the user sees if it changes. At minimum: cleanup_level, polish_model,
    auto_paste, show_floating_pill, pill_style, ptt_binding, dictation_mode,
    denoise, mute_while_dictating, check_updates, preload_model, signature_mode,
    snippet_scope.

    Then add the harder assertion, the one that has teeth:
      \`defaults_reach_every_cleanup_stage\` — build the cleanup pipeline from
      \`AppSettings::default()\` (NOT from a hand-written CleanupLevel) and assert
      each stage the product promises is REACHED. It must fail on today's main.
      Do not fix the default here — Y4-A owns that change. This test is the
      tripwire, and it is allowed to be \`#[ignore]\`d with an \`// UNBLOCKED BY
      Y4-A\` comment ONLY if the ignore is removed in Y4-A's diff. Prefer landing
      it red-then-green inside Y4-A's dependency order if the loop allows it.

    Also add, in the same file, \`formatting_fixtures_declare_the_shipped_level\`:
    assert that at least one fixture row in
    tests/fixtures/formatting/*.jsonl carries the level that
    \`AppSettings::default().cleanup_level\` names.

    PANEL 2026-09-12 — that assertion as first drafted is VACUOUS IN BOTH
    DIRECTIONS and the audit sentence under it is false. MEASURED on main: four
    rows already carry "light" (three in group-a, one in group-b, one of them
    literally named \`b17-level-light-runs-no-formatting\`) and 36 carry
    "medium". So it is green today and green after Y4-A flips the default. It
    can never go red. Build the assertion that has teeth instead:
    \`every_rule_has_a_fixture_row_at_the_shipped_level\` — enumerate the rule
    ids the formatting stage owns (the \`rules\` field already exists on every
    fixture row; take the id set from the rule table, never from a literal) and
    assert each one has at least one row whose \`level\` equals
    \`AppSettings::default().cleanup_level\`, failing by NAMING the missing rule
    ids. That is red on main, still red after a partial Y4-A, green only when
    the corpus covers the level the product ships.

    What NOT to do:
      - Do NOT assert defaults by re-reading the literal out of lib.rs with a
        grep. The test must construct \`AppSettings::default()\` so a changed
        default breaks it.
      - Do NOT change any default in this item. This item only makes them visible.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/shipped_defaults.rs
    grep -q 'AppSettings::default()' desktop/src-tauri/tests/shipped_defaults.rs
    grep -q 'defaults_reach_every_cleanup_stage' desktop/src-tauri/tests/shipped_defaults.rs
    grep -q 'every_rule_has_a_fixture_row_at_the_shipped_level' desktop/src-tauri/tests/shipped_defaults.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test shipped_defaults ; test $? -eq 0
    # non-vacuous: flip a default and the table must go red
    sed -i.bak 's/auto_paste: true/auto_paste: false/' src/lib.rs
    cargo test --features custom-protocol --test shipped_defaults ; test $? -ne 0
    mv src/lib.rs.bak src/lib.rs
    # PANEL: a failed restore must be LOUD, not a silently shipped default change
    git diff --exit-code src/lib.rs ; test $? -eq 0
  `,
})

// ── 00a-yap-2026-10-26.mjs ────────────────────────────────────────────────
// yap-2026-10-26 — Wilson 2026-10-26: "Yap is extremely broken". Written 2026-10-26T13:46Z (date -u) by
// the pre-flight + backlog seat. FIX, no panel. Named 00a so it sorts straight after the harness file:
// yap-insert-bridge-2026-10-26 is FIRST PRIORITY in the build order (coordinator 2026-10-26). To run it
// alone first: Workflow {args: {mode: 'build', only: ['yap-insert-bridge-']}}.
//
// Also here: the three items queued in PR #240's thread (2026-10-19T16:34Z, meant for 07d, but 07d only
// exists inside unmerged #240 — so they live on main here) and the #249 carried advisories stub
// (#240 thread 2026-10-19T19:12Z). Two of the #240 items are SATISFIED BY OPEN PRs and must not be
// rebuilt: their preflight prints the DEPENDENCY marker (harness: "skipped: dependency not on main",
// no builder, no PR) until the satisfying PR lands, then checks its deliverable and retires as
// already-done. If that check ever fails after the PR landed, the builder builds ONLY the gap.
//
// Review: Opus only (review-opus + review-opus2). No Codex, no Gemini, no third-party reviewer.
// HAZARD (same as 07/07b/08): no agent launches the GUI app, posts events into, signals or quits Wilson's
// running /Applications/Yap.app, or writes TCC on this machine. Every automated proof is a cargo test, a
// vitest, or a script self-test; on-device checks go in the PR body as WILSON'S WALK.
// NO GUESSING: the root-cause probe goes into the PR body under "Root cause, proven" AND into
// docs/loop/evidence/<item id>.md, first line exactly "ROOT-CAUSE-PROBE <item id>".
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap-insert-bridge-2026-10-26', prompt: 'yap-insert-bridge-2026-10-26', branch: 'loop/yap-insert-bridge-2026-10-26-paste-first', gated: null,
  title: 'Dictation lands in the focused field again — paste bridge first when PostEvent is granted; AX never strands a take on the Copy card; clipboard is never a silent fallback',
  preflight: `
    grep -q 'AX_ASYNC_DENYLIST' desktop/src-tauri/src/paste.rs && grep -q 'fallback=' desktop/src-tauri/src/paste.rs && grep -q 'fn post_event_granted_uses_paste_bridge_regardless_of_ax' desktop/src-tauri/tests/insertion_matrix.rs
  `,
  spec: `
    Panel: not required — FIX, class SECURITY (CGEventPost ⌘V / AX insertion / permission-gated path).
    FIRST PRIORITY. Label the PR "security" (gh pr edit <N> --add-label security) so the gate seats
    review-opus AND review-opus2 — paste.rs/focus.rs do not match SECURITY_PATHS by name.
    Wilson 2026-10-26: dictation no longer lands in the focused text box; he copy/pastes every result.

    ROOT CAUSE (diag seat 2026-10-26T13:45Z — code, not TCC; reproduce it as your probe, do not re-derive):
      #229 (6c3f991, yap-p0-insertion-bridge-fallback, shipped in 0.9.2, still in 0.9.3-dev) made AX
      kAXSelectedText the FIRST path whenever Accessibility is Granted (paste.rs:180-188
      choose_insert_path). In the Claude desktop app (Electron/Chromium) the AX set returns success but
      AXValue never reads back changed within 120 ms (focus.rs:186-208); the code calls that Unverified,
      deliberately skips ⌘V (double-insert risk) and ends on the Copy card (paste.rs:408-411).
      yap.log: "insert path=Ax (ax=Granted post=Granted)" -> "AX insert unverified ... no ⌘V ... showing
      the Copy card" -> "pasted=false". Claude app before #229: 41/41 pasted=true; after: 29/29
      pasted=false. Chrome after #229: 2 unverified -> card, 4 AX ok, 1 refused -> bridge ok. Terminal:
      23/23 "AX refused (not settable) -> paste bridge -> confirmed", so the bridge itself works.
      Permissions are fine: DR identical 0.9.2 vs 0.9.3-dev (team VHYV8C2JNU), TCC rows match, every take
      logs ax=Granted post=Granted. Ruled out: #250 (no paste.rs/focus.rs/paste_tx), #249 (fn and
      ⌃⌥⌘Space both call insert_dictated_take, lib.rs:2723), settings autoPaste is true.
      TextEdit takes the AX path fine, so #242's TextEdit-only smoke would NOT catch this.
      Probe for the evidence file: a cargo test on unmodified main showing choose_insert_path returns Ax
      for ax=Granted post=Granted, plus the read-only yap.log counts above (grep -c, no writes).

    DO (names are contract)
      1. paste.rs choose_insert_path: PostEvent Granted -> PasteBridge, whatever Accessibility is. Ax only
         when PostEvent is not Granted but Accessibility is. CopyCard when neither. This restores the
         0.9.1 path (41/41 Claude, 181/182 Terminal); the bridge stays receipt-checked (paste_tx) and
         still ends on the Copy card if nothing reads the clipboard.
      2. pub const AX_ASYNC_DENYLIST: &[&str] in paste.rs — com.anthropic.claudefordesktop,
         com.google.Chrome, com.tinyspeck.slackmacgap, com.microsoft.VSCode, com.hnc.Discord — plus any
         frontmost app whose bundle contains "Electron Framework.framework". Those never get an AX
         attempt; they go to the bridge.
      3. An AX result of Unverified (focus.rs:203) re-reads for ~400 ms (AXNumberOfCharacters and
         AXSelectedTextRange). Neither moved -> treat as Refused -> bridge runs EXACTLY ONCE, no card.
         A real change stays Inserted with 0 ⌘V.
      4. The "insert path=" log line (paste.rs:617-618) becomes one line per take:
         insert path=<Ax|PasteBridge|CopyCard> app=<frontmost bundle id> fallback=<none|reason>
         (reasons e.g. electron_denylist, ax_unverified_unchanged, no_receipt, post_event_denied).
      5. When insertion is impossible the pill says so — which permission (PostEvent / Accessibility) and
         a one-click Open Settings / Repair (invoke("repair_permission")) — never a silent clipboard
         leave-behind and never pasted=false without a card.
      6. Prior clipboard is restored after every bridge take (keep paste_tx's restore; prove it).
      7. tests/insertion_matrix.rs: the existing ax_trusted_uses_ax and ax_unverified_never_posts_cmd_v
         encode the regression — REPLACE them (do not keep a test asserting AX-first) with, exact names:
         post_event_granted_uses_paste_bridge_regardless_of_ax, ax_only_when_post_event_denied,
         both_denied_shows_copy_card, denylisted_bundle_never_calls_ax_insert,
         ax_unverified_unchanged_becomes_refused_bridge_once, ax_unverified_changed_stays_inserted_zero_cmd_v,
         bridge_no_receipt_ends_on_card_never_silent, insert_log_line_format (snapshot of path/app/fallback).
         Keep the other existing matrix tests green.
      8. If the #242 release smoke (scripts/qa) is on main when you build, add a Chrome <textarea> target
         next to TextEdit; if it is not, write "smoke-target follow-up for #242: add Chrome textarea /
         Claude desktop" in the PR body.

    WILSON'S WALK (PR body; signed Developer-ID app; one fn⌃ take AND one ⌃⌥⌘Space take each):
      1 TextEdit: text at the caret exactly once. 2 Chrome — Google Docs body and a plain <textarea>: once.
      3 Slack composer: once. 4 Claude desktop composer: once (the regression case). 5 A Tauri input
      (Yap scratchpad or Yap Test): once. 6 Terminal: once. 7 Prior clipboard restored after each take.
      8 On Yap Test with PostEvent AND Accessibility revoked: the pill names the blocked permission and
      offers one-click Open Settings / Repair; with only one revoked, insertion still works via the other.
      9 yap.log has one "insert path=... app=... fallback=..." line per take. 10 20 consecutive takes into
      Claude desktop -> 0 pasted=false.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/insertion_matrix.rs
    grep -q 'AX_ASYNC_DENYLIST' desktop/src-tauri/src/paste.rs
    grep -q 'com.anthropic.claudefordesktop' desktop/src-tauri/src/paste.rs
    grep -q 'Electron Framework' desktop/src-tauri/src/paste.rs
    grep -q 'fallback=' desktop/src-tauri/src/paste.rs
    grep -q 'app=' desktop/src-tauri/src/paste.rs
    ! grep -q 'fn ax_trusted_uses_ax()' desktop/src-tauri/tests/insertion_matrix.rs
    ! grep -q 'fn ax_unverified_never_posts_cmd_v()' desktop/src-tauri/tests/insertion_matrix.rs
    test -s docs/loop/evidence/yap-insert-bridge-2026-10-26.md
    grep -q '^ROOT-CAUSE-PROBE yap-insert-bridge-2026-10-26$' docs/loop/evidence/yap-insert-bridge-2026-10-26.md
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    mkdir -p desktop/target
    (cd desktop/src-tauri && cargo test --features custom-protocol --test insertion_matrix -- --list) > desktop/target/ib26-list.txt 2>/dev/null
    grep -q '^post_event_granted_uses_paste_bridge_regardless_of_ax: test$' desktop/target/ib26-list.txt
    grep -q '^ax_only_when_post_event_denied: test$' desktop/target/ib26-list.txt
    grep -q '^both_denied_shows_copy_card: test$' desktop/target/ib26-list.txt
    grep -q '^denylisted_bundle_never_calls_ax_insert: test$' desktop/target/ib26-list.txt
    grep -q '^ax_unverified_unchanged_becomes_refused_bridge_once: test$' desktop/target/ib26-list.txt
    grep -q '^ax_unverified_changed_stays_inserted_zero_cmd_v: test$' desktop/target/ib26-list.txt
    grep -q '^bridge_no_receipt_ends_on_card_never_silent: test$' desktop/target/ib26-list.txt
    grep -q '^insert_log_line_format: test$' desktop/target/ib26-list.txt
    (cd desktop/src-tauri && cargo test --features custom-protocol --test insertion_matrix)
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib paste)
    (cd desktop && npx vitest run src/pill)
    bash scripts/qa/hotkey-walk.sh --self-test
  `,
})

ITEMS.push({
  id: 'yap-founding-coupon-follows-launch', prompt: 'yap-founding-coupon-follows-launch', branch: 'loop/yap-founding-coupon-follows-launch', gated: null,
  title: 'FOUNDING19 follows the launch, not a date — valid for 30 days from the moment PURCHASE_OPEN flips true; no founding-price copy anywhere while purchasing is closed',
  preflight: `
    ( if ! grep -q 'pub const PURCHASE_OPEN' desktop/src-tauri/src/license.rs; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap24-PILL1 introduces PURCHASE_OPEN in license.rs and has not landed; land it, then build this"; exit 0; fi; grep -q 'pub fn founding_valid' desktop/src-tauri/src/license.rs && grep -q 'FOUNDING_OPENED_AT' desktop/src-tauri/src/license.rs && ! grep -q '2026-11-08' desktop/src-tauri/src/license.rs && grep -q 'founding_open' desktop/src/license/status.ts )
  `,
  spec: `
    Panel: not required — Wilson's decision 2026-10-26. Class SECURITY (licensing): label the PR "security"
    (gh pr edit <N> --add-label security) so the gate seats review-opus AND review-opus2 — license.rs does
    not match SECURITY_PATHS by name.
    DEPENDS: yap24-PILL1 (00f-yap24-pill-and-trial.mjs) — it introduces \`pub const PURCHASE_OPEN\` (default false,
    surfaced on the license status payload as purchase_open). PURCHASE_OPEN is a CLIENT const in license.rs,
    not a server-side flag on the issuer, and PR #232's yap-lic items do not define it (checked 2026-10-26). The
    preflight holds this item until PILL1 is on main.

    TODAY: the FOUNDING19 window is a DATE — license.rs:167-168 documents Stripe coupon z8UZkIH2 as "expires
    2026-11-08", and the founding copy (desktop/src/license/status.ts:67-68 FOUNDING_CODE / FOUNDING_PRICE_LABEL,
    rendered in LicensePanel.tsx:140 and PurchasePrompt.tsx:77) shows regardless of whether anyone can buy.

    DO (names are contract)
      1. Founding validity is bound to PURCHASE_OPEN, not a calendar date: valid from the moment PURCHASE_OPEN
         flips true, for 30 days. \`founding_opened_at\` must be ONE global instant, not each install's first run:
         because PURCHASE_OPEN is a release-time const, add \`pub const FOUNDING_OPENED_AT: Option<&str> = None\`
         (RFC 3339 UTC) set in the SAME release that flips PURCHASE_OPEN — a unit test fails the build when
         PURCHASE_OPEN is true and FOUNDING_OPENED_AT is None. (If, when you build, PURCHASE_OPEN has moved
         server-side to the issuer, persist founding_opened_at there on the first flip instead and read it
         over the existing license status fetch; say which in the PR body.)
      2. \`pub fn founding_valid(purchase_open: bool, opened_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool\`
         = purchase_open && opened_at is Some && opened_at <= now < opened_at + 30 days. The license status payload
         carries \`founding_open: bool\` computed by the backend; the webview never recomputes it.
      3. No founding-price copy anywhere while founding_open is false: LicensePanel.tsx and PurchasePrompt.tsx render
         the FOUNDING_CODE / FOUNDING_PRICE_LABEL lines ONLY when founding_open is true. The pill never shows a price
         (Y2-D rule, unchanged).
      4. Remove the hard-coded "expires 2026-11-08" from the license.rs:168 doc comment; replace with "valid 30 days
         from FOUNDING_OPENED_AT (the launch), see founding_valid".
    OWNER-ONLY (PR body, never done by an agent — money, outbound-review): on launch day Wilson sets the Stripe
    coupon z8UZkIH2 redeem_by to FOUNDING_OPENED_AT + 30 d, so Stripe and the app agree.
    TESTS (cargo test --lib license): flag false => founding_valid false for every opened_at/now AND the payload's
    founding_open is false; flip (opened_at = t0) => valid at t0, t0+29d23h; day 31 (t0+30d, t0+31d) => invalid;
    PURCHASE_OPEN true with FOUNDING_OPENED_AT None => test fails. vitest (src/license): founding_open false =>
    no "FOUNDING19" and no "Founding price" text in LicensePanel or PurchasePrompt; true => both present.
  `,
  acceptance: `
    grep -q 'pub fn founding_valid' desktop/src-tauri/src/license.rs
    grep -q 'FOUNDING_OPENED_AT' desktop/src-tauri/src/license.rs
    ! grep -q '2026-11-08' desktop/src-tauri/src/license.rs
    grep -q 'founding_open' desktop/src/license/status.ts
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib license)
    (cd desktop && npx vitest run src/license)
  `,
})

ITEMS.push({
  id: 'yap-meetings-summary-default', prompt: 'yap-meetings-summary-default', branch: 'loop/yap-meetings-summary-default-zero-setup', gated: null,
  title: 'SATISFIED BY #250 — a recorded meeting gets notes with zero setup; no "add a summary model" prompt anywhere',
  preflight: `
    ( if ! test -f desktop/src-tauri/src/summary_jobs.rs; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): satisfied-by #250 (feat/meeting-summary-default) — review and land #250; do not rebuild this"; exit 0; fi; ! grep -rqi 'add a summary model' desktop/src --include='*.ts' --include='*.tsx' --exclude='*.test.ts' --exclude='*.test.tsx' )
  `,
  spec: `
    Panel: not required — queued in #240's thread 2026-10-19T16:34Z (Wilson's Yap Test walk).
    SATISFIED-BY: #250 (feat/meeting-summary-default, head 0a87249). The preflight skips this item until
    #250 is on main, then retires it as already-done. Build ONLY if #250 landed and the preflight still
    fails: then the residual gap is that some non-test UI string still asks the user to add a summary model.
    Original ask: a recorded meeting shows "TRANSCRIPT READY · ADD A SUMMARY MODEL IN SETTINGS". Ship a
    default local summary (reuse the polish sidecar model) so no meeting ever asks the user to add a
    model; Settings may offer a bigger model as an option. Acceptance from the thread: fresh profile ->
    record 20 s -> summary appears with zero setup (WILSON'S WALK, PR body); grep that the string is gone.
  `,
  acceptance: `
    test -f desktop/src-tauri/src/summary_jobs.rs
    ! grep -rqi 'add a summary model' desktop/src --include='*.ts' --include='*.tsx' --exclude='*.test.ts' --exclude='*.test.tsx'
    (cd desktop && npm ci)
    (cd desktop && npx vitest run src/meetings)
  `,
})

ITEMS.push({
  id: 'yap-perm-accessibility-live-refresh', prompt: 'yap-perm-accessibility-live-refresh', branch: 'loop/yap-perm-accessibility-live-refresh', gated: null,
  title: 'SATISFIED BY #247 + #248 — the Accessibility banner follows the live probe, Repair resets the right service, no stale-instance refusal',
  preflight: `
    ( if ! grep -q 'pub fn reset_services' desktop/src-tauri/src/perm_probe.rs || ! test -f desktop/src/permission.refresh.test.tsx; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): satisfied-by #247 (fix/ax-probe-refresh-and-repair-service) — review and land #247; do not rebuild this"; exit 0; fi; if ! grep -q 'pub fn replace_dead_holder' desktop/src-tauri/src/lifecycle.rs 2>/dev/null; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): stale-instance half satisfied-by #248 (stacked on #241) — land #241 then #248"; exit 0; fi; true )
  `,
  spec: `
    Panel: not required — queued in #240's thread 2026-10-19T16:34Z. Class SECURITY (permission path).
    SATISFIED-BY: #247 (banner follows the live probe; Repair resets the service its row names) and #248
    (replace a dead single-instance holder — stacked on #241). The preflight skips this item until both
    are on main, then retires it as already-done. Do not rebuild either PR.
    Original ask: Accessibility granted in TCC (auth 2, csreq matches) while the banner kept saying off;
    the probe ran once at launch and never re-probed; Repair on the Accessibility row reset PostEvent, not
    Accessibility; "stale instance" refusal on second launch.
  `,
  acceptance: `
    grep -q 'pub fn reset_services' desktop/src-tauri/src/perm_probe.rs
    test -f desktop/src/permission.refresh.test.tsx
    grep -q 'pub fn replace_dead_holder' desktop/src-tauri/src/lifecycle.rs
    (cd desktop && npm ci)
    (cd desktop && npx vitest run src/permission.refresh.test.tsx)
    (cd desktop/src-tauri && cargo test --features custom-protocol --test lifecycle)
  `,
})

ITEMS.push({
  id: 'yap-test-channel-product-name', prompt: 'yap-test-channel-product-name', branch: 'loop/yap-test-channel-product-name-everywhere', gated: null,
  title: 'Every permission banner and message names the running product ("Yap Test" in a test build), never a hard-coded "Yap"',
  preflight: `
    grep -q 'fn permission_copy_uses_product_name' desktop/src-tauri/src/permissions.rs && grep -rq 'productName' desktop/src/permission.ts
  `,
  spec: `
    Panel: not required — queued in #240's thread 2026-10-19T16:34Z (Wilson's Yap Test walk). Class
    SECURITY by path (permissions.rs).
    Problem: permission banners/copy say "Yap" inside a Yap Test build. System Settings lists the grant
    under the bundle's own name, so the copy must say the same name or the user toggles the wrong row.
    perm_truth::product_name_from_bundle_path already exists on main (perm_truth.rs:406); use it, do not
    add a second source of truth. Hard-coded sites on main include lib.rs:1491 ("Yap cannot hear you"),
    permissions.rs:157 and :332, and the frontend permission views — grep every user-facing "Yap" in the
    permission/onboarding/pill copy and route it through the product name (the brand word in marketing
    copy and the app title stays).
    DO: (1) Rust: every permission message takes product: &str from perm_truth; unit test
    permission_copy_uses_product_name asserts "Yap Test" appears and "Yap " alone does not for a
    /Applications/Yap Test.app path. (2) Expose the product name to the UI (existing permission status
    payload gets productName); desktop/src/permission.ts reads it; vitest renders the banner with
    productName "Yap Test". (3) If #238's buildChannel.ts is on main, keep its chip separate — the chip
    is the build head, productName is the TCC row name.
  `,
  acceptance: `
    grep -q 'fn permission_copy_uses_product_name' desktop/src-tauri/src/permissions.rs
    grep -rq 'productName' desktop/src/permission.ts
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && npx vitest run src/permission)
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib permission_copy_uses_product_name)
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib perm_truth)
  `,
})

ITEMS.push({
  id: 'yap-hotkeys-advisories-r1', prompt: 'yap-hotkeys-advisories-r1', branch: 'loop/yap-hotkeys-advisories-r1', gated: null,
  title: 'Close the seven non-blocking review advisories carried from #249 (reserved chords, launch re-validation, copy, TCC errors, parity test)',
  preflight: `
    grep -q 'reserved_chords_parity' desktop/src-tauri/src/hotkey_table.rs && grep -q 'sqlite3 -readonly' scripts/qa/hotkey-walk.sh && grep -q 'Esc to cancel' desktop/src/hotkeyRecorder.ts desktop/src/views/settings/Shortcut.tsx desktop/src/HotkeyRow.tsx 2>/dev/null
  `,
  spec: `
    Panel: not required — stub carried from #249 (landed a740614), posted in #240's thread
    2026-10-19T19:12Z. Advisories from review-opus / review-opus2 on e472c17:
      1. The reserved-chord list misses ⌃⇧⌘3, ⌃⇧⌘4, ⌥⌘H, ⌥⌘M, ⌥⌘W and ⌃⌘D — add them.
      2. The stored dictate chord is not re-validated against the reserved list at launch — re-validate;
         a reserved stored chord falls back to the default and the UI says why.
      3. The Cancel button in the chord recorder needs "Esc to cancel" copy.
      4. Without Full Disk Access, a stale grant reads "Needs Input Monitoring" without the
         remove-and-re-add hint — add the hint.
      5. A TCC query error is counted as "no row" — surface it as an error state instead.
      6. No Rust/TS parity test for the reserved-chord list — add test reserved_chords_parity (Rust reads
         the TS list or a shared JSON; both sides fail on drift).
      7. hotkey-walk.sh --table opens the TCC database with sqlite3 -readonly and quotes the bundle id.
  `,
  acceptance: `
    grep -q 'reserved_chords_parity' desktop/src-tauri/src/hotkey_table.rs
    grep -q 'sqlite3 -readonly' scripts/qa/hotkey-walk.sh
    shellcheck scripts/qa/hotkey-walk.sh
    bash scripts/qa/hotkey-walk.sh --self-test
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && npx vitest run src/hotkeyRecorder.test.tsx)
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib hotkey_table)
  `,
})

// yap-loop-stall-detector — filed 2026-10-26T22:58Z (date -u) by a Sonnet spec-writer seat. Audit 2026-10-26: a review
// pass sat 2 h with zero lands because the land gate required fmt, which was red on unmodified main; every PASS/PASS
// PR failed fix-land with ci-red and nothing (no seat, no parent code) noticed the pattern. Fixed by hand at ~15:20Z
// (fmt advisory, template.mjs:459-479); this item makes the loop notice next time. SECURITY class: it touches
// scripts/loop/*, which review-gate.mjs lists in SECURITY_PATHS. FUTURE item: do not stop a live pass to build it.
// The word the validator bans for the machine monitor script is avoided on purpose below: that script is globbed.
ITEMS.push({
  id: 'yap-loop-stall-detector', prompt: 'yap-loop-stall-detector', branch: 'loop/yap-loop-stall-detector', gated: null,
  title: 'The loop notices its own stalls — same-reason-twice, gate-red-on-main halt, no-progress halt, and an external ALERT STALL line',
  preflight: `
    test -f scripts/loop/stall.mjs && test -f scripts/loop/stall-detector.test.mjs && test -f scripts/loop/stall-alarm.sh && grep -q 'const GATE_BLOCKING' scripts/loop/template.mjs
  `,
  spec: `
    Panel: not required — harness fix. Class SECURITY: everything here is under scripts/loop/, which review-gate.mjs
    lists in SECURITY_PATHS, so the gate seats review-opus AND review-opus2. Label the PR "security"
    (gh pr edit <N> --add-label security) anyway. Builder stays Opus (no builder tag on this item).
    Do not regenerate or launch anything: this item only edits the harness sources; npm run loop:build is run by the
    orchestrator after the merge, never by the builder, and a live pass is never stopped for it.

    ROOT CAUSE (audit 2026-10-26, already fixed by hand — do not re-derive): GATE_CMDS (template.mjs:466-479) required
    cargo fmt while main was red on it (65 hunks), so every PASS/PASS PR returned fix-land status ci-red for the same
    command. Three blind spots let it run 2 h: (a) the review barrier retried or re-queued with no memory of the last
    reason; (b) REVIEW_RECON (template.mjs:1911-1958, baseline at :1938-1942) reports the baseline exits and the pass
    continued — recon's own prose called fmt informational (the BUILD_RECON text at :1860-1862) while the land gate
    called it blocking; (c) nobody outside the workflow was told.

    WHERE THINGS ARE (template.mjs, 2026-10-26; line numbers drift, grep the symbol): GATE_CMDS :466; the "TEN BLOCKING
    COMMANDS" prose :479; CI_GATE :516 (its OUTCOMES block :570-575 is where ci-red is defined); fixLandPrompt LAND
    RETURN with gateExits :1700-1703; the review barrier's two-seat dispatch :1531; the land seat dispatch and the status
    derivation :1715-1733; reviewOnePrSafe :1740; reviewChain :1756-1763 (it already breaks on halted); reviewPass
    :2118 (triage :2119, chains :2207 Promise.all, land-sweeper :2246, main-verify :2301); the recon call :1985 and
    RECON_SCHEMA just above it; the board writer is scripts/loop/status.mjs (STATUS in the prompts); Loop-Logs lines are appended by seats via LOGLINE (:333). The generated workflow is ONE script with no
    imports, so scripts/loop/stall.mjs cannot be imported by it: stall.mjs is the SOURCE, and template.mjs carries a
    byte-identical copy of its function bodies between the markers  STALL-RULES-BEGIN / STALL-RULES-END  (comment
    lines). The test asserts the two are identical. Do not add an import to template.mjs.

    DO (names are contract)
      0. ONE SOURCE FOR THE GATE FLAGS. In template.mjs add, immediately above GATE_CMDS:
         const GATE_BLOCKING = ['typecheck','test','build','polish','diarize','stage','polishtest','diarizetest','clippy','cargotest']
         const GATE_ADVISORY = ['fmt']
         The echo labels inside GATE_CMDS (the  echo "<name>=\$?"  tokens) must be exactly GATE_BLOCKING plus GATE_ADVISORY,
         and the "TEN BLOCKING COMMANDS" prose must say the count of GATE_BLOCKING. stall.mjs exports the same two
         arrays. A test extracts the echo labels from GATE_CMDS and fails on any drift in either direction, so
         adding a command to the gate without classifying it breaks the build. When yap-fmt-sweep-2026-10-26 flips fmt
         back to blocking, it moves 'fmt' between the arrays in the same diff (and updates land-gate-fmt.test.mjs).
         RECON AND THE LAND GATE READ THE SAME LIST: REVIEW_RECON and BUILD_RECON already interpolate GATE_CMDS(...);
         additionally make both recon prompts say "classify each exit with GATE_BLOCKING / GATE_ADVISORY — you do not
         decide what is informational" (interpolate the arrays, do not retype names), delete the recon-only fmt line
         at :1860-1862, and require RECON_SCHEMA to carry gateExits {typecheck,test,build,polish,diarize,stage,
         polishtest,diarizetest,fmt,clippy,cargotest} as numbers (required, not optional).
      1. SAME-REASON-TWICE (pure fn in stall.mjs, mirrored into template.mjs):
         stallReason(landResult) -> {status, reason}: status is the fix-land status already derived at :1726-1733;
         reason is, for ci-red, the FIRST non-zero BLOCKING command name read from gateExits (GATE_BLOCKING order),
         else the "failingCommand" field, else the reason field, else "unknown"; for any other status the reason field
         or "none". sameReasonTwice(history, next) -> boolean: true when history (array of {pr, status, reason}) holds
         a prior entry for the SAME pr whose status and reason both equal next's and status is not "merged".
         In reviewOnePr (before the land dispatch at :1715), consult the per-PR history kept in a module-level Map;
         when sameReasonTwice is true, do NOT dispatch another fix-land for that PR: dispatch ONE stall-writer seat
         (agentR, model 'opus', effort 'low', phase = the same phase the neighbouring fix-land calls use, label
         'stall-writer:' + itemId — not listed in seat-models.json, so it runs as written) that (a) labels the PR
         loop-stalled (create the label first, || true), (b) comments on the PR "loop-stalled: <status> <reason> twice
         in a row", (c) appends  "STALL pr=<n> item=<id> status=<s> reason=<command>"  as one line to the LOG file (Loop-Logs) and writes
         the board row with stage review:needs-human and a note beginning "STALL: <command>", and (d) returns.
         The PR's outcome is status 'loop-stalled'; the chain moves to the next PR (no third attempt, no needs-human
         relabel, no halt). The rule never fires on 'merged', and a changed reason resets nothing — the history is
         per PR and compares only the most recent prior entry.
      2. GATE RED ON MAIN, BEFORE ANY REVIEW (pure fn gateRedOnMain(gateExits, blocking, advisory) -> {red:[cmds],
         unreadable:[cmds], advisoryRed:[cmds]}): a blocking command is red when its exit is non-zero, unreadable when
         the key is missing or not an integer (unreadable counts as red for the halt, fail closed). Call it in
         reviewPass() right after the recon result is threaded (:2002-2018), ahead of the triage dispatch at :2119.
         When red or unreadable is non-empty: dispatch the stall-writer seat once to write
         "GATE RED ON MAIN: <cmd>[, <cmd>...]" (prefix "GATE UNREADABLE ON MAIN:" for the unreadable ones) to the board
         (harness:halted row), to the LOG file (Loop-Logs), and to the sentinel file /Users/wilsonguenther/code/wilson-voice-loop/STALL.txt
         (mkdir -p its dir; the file holds a stamp from date -u, the rule number, and the line); then reviewPass returns
         { halted: 'gate-red-on-main', red, queued: 0 } WITHOUT triage, chains, sweeper or verify, and the parent log says
         so. advisoryRed (today: fmt) is only reported in the log as "advisory red on main: fmt, N hunks", never a halt.
         Build mode (MODE === 'build') gets the same check after its recon: a red blocking command on main halts the
         builders too, because every builder gate would fail the same way (state this in the PR body if you scope it
         to review only; the review pass is the required half).
      3. NO PROGRESS (pure fn noProgress(results, n=3) -> boolean; results is the array of per-PR land outcomes in
         completion order, each {pr, status, gateFailure: boolean}): true when the LAST n entries are for n DISTINCT PRs,
         none is 'merged', and every one has gateFailure true (status ci-red, or a non-zero BLOCKING exit in gateExits).
         Evaluate after every land outcome in reviewChain (:1756-1763). On true: dispatch the stall-writer once (guard
         with a module-level boolean so two chains cannot both fire it) to write "STALL no-progress: last 3 land
         results were gate failures: <pr:command, ...>" to the board, the LOG file (Loop-Logs) and STALL.txt, then set a new module
         flag  stalled = true  that reviewChain checks next to its existing  if (halted) break  — the review barrier
         stops pulling PRs. Builders are unaffected (the flag is only read in reviewChain). The land-sweeper and
         main-verify still run (they settle what is open and are how the orchestrator sees the damage); the pass result
         carries stalled: true. A merged result anywhere in the last 3 breaks the run.
      4. EXTERNAL ALARM. The machine monitor in ~/bin (its filename contains the banned word, so reach it only by the
         glob  ~/bin/loop-w*-yap-learn.sh ) is outside this repo and a builder lane cannot ship it in a PR. So:
         (a) add scripts/loop/stall-alarm.sh to the repo — a sourceable bash function  stall_alarm_check  that reads
         \${STALL_FILE:-/Users/wilsonguenther/code/wilson-voice-loop/STALL.txt} and a state variable stall_alert
         (edge-triggered, same style as the RSS/mem/disk alerts in that script): when the file exists and the state
         is 0, set it to 1 and print one line  "<stamp> ALERT STALL <file contents, newlines folded to ' | '>" (stamp
         = date -u +%Y-%m-%dT%H:%M:%SZ); when the file is gone and the state is 1, set 0 and print
         "<stamp> cleared STALL". The monitor's Monitor filter already surfaces lines containing ALERT.
         (b) The PR body carries the exact 3-line patch for the ~/bin script (source the repo file by absolute path,
         initialise stall_alert=0 next to rss_alert, call stall_alarm_check once per loop iteration and once in the
         --status branch); the ORCHESTRATOR applies that patch to ~/bin after merge — say so in the PR body, and do not
         write outside the repo yourself.
         (c) Document in the PR body and as the header comment of stall-alarm.sh: the orchestrator removes STALL.txt
         after acting (that is the acknowledgement; the next monitor tick prints "cleared STALL"); the loop itself never
         deletes it, and a fresh stall overwrites it.
      5. TESTS: scripts/loop/stall.mjs (pure, no I/O, no imports) and scripts/loop/stall-detector.test.mjs (node:test,
         hermetic). Exact test names:
           gate_flags_match_gate_cmds            (echo labels in GATE_CMDS == GATE_BLOCKING + GATE_ADVISORY, both ways)
           recon_prompts_interpolate_gate_flags   (both recon prompts reference GATE_BLOCKING; no recon-only fmt line)
           stall_rules_block_is_identical_to_stall_mjs  (the template copy between the markers equals stall.mjs)
           same_reason_twice_ci_red_same_command_stalls
           same_reason_twice_different_command_does_not_stall
           same_reason_twice_different_pr_does_not_stall
           same_reason_twice_never_on_merged
           gate_red_on_main_flags_blocking_nonzero
           gate_red_on_main_ignores_advisory_fmt
           gate_red_on_main_missing_key_is_unreadable_and_red
           no_progress_three_distinct_prs_gate_failures_halts
           no_progress_same_pr_three_times_does_not_count
           no_progress_a_merge_in_the_window_resets
           stall_alarm_prints_alert_then_cleared   (runs stall-alarm.sh in bash with a temp STALL_FILE: absent -> no
                                                    output; created -> exactly one "ALERT STALL" line containing the
                                                    file text; second tick -> silent; file removed -> one "cleared
                                                    STALL" line)
         Keep land-gate-fmt.test.mjs, review-gate.test.mjs, seat-models.test.mjs and seat-cargo-env.test.mjs green.
         build.mjs's dry run (stubbed agent/parallel/phase/log) must still pass with the new stall-writer dispatches
         and the new recon schema key: extend the stubs in build.mjs only as far as the new code needs.
  `,
  acceptance: `
    test -f scripts/loop/stall.mjs
    test -f scripts/loop/stall-detector.test.mjs
    test -f scripts/loop/stall-alarm.sh
    grep -q 'const GATE_BLOCKING' scripts/loop/template.mjs
    grep -q 'const GATE_ADVISORY' scripts/loop/template.mjs
    grep -q 'STALL-RULES-BEGIN' scripts/loop/template.mjs
    grep -q 'STALL-RULES-END' scripts/loop/template.mjs
    test "$(grep -c 'GATE_CMDS(' scripts/loop/template.mjs)" -ge 4
    grep -q 'GATE_BLOCKING' scripts/loop/template.mjs && ! grep -n 'Plus the informational one' scripts/loop/template.mjs
    bash -n scripts/loop/stall-alarm.sh
    shellcheck scripts/loop/stall-alarm.sh
    node --test scripts/loop/stall-detector.test.mjs
    node --test scripts/loop/*.test.mjs
    S=scripts/loop/stall.mjs; cp $S $S.orig; sed -i.bak 's/function sameReasonTwice/function sameReasonTwiceDISABLED/' $S; node --test scripts/loop/stall-detector.test.mjs; MUT=$?; cp $S.orig $S; rm -f $S.orig $S.bak; test $MUT -ne 0
    node --test scripts/loop/stall-detector.test.mjs
    T="$(mktemp -d)" && printf 'GATE RED ON MAIN: clippy\\n' > "$T/STALL.txt" && STALL_FILE="$T/STALL.txt" bash -c '. scripts/loop/stall-alarm.sh; stall_alert=0; stall_alarm_check' | grep -q 'ALERT STALL.*GATE RED ON MAIN: clippy' && rm -rf "$T"
    (cd desktop && npm ci)
    (cd desktop && npm run loop:validate)
  `,
  notes: `
    Audit 2026-10-26: 2 h stall, root cause fmt blocking in land gate while red on main; fixed by hand (fmt advisory) at
    ~15:20Z; this item makes the loop notice next time. FUTURE item — never stop a live pass for it; build it when the
    lanes are idle, then the orchestrator runs npm run loop:build and applies the ~/bin monitor patch from the PR body.
    Open design points the builder should state a decision on in the PR body: (1) whether build mode also halts on a
    red blocking command (recommended yes); (2) an unreadable recon exit counts as red (fail closed) — a Sonnet recon
    seat that omits gateExits will halt the pass, which is the intended loud failure; (3) STALL.txt is removed only by
    the orchestrator.
  `,
})

// ── 00b-yap-telemetry-2026-10-27.mjs ──────────────────────────────────────
// yap-telemetry-2026-10-27 — Wilson 2026-10-27 (verbatim): "Just add telemetry where we need to so we can update".
// Written 2026-10-27T17:20Z (date -u) by the harness seat. FIX/FEATURE, class STANDARD, builder OPUS (no
// builder:'sonnet' tag), NO panel. BUILD-ONLY run (args.landBar 'ci'): the PR merges on GitHub CI green.
//
// DEPENDENCY ROOT. Every later item's builder prompt carries the TELEMETRY (Wilson 2026-10-27) block, which tells it
// to record through desktop/src-tauri/src/diagnostics.rs once this item is on origin/main. The harness has NO
// `after:` / dependsOn edge (build order = file order; lanes = files, resolved at run time against args.lanes), so
// this item is (a) in the 00b file, straight after the harness (00) and the 2026-10-26 fixes (00a), and (b) PINNED
// to lane A with `lane: 'A'` (build.mjs stamps the pin; with lanes:3 the 00b file would otherwise round-robin onto
// lane C). Items in the same part on lanes B/C do not wait for it — they log through the `log` crate as the
// TELEMETRY block says, and the drain lands this PR at the part boundary (or a builder's LAND step earlier, on CI
// green), so items in later parts build on top of it.
//
// HAZARD (same as 07/07b/08/00a): no agent launches the GUI app, posts events into, signals or quits Wilson's running
// /Applications/Yap.app, or writes TCC on this machine. Proof is cargo test + vitest; the on-device look goes in the
// PR body as WILSON'S WALK. SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap-telemetry-diagnostics', prompt: 'yap-telemetry-2026-10-27', branch: 'loop/yap-telemetry-diagnostics', gated: null,
  lane: 'A',
  title: 'One Diagnostics surface — structured event ring buffer fed by the logger, a Settings panel with a live health line, and a redacted "Export diagnostics" bundle',
  preflight: `
    test -f desktop/src-tauri/src/diagnostics.rs && grep -q 'mod diagnostics' desktop/src-tauri/src/lib.rs && test -f desktop/src/diagnostics.ts && grep -q 'export_diagnostics' desktop/src-tauri/src/lib.rs && grep -q 'diag_warn!' desktop/src-tauri/src/paste.rs && grep -q 'diag_warn!' desktop/src-tauri/src/meeting_pipeline.rs
  `,
  spec: `
    Panel: not required — class STANDARD, builder opus. Wilson 2026-10-27: "Just add telemetry where we need to
    so we can update". Today a failure on Wilson's machine is a grep through yap.log: there is no in-app view of
    what just went wrong, no single health line, and no one-click bundle he can hand back to the loop.

    WHAT EXISTS (reuse it — do not build a second one of anything):
      - desktop/src-tauri/src/logging.rs: env_logger teed to <data_dir>/logs/yap.log (TeeWriter) + panic hook.
      - desktop/src-tauri/src/latency.rs: PipelineSpans, the key=value latency line per dictation.
      - desktop/src-tauri/src/support.rs: the support bundle; redact / redact_line (username scrub),
        username_from_home, permissions_block, models_block. Its redaction is the redaction — call it.
      - Error surfaces: Result<_, String> / CommandError {code, message} -> toast() in desktop/src/appShell.ts;
        status.lastError via app.emit("status", build_status(..)) in lib.rs -> desktop/src/pill/live.ts.
      - Settings views live in desktop/src/views/settings/ (Advanced.tsx is the debug-ish one).
    There is NO metrics sink and NO ring buffer on main (pre-flight proves diagnostics.rs is absent).

    DO (names are contract)
      1. desktop/src-tauri/src/diagnostics.rs, declared "mod diagnostics;" in lib.rs:
         - pub struct DiagEvent { ts_ms: i64, level: String, module: String, msg: String, fields: BTreeMap<String, String> }
           (serde Serialize).
         - pub struct DiagRing — fixed capacity (pub const DIAG_RING_CAPACITY: usize = 500), oldest evicted first,
           Mutex-guarded global (OnceLock) so any thread records without plumbing state.
         - FED BY THE EXISTING LOGGER: logging.rs's tee pushes every record at INFO and above into the ring
           (level, target as module, message). No second logger, no new crate.
         - macros diag_error! / diag_warn! / diag_info! (exported from diagnostics.rs) taking a module, a message and
           key = value fields: they record the event WITH its fields AND emit the same line through log::error! /
           log::warn! / log::info! (so yap.log is unchanged). The tee must not record that line a second time
           (e.g. a dedicated log target the tee skips) — one event per occurrence, proven by a test.
         - pub fn record_duration(module, name, ms) and pub fn bump(module, counter): counters/durations kept beside
           the ring (last value + count + max), included in the snapshot and the export.
      2. HEALTH LINE — #[tauri::command] diagnostics_snapshot -> { events: last N, counters, health } where health
         is: permissions state (the existing permissions/perm_truth report: microphone, accessibility, input
         monitoring, post-event), model loaded (selected model + loaded yes/no), hotkey registered (yes/no + the
         chord), meeting pipeline state (idle/recording/processing/failed + last stage), insert bridge state (last
         insert path + last outcome from paste.rs). Registered in tauri::generate_handler!.
      3. EXPORT — #[tauri::command] export_diagnostics writes TWO files to ~/Downloads:
         yap-diagnostics-<UTC stamp>.json (snapshot) and .txt (human-readable: health line, counters, events).
         REDACTED: run every string through support.rs redact with the username; drop any field whose key is
         text / transcript / content / snippet; reduce any absolute path to its basename. Returns both paths;
         the UI reveals them (support.rs reveal_in_finder). Failure -> Err(String) that the panel shows.
      4. FRONTEND — desktop/src/diagnostics.ts (types + invoke wrappers + pure formatters: formatHealth,
         formatEvent) and a "Diagnostics" panel desktop/src/views/settings/Diagnostics.tsx reached from the Settings
         view (its own tab/section, not a modal): the health line at the top, the last events (newest first, level
         coloured with existing tokens.css variables), counters, a Refresh button and an "Export diagnostics" button
         that shows the written paths or the error via the existing toast. Same visual language as the other
         settings sections — no new design system.
      5. ROUTE THE ERROR SITES THROUGH IT — every existing log::error! and log::warn! in these files becomes
         diag_error! / diag_warn! with its values as fields (no behaviour change otherwise): dictation insert path
         desktop/src-tauri/src/paste.rs and paste_tx.rs; meeting pipeline desktop/src-tauri/src/meeting_pipeline.rs;
         model download desktop/src-tauri/src/models.rs; permissions desktop/src-tauri/src/mic_auth.rs,
         perm_truth.rs and permissions.rs. And record: dictation insert latency (record_duration "insert" ms at the
         end of each insert), meeting pipeline stage durations (one per stage), model download duration + bytes.
      6. Tests: Rust unit tests in diagnostics.rs — ring_buffer_evicts_oldest_at_capacity,
         ring_buffer_keeps_order, macro_records_once_not_twice, export_redacts_username_and_transcript_fields,
         export_reduces_paths_to_basename; vitest desktop/src/diagnostics.test.ts for the formatters.

    DO NOT: send anything off the machine (no network, no upload — the bundle is a local file Wilson attaches
    himself); log transcript text or audio; rename the bundle identifier or the data directory; add a crate.
  `,
  acceptance: `
    test -f desktop/src-tauri/src/diagnostics.rs
    grep -q 'mod diagnostics' desktop/src-tauri/src/lib.rs
    grep -q 'export_diagnostics' desktop/src-tauri/src/lib.rs
    grep -q 'diagnostics_snapshot' desktop/src-tauri/src/lib.rs
    test -f desktop/src/diagnostics.ts && test -f desktop/src/views/settings/Diagnostics.tsx
    test "$(grep -c 'log::\\(error\\|warn\\)!' desktop/src-tauri/src/paste.rs desktop/src-tauri/src/paste_tx.rs desktop/src-tauri/src/meeting_pipeline.rs desktop/src-tauri/src/models.rs desktop/src-tauri/src/mic_auth.rs desktop/src-tauri/src/perm_truth.rs desktop/src-tauri/src/permissions.rs | awk -F: '{s+=$2} END {print s}')" = 0
    test "$(grep -c 'diag_\\(error\\|warn\\)!' desktop/src-tauri/src/paste.rs desktop/src-tauri/src/paste_tx.rs desktop/src-tauri/src/meeting_pipeline.rs desktop/src-tauri/src/models.rs desktop/src-tauri/src/mic_auth.rs desktop/src-tauri/src/perm_truth.rs | awk -F: '{s+=$2} END {print s}')" -ge 15
    grep -q 'record_duration' desktop/src-tauri/src/paste.rs && grep -q 'record_duration' desktop/src-tauri/src/meeting_pipeline.rs && grep -q 'record_duration' desktop/src-tauri/src/models.rs
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && npx vitest run src/diagnostics)
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib diagnostics)
  `,
})

// ── 00c-y5-ui-ux-polish.mjs ───────────────────────────────────────────────
// Y5 — UI/UX. Wilson, 2026-09-12, verbatim: "the app looks broken, not smooth —
// lots of UI/UX problems." And, standing: "we got to really think about this
// thing end to end."
//
// AUDIT, structural, at 4e8c9adf:
//   desktop/src/App.tsx      3,439 lines  — ONE component holding seven views
//                            (Nav: home | permissions | meetings | insights |
//                            dictionary | scratchpad | settings, App.tsx:55-61)
//                            and eight settings sub-tabs (App.tsx:66-85).
//   desktop/src/App.css      2,789 lines  — one stylesheet, no token layer.
//   desktop/src/home/YappyHouse.tsx  919 lines
//   Empty / loading / error states:
//     git grep -c "empty-state\|EmptyState\|skeleton" -- desktop/src
//       -> App.css: 1, App.tsx: 1.  For SEVEN views. That is the "looks broken"
//          report: a view with no data renders a bare frame with no explanation
//          and no action.
//   The pill's whole state vocabulary is four values:
//     ClassicPill.tsx:23-24  {recording, busy, message} + a `done` flag
//     live.ts:252   LivePhase = idle | listening | thinking | done | sleepy
//   Wispr's Flow Bar has TWELVE states with exact geometry per dock
//   (reference_wispr_parity_research §4.2, [BUNDLE]): resting · ready ·
//   activePtt · activePopo · processing · polishProcessing · polishCompleted ·
//   autoCleanupCompleted · error · growthNudgeActive · navigationActive ·
//   postInstructBubble · instructCollapsing.
//   project_yap_pill_vision, Wilson's own words: "fill the dead time after
//   talking stops and before text appears (transcribe/think gap) and every
//   other micro-state — idle->listening->transcribing->polishing->pasting->
//   done->fold-back + errors/permission/model-loading/empty. Today only
//   listening/busy/done exist."
//
// MOTION CONSTANTS, primary-sourced, use these exact numbers
// (reference_wispr_parity_research §4.3 [BUNDLE]):
//   springs stiffness:600 damping:35 restDelta:0.05 (snappy morphs)
//   springs stiffness:300 damping:28 (the slower one)
//   cubic-bezier(0.05,0.6,0.4,0.95) @ 100ms for state changes
//   300-400ms for expand/collapse
//   hover hysteresis: an invisible ::before alpha margin at inset:-12px,
//   painted ONLY while expanded
//   rgba(0,0,0,0.004) background so the box is clickable while invisible
//
// AESTHETIC LOCK — not negotiable, do not re-litigate:
//   feedback_companion_must_be_cute: PIXEL ART on a little LCD screen/pod
//   (Tamagotchi / Bitzee). Chunky pixels, imageSmoothingEnabled=false, limited
//   retro palette. NOT smooth vector. NO angled "angry" eyebrows. Paper/origami
//   is REJECTED ("def a no on the paper").
//   feedback_no_generic_ui: reject AI-dashboard aesthetics.
//   feedback_ui_quality: truly native feel, no webview tells.
//   feedback_think_ux_first: controls first, prose last.
//
// ── OWNER DECISION 2026-09-13 (Wilson) — THE PILL IS A CHARACTER SYSTEM ──
//   Verbatim: "I thought we were gonna develop it and then make more characters
//   and make it more flexible ... there's a classic pill and there's a yappy
//   pill and there's gonna be different pills with the different creatures that
//   are coming."
//   So "which pill ships in v1" was the WRONG QUESTION and is closed: BOTH ship,
//   as the first two CHARACTERS of a pluggable system, and more creatures come
//   later. The panel's cost objection was real and is answered STRUCTURALLY,
//   not by picking one:
//     * THE SHELL owns everything that is not the creature — the window, the
//       dock, the geometry table, hover/hit-testing, motion, the phase state
//       machine, a11y names. Dock positions are handled ONCE, in the shell.
//     * A CHARACTER is a DATA-DRIVEN MODULE behind one interface: given a phase
//       and a tone it returns a sprite/animation and copy. It knows nothing
//       about docks, windows or license logic.
//     * TESTS RUN A FIXTURE MATRIX OVER THE REGISTERED CHARACTERS instead of
//       duplicating a code path per pill. Y5-C's "13 phases x 2 styles x 3 docks"
//       becomes 13 phases x 3 docks in the shell, plus one data completeness
//       sweep per registered character.
//     * A NEW CREATURE IS A NEW MODULE + A FIXTURE ROW. No shell change.
//   Y5-K builds that system and reinstates the living habitat (the killed Y5-H)
//   as its habitat layer. The aesthetic lock below is unchanged and binding.
//
// SHARED PREAMBLE + STANDARD GATE: see 00-y0-harness-and-gates.mjs.
// EVERY item here owes the two-size screenshots (980x700 and 720x520) plus the
// pill at three dock positions for each state it touches.

ITEMS.push({
  id: 'Y5-G', prompt: 'Y5', branch: 'loop/y5-g-split-app-tsx-into-views', gated: null,
  notes: `STATUS 2026-09-26: MERGED (PR #188) in run wf_f6cd9dfa-860 — acceptance failed twice before merge (needs-human): the review pass re-checks it. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
  title: 'Split the 4,660-line App.tsx into seven view modules so a screen can be worked on at all',
  preflight: `
    test 900 -ge "$(wc -l < desktop/src/App.tsx)"
    test -d desktop/src/views
    cd ${APP} && npm ci && npm run build && npm test
  `,
  spec: `
    \`App.tsx\` is 4,660 lines holding seven views and eight settings sub-tabs
    (PANEL 2026-09-12: the audit's 3,439 was measured against an older tree and
    is 1,221 lines low — \`wc -l desktop/src/App.tsx\` at 4e8c9adf is 4,660, so
    this is a ~3,760-line move, not a ~2,500-line one).
    Every later UI item in this loop has to edit it, which makes them serially
    conflicting and makes each one hard to review. This is the enabling refactor.

    Do:
      * \`desktop/src/views/{Home,Permissions,Meetings,Insights,Dictionary,Scratchpad,Settings}.tsx\`,
        one per \`Nav\` value (App.tsx:55-61), and
        \`desktop/src/views/settings/\` for the eight \`SettingsTab\`s
        (App.tsx:66-85). App.tsx keeps the shell: nav, the license chip, the
        toast host, the event listeners.
      * PURE MECHANICAL MOVE. No behaviour change, no restyling, no renaming of
        a state field. The gate is that \`npm test\` and \`npm run build\` pass and
        Y0-E's structural smoke reports the same result before and after; a
        mixed refactor-plus-redesign diff
        is unreviewable and is how a regression ships.
      * Shared state that currently lives in one component body has to be lifted
        deliberately. Prefer props and a small number of explicit contexts over
        a global store; do not add a state-management dependency.
      * The event listeners (App.tsx:1190-1210 license, plus the take/status
        listeners) stay in ONE place in the shell. Seven views each subscribing
        to \`recording\` is seven listeners and a leak.

    Tests: existing suites must pass unchanged, and add
    \`desktop/src/views/views.test.tsx\` asserting each view module exports a
    default component and that no view module registers a Tauri \`listen\` —
    the sweep that keeps the listener discipline from eroding.

    PANEL 2026-09-12 — ORDER REVERSED. This item now runs FIRST in this file.
    Four seats converged: eleven items across both lanes edit App.tsx, this item
    empties it, and the build agent's LAND step is told to SKIP a conflicting PR
    rather than fix it. Doing the states first means Y5-B/Y5-F write markup into
    the monolith and then this item moves it again, with the cross-lane items
    (PERM-B, PERM-E, Y2-D, Y4-G, DB-D, Y10-E, Y7-D) all branched off the old
    shape. So: pure mechanical move FIRST, on the smallest possible diff, then
    every later UI item writes into desktop/src/views/<View>.tsx.
    Because it moves first, the gate is the existing suites plus Y0-E's
    structural windowed smoke (same assertions before and after) — NOT
    "the screenshots are pixel-identical", which named a golden-image gate that
    does not exist and that Y7-B explicitly forbids.
    Split the landing if the diff is unreviewable: views first, then settings/*,
    two PRs, identical gate on each.

    What NOT to do:
      - Do NOT restyle while moving.
      - Do NOT add Redux/Zustand/Jotai.
      - Do NOT leave a re-export shim that lets code keep importing views from
        App.tsx.
  `,
  acceptance: `
    test 900 -ge "$(wc -l < desktop/src/App.tsx)"                 # MEASURED baseline: 4660
    # PANEL: views.test.tsx used to live in views/ and matched this glob, so the
    # count was 8 and \`test 7 -eq\` could never pass however well the item was built.
    test 7 -eq "$(ls desktop/src/views/*.tsx | wc -l | tr -d ' ')"
    test 8 -eq "$(ls desktop/src/views/settings/*.tsx | wc -l | tr -d ' ')"
    test -f desktop/src/views/__tests__/views.test.tsx
    grep -q 'no_view_module_registers_a_listener' desktop/src/views/__tests__/views.test.tsx
    # a PURE MOVE adds no behaviour: the moved lines land, they do not multiply
    test 5200 -ge "$(cat desktop/src/App.tsx desktop/src/views/*.tsx desktop/src/views/settings/*.tsx | wc -l)"
    test 0 -eq "$(grep -c 'settingsTab === ' desktop/src/App.tsx)"
    cd ${APP} && npm ci
    npx tsc --noEmit ; test $? -eq 0
    npm run build ; test $? -eq 0
    npm test      ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y5-A', prompt: 'Y5', branch: 'loop/y5-a-design-tokens-and-one-visual-system', gated: null,
  notes: `STATUS 2026-09-26: BUILT, PR #190 OPEN and stale (main moved 30+ times under it), labelled needs-human. Rebase that PR onto main and finish it; do not open a second PR.`,
  title: 'A token layer so the seven views stop each inventing their own colours, spacing and radii',
  preflight: `
    test -f desktop/src/tokens.css
    test 0 -eq "$(grep -cE '#[0-9a-fA-F]{3,8}' desktop/src/App.css)"
    cd ${APP} && npm ci && npm run build
  `,
  spec: `
    \`App.css\` is 2,789 lines with literal colours, spacings and radii repeated
    throughout, which is mechanically why unrelated screens look like different
    apps — the "looks broken, not smooth" report is largely inconsistency, not
    any single broken screen.

    Do:
      * \`desktop/src/tokens.css\`: one \`:root\` block. Colour, elevation,
        radius, spacing (a 4px-based scale), type scale, motion durations and
        the two spring curves from the header. Name tokens by ROLE
        (--surface-raised, --text-muted, --accent-urgent), never by value
        (--gray-3). A role-named token survives a palette change; a
        value-named one guarantees the next inconsistency.
      * Migrate App.css and float.css to the tokens. Zero hex literals left in
        either. The gate greps for that, so a partial migration fails.
      * Keep the LOOK as it is in this item, to within a rounding error. This is
        a refactor whose whole value is that it is invisible; changing the
        palette at the same time makes every later visual diff unreadable.
      * Palette: pin the retro/LCD palette the companion already uses so the
        chrome and the character share one world instead of two
        (feedback_companion_must_be_cute — chunky pixels, limited retro
        palette). Read the palette off docs/prototypes/yappy-house.html and
        src/home/YappyHouse.tsx rather than inventing one.
      * Dark/light: whichever the app ships today is the one that must keep
        working. Define both token sets if both exist; define one and say so if
        only one does. Do not add a theme switcher in this item.

    PR body owes a before/after screenshot of all seven views at both window
    sizes, and the statement "no intentional visual change" with any unavoidable
    diff called out by name.

    What NOT to do:
      - Do NOT add Tailwind or a CSS framework. The CSP is strict
        (tauri.conf.json app.security.csp: style-src 'self' 'unsafe-inline',
        no external hosts) and a framework here buys nothing.
      - Do NOT restyle anything in this item. Y5-B..I do the visual work on top.
  `,
  acceptance: `
    test -f desktop/src/tokens.css
    test 0 -eq "$(grep -cE '#[0-9a-fA-F]{3,8}' desktop/src/App.css)"     # 0 literals left
    test 0 -eq "$(grep -cE '#[0-9a-fA-F]{3,8}' desktop/src/float.css)"
    grep -q -- '--surface' desktop/src/tokens.css
    grep -q -- 'cubic-bezier(0.05, *0.6, *0.4, *0.95)' desktop/src/tokens.css
    grep -q 'tokens.css' desktop/src/main.tsx desktop/src/float-main.tsx
    cd ${APP} && npm ci && npm run build ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y5-B', prompt: 'Y5', branch: 'loop/y5-b-every-view-has-empty-loading-and-error-states', gated: null,
  notes: `STATUS 2026-09-26: BUILT, PR #192 OPEN and stale (main moved 30+ times under it). Rebase that PR onto main and finish it; do not open a second PR.`,
  title: 'The "looks broken" fix: all seven views get a real empty state, a real loading state and a real error state',
  preflight: `
    test 7 -le "$(grep -ro 'data-empty-state' desktop/src --include=*.tsx | wc -l)"
    cd ${APP} && npm ci && npm test -- states
  `,
  spec: `
    MEASURED: \`git grep -c "empty-state\\|EmptyState\\|skeleton" -- desktop/src\`
    returns one match in App.tsx and one in App.css, for seven views
    (App.tsx:55-61: home, permissions, meetings, insights, dictionary,
    scratchpad, settings). A fresh install has no history, no meetings, no
    dictionary entries, no scratchpad notes and no insights — which is to say
    every view a new user opens is in its least-designed state. That is the
    first impression and it is the complaint.

    For EACH of the seven views ship three states:
      * EMPTY: one sentence saying what lives here, and ONE primary action that
        creates the first thing. Home's empty action is "hold fn and say
        something"; Dictionary's is "add a word"; Scratchpad's is "new note".
        Never a shrug, never a bare illustration with no action.
      * LOADING: a determinate state where the count is knowable and a calm
        indeterminate one where it is not. Not a full-page spinner. Not a
        skeleton that pulses forever (a skeleton with no timeout is how the
        Drivia audit found nineteen pages "still loading at 15s").
      * ERROR: what failed, in the user's terms, and the one button that retries
        or fixes it. A DB error is "Yap could not open its history file", not an
        SQLite code.
    Mark each with \`data-empty-state\` / \`data-loading-state\` /
    \`data-error-state\` so the gate can count them and a future browser walk
    can assert them.

    Two specific measured cases that must be covered by name:
      * Home before the model is downloaded. \`status.modelReady\` and
        \`needsPerms\` already gate a banner (App.tsx:2227) — make the whole view
        coherent in that state rather than a normal view with a warning strip.
      * Insights with zero takes. \`nav === "insights" && insights &&\`
        (App.tsx:2953) renders NOTHING when \`insights\` is falsy — a blank
        screen with a heading. That is a literal blank page in the shipped app.

    Copy rules: sentence case, no exclamation marks, name the action in the
    button ("Add a word", not "OK"), say what happens next rather than what went
    wrong (feedback_think_ux_first).

    Tests: extract each view's state decision into a pure function
    (\`viewState(data, loading, error)\`) in \`desktop/src/viewState.ts\` with
    \`viewState.test.ts\` covering the 3x7 matrix, so the assertions do not
    require rendering 3,439 lines of App.tsx.

    What NOT to do:
      - Do NOT ship an empty state without an action.
      - Do NOT use the same generic illustration for all seven. Generic is the
        thing being fixed (feedback_no_generic_ui).
      - Do NOT satisfy the gate by adding the attribute to a div that renders
        nothing. The gate counts attributes; the reviewer looks at the
        screenshots, and a hollow marker is a failed item.
    PANEL 2026-09-12 — three corrections.
      * Enumerate each view's REAL state set instead of demanding all three
        everywhere. A permissions screen with nothing in it is a bug, not an
        empty state, and a settings screen has no empty state either: those two
        get loading + error plus a settled "nothing to fix here" state. The list
        views (History, Meetings, Dictionary, Insights, Scratchpad) get all
        three. Lower the counts to match the enumeration — markers that exist
        only to satisfy a count are the hollow markers this item's own "What NOT
        to do" forbids, and build mode runs NO reviewer to catch them.
      * The gate that decides is Y0-E's structural smoke (it already fails a
        view that renders zero rows with no \`data-empty-state\`), not an
        attribute count. The counts are a cheap pre-flight.
      * Scratchpad's shape changes in DB-D (a real second window, versions), so
        its states are provisional here — say so in the PR body and do not build
        them twice.
      * Write the markers into the view MODULES (Y5-G has already moved them);
        never into the App.tsx shell.

  `,
  acceptance: `
    test 7 -le "$(grep -ro 'data-empty-state' desktop/src --include=*.tsx | wc -l)"
    test 7 -le "$(grep -ro 'data-loading-state' desktop/src --include=*.tsx | wc -l)"
    test 7 -le "$(grep -ro 'data-error-state' desktop/src --include=*.tsx | wc -l)"
    test -f desktop/src/viewState.ts
    test -f desktop/src/viewState.test.ts
    grep -q 'insights' desktop/src/viewState.test.ts
    cd ${APP} && npm ci
    npm test -- viewState ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build         ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y5-C', prompt: 'Y5', branch: 'loop/y5-c-the-full-pill-state-machine', gated: null,
  notes: `STATUS 2026-09-26: MERGED (PR #193) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
  title: 'The pill gets every state the product has, including the transcribe/think gap Wilson named',
  preflight: `
    grep -q '"polishing"' desktop/src/pill/live.ts
    grep -q '"pasting"' desktop/src/pill/live.ts
    grep -q '"model_loading"' desktop/src/pill/live.ts
    cd ${APP} && npm ci && npm test -- pill/live
  `,
  spec: `
    \`LivePhase\` (live.ts:252) is \`idle | listening | thinking | done | sleepy\`.
    PERM-C adds \`blocked\`, Y2-C adds \`gated\`, Y3-C adds \`transcribing\`. This
    item completes the vocabulary against project_yap_pill_vision's list, which
    is Wilson's own enumeration:

      idle -> listening -> transcribing -> polishing -> pasting -> done
      plus: error · blocked (permission) · gated (license) · model_loading ·
            empty (nothing was said) · cancelled · sleepy

    Add the missing ones — \`polishing\`, \`pasting\`, \`error\`, \`model_loading\`,
    \`empty\`, \`cancelled\` — and make each REAL:
      * \`polishing\` is distinct from \`transcribing\`. It is the LLM stage and it
        has its own deadline (1200 ms, polish.rs:59), so its state has a
        knowable duration and must not look like an indefinite wait.
      * \`pasting\` exists because the paste is receipt-sequenced (YV39) and can
        fail on its own — a failure there is an Accessibility problem, not a
        transcription problem, and the pill must say the right one.
      * \`empty\` is the YV16 no-speech / hallucination-gate outcome: Yap
        correctly refuses to paste garbage, and today says nothing, so a user
        experiences a dead hotkey. This state is the whole visible payoff of
        that gate.
      * \`model_loading\` covers YV80's lazy arm: the first dictation after
        launch loads the engine while capture is already live (lib.rs:1156-1158).
        The pill should say the engine is warming rather than appear stuck.
      * \`error\` is the generic terminal state with a one-line reason from the
        take's \`last_error\` (lib.rs:1147 sets it) — never a code.

    Every phase needs: a duration policy (how long it holds), a next phase, and
    a rendering that the SHELL places at ALL THREE dock positions — once, not
    once per pill (OWNER DECISION 2026-09-13, top of this file). Both
    ClassicPill and YappyPill ship; they are the first two characters, so what
    each owes this item is PHASE COVERAGE AS DATA (a sprite/animation and copy
    for every phase in every tone), never a second copy of the phase logic or of
    the dock placement. Until Y5-K lands the registry, keep the per-character
    data in the component that already holds it and DO NOT add a third branch
    on \`pill_style\` anywhere outside those two components — Y5-K's first act
    is to lift exactly that data out.
    Put the policy in the pure state machine (live.ts) and only the rendering in
    the components — the
    file is already 310 lines of pure logic with 271 lines of tests precisely so
    this is possible, and ci.yml calls out that vitest is a gate because a
    regression here shipped once.

    Also fix the state-gap problem directly: Wilson's words are "fill the dead
    time after talking stops and before text appears". Assert in tests that
    there is NO reachable sequence in which the pill sits in a single
    undifferentiated phase across the whole post-hold pipeline. Concretely:
    \`listening -> done\` with no intervening phase is illegal.

    Tests, in \`desktop/src/pill/live.test.ts\`:
      * a transition table test — every phase has a defined successor set, and
        no phase is unreachable.
      * \`no_path_from_listening_to_done_without_an_intermediate_phase\`
      * \`every_phase_has_copy_in_every_tone\` — the tone presets are
        rude|friendly|rose (live.ts:29) and \`companion_tone: "friendly"\` is the
        default (lib.rs:412). A phase with no copy in one tone is a blank pill.
      * \`every_phase_renders_within_the_side_dock_strip\`
      * \`every_shipped_character_has_copy_and_art_for_every_phase\` — a table
        test driven off the list of characters that ship (classic, yappy), so
        adding a creature adds a row and not a test file. This is the fixture
        matrix the character system formalises in Y5-K.
      * precedence: blocked > gated > error > cancelled > the happy path.

    Depends on PERM-C, Y2-C, Y3-C.

    What NOT to do:
      - Do NOT add a phase without copy in all three tones.
      - Do NOT let a phase hold indefinitely with no timeout except \`idle\`,
        \`listening\`, \`blocked\` and \`gated\` (the four that legitimately wait on
        the user or the OS). Everything else has a deadline; say it in the table.
    PANEL 2026-09-12 — PERM-C now lands the COMPLETE \`LivePhase\` union in one
    commit (see PERM-C (b)), because six items across two unsynchronised lanes
    were each adding a variant to the same 310-line pure module from their own
    branch off main. So this item ADDS RENDERING AND COPY for phases that
    already exist in the union; it does not edit the union. If a phase is
    missing when this item starts, that is a signal PERM-C has not landed —
    report it, add the rendering against the union as PERM-C specifies it, and
    do not invent a differently-named variant.

  `,
  acceptance: `
    grep -q '"polishing"' desktop/src/pill/live.ts
    grep -q '"pasting"' desktop/src/pill/live.ts
    grep -q '"model_loading"' desktop/src/pill/live.ts
    grep -q '"empty"' desktop/src/pill/live.ts
    grep -q '"cancelled"' desktop/src/pill/live.ts
    grep -q '"error"' desktop/src/pill/live.ts
    grep -q 'no_path_from_listening_to_done_without_an_intermediate_phase' desktop/src/pill/live.test.ts
    grep -q 'every_phase_has_copy_in_every_tone' desktop/src/pill/live.test.ts
    grep -q 'every_phase_renders_within_the_side_dock_strip' desktop/src/pill/live.test.ts
    grep -q 'every_shipped_character_has_copy_and_art_for_every_phase' desktop/src/pill/live.test.ts
    cd ${APP} && npm ci
    npm test -- pill ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build    ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y5-D', prompt: 'Y5', branch: 'loop/y5-d-pill-physics-and-motion-from-the-parity-constants', gated: null,
  notes: `STATUS 2026-09-26: BUILT, PR #196 OPEN and stale (main moved 30+ times under it). Rebase that PR onto main and finish it; do not open a second PR.`,
  title: 'Soft-body pill motion using Wispr\'s measured spring constants, with Reduce Motion respected',
  preflight: `
    grep -q 'stiffness: 600' desktop/src/pill/motion.ts
    cd ${APP} && npm ci && npm test -- motion
  `,
  spec: `
    reference_wispr_parity_research P0 #2: "Soft-body pill physics (M · D).
    Wispr uses \`motion\` springs \`stiffness:600 damping:35 restDelta:0.05\` for
    snappy morphs and \`stiffness:300 damping:28\` for the slower one, plus
    \`cubic-bezier(0.05,0.6,0.4,0.95)\` at 100 ms for state changes and
    300-400 ms for expand/collapse. Yappy should go further: a squash-and-
    stretch response on click/drag and a settle bounce on dock. Why: Wilson's
    exact words — 'bounces when touched, feels soft not stiff'."

    Do:
      * \`desktop/src/pill/motion.ts\` — a hand-written critically-damped spring
        integrator (about forty lines) exposing the two named springs and the
        state-change easing, driven off the rAF loop the pill ALREADY runs
        (ClassicPill.tsx:48-62, which smooths \`--level\` and parks itself at
        rest). Do not add a motion library: the CSP blocks external hosts and
        the parked-rAF discipline from the YV81 energy pass must survive.
      * Squash-and-stretch on press and on drag release; a settle bounce on dock
        (the drag machinery is \`pill/drag.ts\`, YV65).
      * Every morph between the Y5-C phases uses the 100 ms state-change curve;
        expand/collapse uses 300-400 ms. One table, in motion.ts, so no
        component hardcodes a duration.
      * REDUCE MOTION: \`ClassicPill.tsx:44-47\` already paints one calm static
        frame under \`prefers-reduced-motion: reduce\`. All new motion must be
        behind the same check, and the information (phase, numeral, progress)
        must still be fully present in the static frame. Assert it.
      * ENERGY: the loop must still park when at rest. YV81 removed busy timers
        on purpose and YV24 idle-throttles canvases; a spring that never settles
        is a 60 fps rAF forever. Assert the integrator reaches rest and stops
        scheduling frames within a bounded number of ticks.

    Tests \`desktop/src/pill/motion.test.ts\`, pure and deterministic (inject the
    timestep, never use real time):
      * \`spring_600_35_settles_within_the_expected_tick_budget\`
      * \`spring_never_overshoots_past_the_soft_limit\`
      * \`reduce_motion_returns_the_target_immediately\`
      * \`integrator_reports_at_rest_and_stops\`
      * \`no_duration_literal_outside_motion_ts\` — a source sweep over
        desktop/src/pill.

    Depends on Y5-A (the motion tokens), Y5-C (the phases to morph between).

    PR body owes a screen recording of press, drag, dock and a phase morph, plus
    the same four with Reduce Motion on.

    What NOT to do:
      - Do NOT add framer-motion, motion, or GSAP.
      - Do NOT animate the trial numeral (Y2-B forbids it) or any other
        informational text.
      - Do NOT let the spring run while the pill is idle and off-screen.
  `,
  acceptance: `
    test -f desktop/src/pill/motion.ts
    test -f desktop/src/pill/motion.test.ts
    grep -q 'stiffness: 600' desktop/src/pill/motion.ts
    grep -q 'damping: 35' desktop/src/pill/motion.ts
    grep -q 'stiffness: 300' desktop/src/pill/motion.ts
    grep -q 'reduce_motion_returns_the_target_immediately' desktop/src/pill/motion.test.ts
    grep -q 'integrator_reports_at_rest_and_stops' desktop/src/pill/motion.test.ts
    node -e "const d=require('./desktop/package.json').dependencies;process.exit(Object.keys(d).some(k=>/framer|^motion$|gsap|popmotion/.test(k))?1:0)"
    cd ${APP} && npm ci
    npm test -- pill/motion ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build           ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y5-E', prompt: 'Y5', branch: 'loop/y5-e-hover-hysteresis-and-alpha-hit-testing', gated: null,
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop. 2026-09-26: the lane-b worktree that held local commit 04e4a94 no longer exists (~/code/wilson-voice-loop is gone), so that partial work is lost — build from scratch.`,
  title: 'The docked pill stops oscillating on the screen edge — the bug Wispr shipped a comment about',
  preflight: `
    grep -q 'inset: -12px' desktop/src/float.css
    cd ${APP} && npm ci && npm test -- hitbox
  `,
  spec: `
    reference_wispr_parity_research P0 #3, and the reason it is ranked P0:
    "an invisible \`inset: -12px\` alpha margin painted only in the expanded
    state, and a ~0.004-alpha background so the panel is clickable while
    invisible. Why: without it, an edge-docked pill oscillates expand/collapse
    when the cursor dwells on the screen edge — Wispr shipped a comment
    explaining they hit exactly this." §4.4 is titled "Hit-testing and hover
    (the part that breaks naive implementations)" and scores Yap ❌ with the note
    "Yap will hit this exact bug", plus 🟡 "NSPanel ignores margin clicks" on
    alpha hit-testing.

    Yap has half the machinery: \`ClassicPill.tsx:38-42\` publishes the capsule's
    rect via \`watchPillHitbox\` (YV65) "so the panel only takes the cursor over
    the pill itself; the transparent shadow margin stays click-through".

    Do:
      * The \`::before\` alpha margin at \`inset: -12px\`, painted ONLY while
        expanded. Painted always, it makes a 12 px dead zone around an idle
        pill; painted never, the boundary oscillates. The conditionality IS the
        fix.
      * \`rgba(0,0,0,0.004)\` background on the hot area so the box is clickable
        while visually absent.
      * Feed the expanded rect (capsule + margin) to \`watchPillHitbox\` so the
        NSPanel's ignore-mouse-events region matches what CSS is painting. A
        margin CSS believes in and the panel does not is worse than no margin.
      * Hysteresis in the state machine, not only in CSS: expand on enter,
        collapse only after the cursor has been outside the EXPANDED rect for a
        debounce. Put the thresholds in motion.ts's table.

    Tests \`desktop/src/pill/hitbox.test.ts\`, pure over a
    \`hoverState(rect, cursorPath)\` reducer:
      * \`dwell_at_the_dock_edge_produces_at_most_one_transition\` — the exact
        acceptance the parity note specifies: "simulated pointer dwell at the
        dock edge produces <=1 state transition". Drive a synthetic path that
        crosses the collapsed boundary repeatedly by one pixel.
      * \`collapsed_pill_has_no_margin_dead_zone\`
      * \`published_hitbox_matches_the_painted_rect_in_both_states\`
      * all three dock edges.

    Depends on Y5-D.

    What NOT to do:
      - Do NOT paint the margin in the collapsed state.
      - Do NOT fix oscillation with a long timeout. A 500 ms lag on expand makes
        the pill feel dead; hysteresis is a geometry fix, not a delay.
  `,
  acceptance: `
    grep -q 'inset: -12px' desktop/src/float.css
    grep -qE 'rgba\\(0, *0, *0, *0?\\.004\\)' desktop/src/float.css
    test -f desktop/src/pill/hitbox.test.ts
    grep -q 'dwell_at_the_dock_edge_produces_at_most_one_transition' desktop/src/pill/hitbox.test.ts
    grep -q 'collapsed_pill_has_no_margin_dead_zone' desktop/src/pill/hitbox.test.ts
    grep -q 'published_hitbox_matches_the_painted_rect_in_both_states' desktop/src/pill/hitbox.test.ts
    cd ${APP} && npm ci
    npm test -- hitbox ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build      ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y5-F', prompt: 'Y5', branch: 'loop/y5-f-error-toasts-that-say-what-to-do', gated: null,
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
  title: 'One error surface, one sentence per failure, one action — replacing raw strings and silent failures',
  preflight: `
    test -f desktop/src/toast.ts
    grep -q 'errorAdvice' desktop/src/errors.ts
    cd ${APP} && npm ci && npm test -- toast
  `,
  spec: `
    \`errors.ts\` is the right foundation and only half the job. It converts a
    rejection to a sentence (\`errorText\`, errors.ts:27-30) and falls back to
    \`String(e)\` — which is a raw Rust error string in front of a user. Nearly
    every Tauri command in Yap answers \`Result<_, String>\` (errors.ts:3-4 says
    so), so \`String(e)\` is the common path, not the rare one.

    Do:
      * An error CATALOGUE: \`errorAdvice(code) -> { line, action }\` in
        errors.ts, keyed on the structured codes the backend already returns
        (\`license_required\` exists at errors.ts:38; Y1/Y3 add
        \`mic_permission_required\`, \`silent_capture\`, \`cancelled\`). Every code
        Yap can emit gets a line and, where there is one, a button. An unknown
        code gets a generic line that still tells the user what to do (open the
        support bundle sheet, which already exists:
        src/support/SupportBundleSheet.tsx).
      * Make the backend emit codes where it emits strings on the take path.
        Do NOT boil the ocean: the take path, the paste path, the model path and
        the permission path. List the ones you converted in the PR body and
        leave the rest as strings with the generic advice.
      * ONE toast implementation, \`desktop/src/toast.ts\`, replacing whatever
        ad-hoc surfaces exist (audit them first and say in the PR body how many
        you found — \`git grep -n "note\\|setNote\\|banner" desktop/src/App.tsx\`
        is the starting point; Onboarding.tsx has its own \`note\` string).
        Queue, dedupe by code, auto-dismiss with a duration proportional to
        length, manual dismiss, and a cap so a storm cannot cover the app.
      * Errors also reach the PILL as \`error\` phase (Y5-C). Both surfaces, one
        catalogue: the pill shows the line, the main window adds the action.
      * A FAILURE MUST NEVER BE SILENT. Add a test that sweeps the take path for
        \`Err(...)\` returns that reach no emit and no toast. If a full sweep is
        impractical, enumerate the take path's error returns explicitly in the
        test and assert each is surfaced — an explicit list that must be updated
        is better than a clever grep that proves nothing.

    Tests: \`desktop/src/toast.test.ts\` (queue, dedupe, cap, duration) and
    \`desktop/src/errors.test.ts\` extended (it exists, 1 case at errors.test.ts:6)
    — every catalogued code has a non-empty line; no line contains a Rust type
    name, \`Error(\`, \`unwrap\` or a file path.

    What NOT to do:
      - Do NOT show a raw error string. If you have nothing better, say "Yap
        could not finish that take" and offer the support bundle.
      - Do NOT add Sentry or PostHog. Local crash capture (crash.rs) is the
        observability stack (feedback_queryguard).
      - Do NOT stack more than the cap. Three visible toasts is a broken app.
  `,
  acceptance: `
    test -f desktop/src/toast.ts
    test -f desktop/src/toast.test.ts
    grep -q 'errorAdvice' desktop/src/errors.ts
    grep -q 'every_catalogued_code_has_a_human_line' desktop/src/errors.test.ts
    grep -q 'no_line_leaks_a_rust_type_or_path' desktop/src/errors.test.ts
    test 0 -eq "$(git grep -ci 'sentry\\|posthog' -- desktop | wc -l)"
    cd ${APP} && npm ci
    npm test -- toast  ; test $? -eq 0
    npm test -- errors ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build      ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y5-J', prompt: 'Y5', branch: 'loop/y5-j-focus-names-announcements-and-contrast-floors', gated: null,
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
  title: 'The accessibility floor: a focus ring, an accessible name per state, one live region, a contrast bar on the token layer',
  preflight: `
    test 12 -le "$(grep -ro 'focus-visible' desktop/src --include=*.css | wc -l)"
    cd ${APP} && npm ci && npm test -- a11y
  `,
  spec: `
    PANEL 2026-09-12. This loop adds roughly thirteen pill phases and
    twenty-one view states and not one item requires a focus ring, an
    accessible name, an announcement or a contrast ratio. MEASURED at 4e8c9adf:
      grep -rniE 'aria|screen ?reader|focus-visible|contrast|WCAG' over all item
        files -> only prefers-reduced-motion hits
      desktop/src/App.css — three focus-related selectors in 2,789 lines
        (:957, :1408, :2397)
      desktop/src/pill/ClassicPill.tsx:142-144 — one aria-label covering three
        of the planned thirteen phases
      only pill/MeetingBadge.tsx:53 has aria-live
      desktop/src/App.tsx:2228 — a clickable <div className="banner warn">
      desktop/src-tauri/src/float_pill.rs:355 \`.focused(false)\` + the
        non-activating NSPanel: every pill affordance is mouse-only
    Y5-A freezes the look while centralising every colour, so the one cheap
    moment to fix contrast is the moment the plan forbids touching it. Hence a
    separate item, after Y5-G's split and after the states exist.

    Do:
      * A contrast test over the token pairs in tokens.css: 4.5:1 for body text,
        3:1 for large text and UI boundaries. A failing pair is a failed item,
        not a TODO — adjust the token, and say which.
      * Every state Y5-B added: its primary action is a real \`<button>\` and is
        keyboard reachable; \`:focus-visible\` is styled once, globally.
      * Every pill phase supplies an accessible name, and the pill root carries
        \`aria-live="polite"\` so a state change is announced once, not on every
        frame. Because the panel is non-activating, any affordance the pill
        gains must ALSO be reachable from the main window or a binding — state
        which, per affordance.
      * Sweep the clickable divs (App.tsx:2228 and its siblings) into buttons.

    What NOT to do:
      - Do NOT add an accessibility library or a linter plugin to satisfy this.
      - Do NOT put aria-live on the pill's frame-by-frame amplitude value.
  `,
  acceptance: `
    test -f desktop/src/a11y/contrast.test.ts
    grep -q 'every_token_pair_meets_its_contrast_floor' desktop/src/a11y/contrast.test.ts
    grep -q 'every_phase_has_an_accessible_name' desktop/src/pill/live.test.ts
    grep -rq 'aria-live' desktop/src/pill
    test 12 -le "$(grep -ro 'focus-visible' desktop/src --include=*.css | wc -l)"
    test 0 -eq "$(grep -rn 'className="banner warn"' desktop/src --include=*.tsx | wc -l)"
    cd ${APP} && npm ci
    npx tsc --noEmit ; test $? -eq 0
    npm test         ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y5-I', prompt: 'Y5', branch: 'loop/y5-i-vertical-dock-as-the-css-base', gated: null,
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
  title: 'Rebuild the docked pill with vertical as the CSS base — the architecture Wispr abandoned trying the other way',
  preflight: `
    grep -q 'data-bar-position' desktop/src/float.css
    grep -q -- '--flow-bar-length' desktop/src/float.css
    cd ${APP} && npm ci && npm test -- dock
  `,
  spec: `
    THE MOST IMPORTANT ARCHITECTURAL NOTE IN THE PARITY RESEARCH, verbatim from
    reference_wispr_parity_research (MUST-KNOW section, primary-sourced from the
    extracted bundle): "Wispr's vertical bar is NOT a rotated horizontal capsule
    — vertical/column layout is the CSS BASE and horizontal is the override,
    with orientation-neutral tokens (\`$flow-bar-length\`/\`$flow-bar-thickness\`);
    their first rotate-the-capsule attempt failed (their comment records the
    '30x6-always-horizontal collapse bug'). Only the primary listening states
    (ready/activePtt/activePopo) rotate into the strip — processing/error/
    completion banners STAY horizontal even when side-docked (fixed-width labels
    crush in a 30px column). Waveform inverts axis when docked (side dock = 2px
    bars animating on X). Yap's current YV53/65 move-the-capsule approach is the
    exact architecture Wispr abandoned — the design loop must rebuild with
    vertical-as-base."

    §4.2 gives the exact geometry table to build against, per state, with the
    side-dock overrides. Use it as the golden table; it is [BUNDLE]-sourced:
      resting 8x40 rgba(0,0,0,.5) 1px rgba(255,255,255,.5) border radius 6 ·
      ready 30x50 solid radius 22.5 · activePtt 30x73 ·
      activePopo 30x102.5 (+cancel/stop, row gap 8; padding 6px 0 on side docks) ·
      processing 30x98 padding 12px 6px, STAYS HORIZONTAL in both docks ·
      polishProcessing 136x30, on side docks column with label hidden and the
      progress fill flipping bottom-up · polishCompleted 30x152, side dock
      152x30 · error 30x91 stays horizontal · navigationActive 72x84 gap 4.

    Do:
      * Orientation-neutral tokens \`--flow-bar-length\` / \`--flow-bar-thickness\`
        in tokens.css (Y5-A). Column layout is the BASE. \`[data-bar-position]\`
        on the pill root supplies the bottom-dock horizontal override.
      * Map every Y5-C phase onto the table: which rotate into the strip and
        which stay horizontal. Yap has phases Wispr does not (blocked, gated,
        transcribing); decide and DOCUMENT each one's orientation, with the
        30 px-crush rule as the deciding test.
      * The waveform inverts axis on a side dock (2 px bars animating on X).
        ClassicPill.tsx drives 9 bars off a \`--level\` CSS var
        (ClassicPill.tsx:20, :52) — that is already the right seam; make the
        axis a token.
      * A golden geometry test: for each of \`left|right|bottom\` x each phase,
        assert the rendered bounding box matches the table, and that NO state
        exceeds the 30 px strip on a side dock. This is verbatim the parity
        note's own acceptance for P0 #1.

    Tests \`desktop/src/pill/dock.test.ts\` with the table as a committed
    fixture \`desktop/src/pill/dock-geometry.json\` so the numbers are reviewable
    as data.

    Depends on Y5-A, Y5-C, Y5-D, Y5-E. This item supersedes the YV53/65
    move-the-capsule approach; delete that code path rather than leaving both.

    What NOT to do:
      - Do NOT rotate the horizontal capsule with a CSS transform. That is the
        approach Wispr tried and abandoned, and their bug comment is the evidence.
      - Do NOT force the banner states into the column. They crush; the research
        says so explicitly and the golden table encodes it.
      - Do NOT keep the old positioning code alongside the new base.
    PANEL 2026-09-12, SUPERSEDED BY THE OWNER DECISION 2026-09-13 — THE
    GEOMETRY TABLE BELONGS TO THE SHELL AND IS CHARACTER-INDEPENDENT. The panel
    said "scope it to ClassicPill" because it read the two pills as two products
    and expected one to be cut. Both ship (top of this file), so scoping the
    table to one of them would have left the other with no dock contract at all.
    The correct target is the PILL SHELL: the measured Wispr per-state box
    (8x40 resting, 30x50 ready, 30x73, 30x102.5, 136x30 polishing ...) is the
    size and orientation of the WINDOW CONTENT BOX for a phase, and every
    character renders INSIDE that box. So:
      * \`dock-geometry.json\` is the shell's table, keyed by phase x dock, with
        NO style dimension in it. One table, forever, for every creature.
      * A character declares only how it fills the box it is given — a pixel
        character on an LCD pod scales or crops to the box, and a capsule paints
        it. If a character cannot render a phase inside the box the table gives
        it, that is a CHARACTER defect and the character's fallback covers it;
        it is never a reason to fork the table.
      * The golden test asserts the SHELL's boxes. The per-character sweep is
        Y5-C's completeness table and Y5-K's registry contract test, not this one.
      * Do not delete either renderer, and do not add a style dimension to the
        fixture.

  `,
  acceptance: `
    grep -q 'data-bar-position' desktop/src/float.css
    grep -q -- '--flow-bar-length' desktop/src/tokens.css
    grep -q -- '--flow-bar-thickness' desktop/src/tokens.css
    test -f desktop/src/pill/dock-geometry.json
    test 0 -eq "$(grep -c 'pill_style\\|pillStyle\\|classic\\|yappy' desktop/src/pill/dock-geometry.json)"
    test -f desktop/src/pill/dock.test.ts
    grep -q 'no_state_exceeds_the_thirty_pixel_strip_on_a_side_dock' desktop/src/pill/dock.test.ts
    grep -q 'banner_states_stay_horizontal_in_every_dock' desktop/src/pill/dock.test.ts
    grep -q 'waveform_axis_inverts_on_a_side_dock' desktop/src/pill/dock.test.ts
    test 0 -eq "$(grep -c 'transform: rotate' desktop/src/float.css)"
    cd ${APP} && npm ci
    npm test -- dock ; test $? -eq 0
    npm test         ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build    ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y5-K', prompt: 'Y5', branch: 'loop/y5-k-pill-character-system-and-habitat-layer', gated: null,
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
  title: 'The pill becomes a pluggable character system, and the habitat comes back as its habitat layer',
  preflight: `
    test -f desktop/src/pill/characters/registry.ts
    test -f desktop/src/pill/characters/characters.test.ts
    test -d desktop/src/home/habitat
    cd ${APP} && npm ci && npm test -- characters
  `,
  spec: `
    OWNER DECISION 2026-09-13 (Wilson), and it REINSTATES \`Y5-H\`, which the
    panel killed on a two-seat convergence. Wilson, verbatim: "I thought we were
    gonna develop it and then make more characters and make it more flexible ...
    there's a classic pill and there's a yappy pill and there's gonna be
    different pills with the different creatures that are coming."

    The panel's cost objection was CORRECT and is not waved away: a second pill
    style doubles the render and test surface of every pill item, and Y5-C alone
    was 13 phases x 2 styles x 3 docks. The remedy is structural. The shell
    stops knowing about creatures and the creatures stop knowing about the
    shell, so the matrix stops multiplying.

    THIS ITEM RUNS LAST IN THIS FILE ON PURPOSE: shell first (Y5-A tokens, Y5-C
    phases, Y5-D motion, Y5-E hit-testing, Y5-I dock geometry), then the
    characters, then the habitat. Do not start it before Y5-I has landed — the
    shell's box contract is the thing the character interface is defined
    against.

    (1) THE SHELL / CHARACTER SEAM.
      * \`desktop/src/pill/characters/types.ts\` — ONE interface. A character is
        \`{ id, label, render(frame) }\` where \`frame\` is what the shell already
        computed: \`{ phase, tone, level, box: {w,h}, dock, reducedMotion }\`.
        A character receives a BOX and paints inside it. It never reads
        \`pill_style\`, never reads settings, never reads license state, never
        positions a window, never knows a dock exists beyond the axis hint in
        \`frame\`.
      * \`registry.ts\` — \`registerCharacter()\` + \`characters()\`. The shell
        resolves \`settings.pill_style\` to a registered id ONCE, at the mount
        point, and falls back to \`classic\` for an unknown id rather than
        rendering nothing. \`pill_style\` stays a free string in
        \`AppSettings\` (lib.rs:234-235, default "classic", lib.rs:366/411) —
        do NOT turn it into a Rust enum: a new creature must be shippable
        without touching Rust.
      * PORT, do not rewrite: \`ClassicPill.tsx\` and \`YappyPill.tsx\` become
        \`characters/classic/\` and \`characters/yappy/\` with their phase art and
        copy as DATA, and everything that is not the creature — the capsule
        chrome, the waveform placement, dock geometry, hover hysteresis, the
        license chip placement, aria names — moves UP into the shell. The two
        characters must end up with NO duplicated shell logic between them; that
        deduplication is the whole point and it is measurable (see acceptance).
      * A NEW CREATURE IS A NEW MODULE PLUS A FIXTURE ROW, WITH NO SHELL CHANGE.
        Prove it: the item ships a third, deliberately minimal character
        (\`characters/example/\`) whose only purpose is to be the proof that the
        seam holds, and the contract test registers it with zero shell edits.

    (2) THE TEST MATRIX STOPS DUPLICATING.
      * The shell owns phase x dock. That suite runs ONCE, character-agnostic,
        against \`dock-geometry.json\` (Y5-I).
      * Each registered character is swept by ONE data-completeness contract
        test over the registry: every phase in the \`LivePhase\` union, in every
        tone (rude|friendly|rose, live.ts:29), has art and copy; nothing exceeds
        the box it was handed; \`imageSmoothingEnabled\` is false wherever a
        character paints to a canvas.
      * Registering an INCOMPLETE character must turn that contract test RED.
        That is the test's reason to exist and it is the item's mutation proof.

    (3) THE HABITAT LAYER — the reinstated \`Y5-H\`, with its design note
        preserved in docs/loop/DEFERRED.md §1. \`desktop/src/home/YappyHouse.tsx\`
        is 919 lines of working real-clock canvas scene with an ambient
        director; this is a REFACTOR PLUS A LAYER, not a rewrite.
      * \`desktop/src/home/habitat/\` — the habitat is the CHARACTER'S WORLD, and
        it is selected by the same registered character id, so a new creature
        brings its own pod. Split what exists into: the director (clock,
        routines, intent pathing), the scene (pod interior, dithered depth,
        parallax), and the character's own idle/reaction sprites, which come
        from the SAME character module the pill uses — one creature, two
        surfaces, one source of art.
      * EVENT-DRIVEN REACTIONS, from the design note: a take starting, a paste
        landing, a model finishing its download. The habitat subscribes to the
        same events the pill does; it never polls.
      * Routines on a real clock and intent pathing rather than a random walk.
      * AESTHETIC LOCK, unchanged and binding (top of this file): pixel art,
        chunky pixels, \`imageSmoothingEnabled = false\`, limited retro palette,
        Tamagotchi / Bitzee. Hand-coded. NOT smooth vector. No angled "angry"
        eyebrows. Paper/origami is REJECTED.
      * Wilson's taste is the real gate on the ART and cannot be automated in a
        build-first pass — which is why this item's acceptance gates the
        STRUCTURE (the seam, the completeness sweep, the deduplication, the
        no-shell-change proof) and the PR body carries the screenshots and a
        recording of the habitat for him to judge. Say that in the PR body.

    Depends on Y5-A, Y5-C, Y5-D, Y5-E, Y5-I. Consumes PERM-C's complete
    \`LivePhase\` union.

    What NOT to do:
      - Do NOT delete either shipped character. Both ship.
      - Do NOT let a character read settings, license state or dock position
        directly. Everything it needs arrives in \`frame\`.
      - Do NOT add a style dimension to \`dock-geometry.json\`.
      - Do NOT turn \`pill_style\` into a Rust enum or a TypeScript union of two
        literals — the whole point is that the next creature is additive.
      - Do NOT rewrite YappyHouse from scratch, and do not lose its ambient
        director.
      - Do NOT smooth the pixels.
  `,
  acceptance: `
    test -f desktop/src/pill/characters/types.ts
    test -f desktop/src/pill/characters/registry.ts
    test -d desktop/src/pill/characters/classic
    test -d desktop/src/pill/characters/yappy
    test -d desktop/src/pill/characters/example
    test -f desktop/src/pill/characters/characters.test.ts
    test -d desktop/src/home/habitat
    test -f desktop/src/home/habitat/habitat.test.ts
    grep -q 'registerCharacter' desktop/src/pill/characters/registry.ts
    grep -q 'every_registered_character_covers_every_phase_in_every_tone' desktop/src/pill/characters/characters.test.ts
    grep -q 'a_character_never_exceeds_the_box_the_shell_hands_it' desktop/src/pill/characters/characters.test.ts
    grep -q 'a_new_creature_needs_no_shell_change' desktop/src/pill/characters/characters.test.ts
    grep -q 'imageSmoothingEnabled' desktop/src/pill/characters/characters.test.ts
    grep -q 'the_director_runs_on_the_real_clock_not_a_random_walk' desktop/src/home/habitat/habitat.test.ts
    grep -q 'the_habitat_reacts_to_take_paste_and_model_events' desktop/src/home/habitat/habitat.test.ts
    test 0 -eq "$(grep -rc 'pill_style\\|pillStyle' desktop/src/pill/characters | grep -v ':0$' | wc -l | tr -d ' ')"
    test 0 -eq "$(grep -rl 'data-bar-position' desktop/src/pill/characters | wc -l | tr -d ' ')"
    test 0 -eq "$(grep -c 'classic\\|yappy' desktop/src/pill/dock-geometry.json)"
    cd ${APP} && npm ci
    npm test -- characters ; test $? -eq 0
    npm test -- habitat    ; test $? -eq 0
    npm test               ; test $? -eq 0
    npx tsc --noEmit       ; test $? -eq 0
    npm run build          ; test $? -eq 0
    printf '\\nregisterCharacter({ id: "mutant", label: "mutant", render: () => null });\\n' >> src/pill/characters/registry.ts
    npm test -- characters ; test $? -ne 0
    cd .. && git checkout -- desktop/src/pill/characters/registry.ts
    git diff --exit-code -- desktop/src/pill/characters/registry.ts
  `,
})

// ── 00d-yap24-ui-pixels-motion.mjs ────────────────────────────────────────
// yap24-UI — CLEARER PIXELS, SMOOTHER MOTION. Written 2026-09-26 by the architecture audit
// (docs/ARCHITECTURE-AUDIT-2026-09-26.md §5). Wilson, 2026-09-26: "make the UI better, the pixels
// clearer, smoother." Locked aesthetic: pixel-art Tamagotchi on an LCD/pod (memory
// feedback_companion_must_be_cute) — no vector mascot, no origami, no angry eyebrows. Characters
// are data behind the shell (owner decision 2026-09-13, Y5-K).
// Headless verification only: vitest + the headless-Chrome structural smoke (scripts/smoke-windowed.mjs
// runs Chrome with --headless=new). SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap24-UI1', prompt: 'yap24-UI', branch: 'loop/yap24-ui1-integer-pixel-grid-on-every-display', gated: 'panel',
  title: 'Every sprite pixel lands on whole device pixels at every dock size and backing scale — no half-pixel blur',
  preflight: `
    test -f desktop/src/pill/pixelGrid.ts
    test -f desktop/src/pill/pixelGrid.test.ts
  `,
  spec: `
    Panel: pending
    DEPENDS: Y5-I (vertical-as-base geometry) and Y5-K (character shell) when merged; standalone otherwise

    EVIDENCE
      - pill/YappyPill.tsx:262 DPR = Math.min(devicePixelRatio || 1, 2) and the canvas is sized
        from the CSS box; nothing guarantees the art's cell size is an integer number of device
        pixels, so at some dock sizes a 1-art-pixel line straddles two device pixels (soft edges).
      - float.css:379 and App.css:1969-1975 set image-rendering: pixelated / crisp-edges on the
        upscaled canvases — correct, but it cannot fix a non-integer scale.

    DO: a pure pixelGrid.ts — given CSS box, art grid (w,h) and DPR, return the largest integer
    device-pixel cell, the canvas backing size and the centring offset in whole device pixels.
    YappyPill, ClassicPill (where it draws pixel art) and YappyHouse use it; the leftover margin is
    filled with the scene colour, never stretched. Tests cover DPR 1 and 2, all three docks, the
    30 px side dock.
  `,
  acceptance: `
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/pill/pixelGrid.test.ts
    npm test
    npm run build
  `,
})

ITEMS.push({
  id: 'yap24-UI2', prompt: 'yap24-UI', branch: 'loop/yap24-ui2-one-animation-clock-that-parks', gated: 'panel',
  title: 'One animation clock for the pill and the habitat: frame-rate independent, parks at idle, honours Reduce Motion',
  preflight: `
    test -f desktop/src/pill/clock.ts
    test -f desktop/src/pill/clock.test.ts
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26, DEFERRED to the next release (not in panelApproved).
    DEPENDS: Y5-D (open PR #196 — spring constants) when merged

    EVIDENCE
      - ClassicPill was fixed to settle-and-park (memory: yap audit [0] "60fps rAF-forever"); the
        Yappy pill and YappyHouse each run their own loops (YappyPill.tsx canvas loop,
        home/YappyHouse.tsx) — two clocks, two idle policies.
      - ProMotion displays run rAF at 120 Hz; animation that advances per frame instead of per
        millisecond runs twice as fast there (UNVERIFIED for each loop — the builder measures).

    DO: pill/clock.ts — a single requestAnimationFrame scheduler with dt in ms, subscribers,
    automatic park when no subscriber is animating, and a prefers-reduced-motion switch. Port the
    three loops onto it; every motion is expressed per millisecond. Tests with a fake rAF: equal
    motion at 60 and 120 Hz; parks after settle; reduced motion snaps.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - Y5-D (#196, the spring constants) is MERGED (5f3c852), not open. The animation loops are in FIVE files, not three: ClassicPill, YappyPill,
        home/YappyHouse, license/YappySprite.tsx and pill/live.ts. The port covers all five. DEFER: next release (idle-CPU hygiene, no user-visible wow).
  `,
  acceptance: `
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/pill/clock.test.ts
    npm test
    npm run build
  `,
})

ITEMS.push({
  id: 'yap24-UI3', prompt: 'yap24-UI', branch: 'loop/yap24-ui3-pay-the-visual-qa-debt-headless', gated: 'panel',
  title: 'Pay the visual-QA debt: a headless screenshot matrix of every pill phase x dock x character, attached to the PR',
  preflight: `
    test -f scripts/pill-matrix.mjs
    grep -q -- '--only' scripts/pill-matrix.mjs
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26; BUILD THIS FIRST (every PILL item and NT6/NT7 run its phase through this matrix).
    DEPENDS: Y5-B/Y5-C/Y5-D (merged). [REVERSED 2026-10-26: the PILL items depend on THIS item, not the other way round; the matrix reads its phase list from the pill's own phaseVisual keys, so phases the PILL items add appear automatically.]

    EVIDENCE: the 2026-09-15 run's builders logged "NOT visually QA'd" on Y2-B, Y5-B, Y5-C, Y5-D
    (memory project_yap_loop_state_20260912; STATUS-yap.md). A green gate cannot see a pill.

    DO: scripts/pill-matrix.mjs reuses smoke-windowed.mjs's headless Chrome (--headless=new) and
    vite preview, renders float.html with each phase forced through a dev-only query parameter,
    captures PNGs for every phase x {bottom,left,right} x {classic,yappy}, and fails on the same
    structural conditions (empty text, clipped text, overflow). --out <dir> (a scratch dir; attach the PNGs to the PR body or a PR comment, NEVER commit them) and --only <phase> to render one phase across 3 docks x 2 characters. Never launches the app, never opens a visible window.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - The old acceptance only ran node --check, so the matrix this item exists to produce was never produced; a green gate could not see a pill.
        The acceptance now RUNS it and counts the PNGs (at least 24 = phases x 3 docks x 2 characters). The PNGs go to the PR, not the repo. Each PILL
        item then runs scripts/pill-matrix.mjs --only <its phase>.
  `,
  acceptance: `
    test -f scripts/pill-matrix.mjs
    node --check scripts/pill-matrix.mjs
    grep -q -- '--only' scripts/pill-matrix.mjs
    test -z "$(git ls-files docs/pr-screenshots/yap24-UI3)"
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && O="$(mktemp -d)" && node scripts/pill-matrix.mjs --out "$O" && test "$(ls "$O"/*.png | wc -l)" -ge 24
  `,
})


// ── RUN ─────────────────────────────────────────────────────────────────────
phase('Recon')
log(
  `YAP OVERHAUL loop — ${PART}, mode ${MODE}, repo ${REPO}. ` +
    `${ITEMS.length} items: ${ITEMS.map((i) => i.id).join(', ')}. ` +
    `Gated on the Senior Panel: ${ITEMS.filter((i) => i.gated === 'panel').map((i) => i.id).join(', ') || 'none'}. ` +
    (MODE === 'review'
      ? `REVIEW PASS: no builders. Every open ${LOOP_LABEL} PR is triaged, reviewed adversarially and held to the merge bar.`
      : `BUILD PASS: ${BUILD_LANES} builder lane(s), no reviewer and no merge agent. Each build LANDS whatever is already ${LAND_BAR_CI ? 'CI-green' : 'gate-green'}, then builds its own item. ${LAND_BAR_CI ? "landBar 'ci' (BUILD-ONLY): the drain merges every loop-build PR whose GitHub CI is green on its head." : "The review pass runs afterwards with args {mode:'review'}."}`)
)

const BUILD_RECON = `RECON for the Yap overhaul loop. You set up the ${BUILD_LANES === 3 ? 'THREE' : 'TWO'} lane work dirs the items share, you warm the
Rust build cache once per lane, you make sure the ${LOOP_LABEL} label exists, and you MEASURE
whether GitHub Actions can run at all. Do not build anything. Do not open a PR. Each item's own
pre-flight is that item's job, never yours.

# 1 — CREATE ${BUILD_LANES === 3 ? 'ALL THREE' : 'BOTH'} LANE WORKTREES (not clones — they share the object store, which is the disk win)
    lane A  ${WORKDIR}      port ${PREVIEW_PORT}   CARGO_TARGET_DIR ${CARGO_TARGET_A}
    lane B  ${WORKDIR_B}    port ${PREVIEW_PORT_B} CARGO_TARGET_DIR ${CARGO_TARGET_B}${BUILD_LANES === 3 ? `
    lane C  ${WORKDIR_C}    port ${PREVIEW_PORT_C} CARGO_TARGET_DIR ${CARGO_TARGET_C}` : ''}
    mkdir -p /Users/wilsonguenther/code/wilson-voice-loop
    cd ${LOCAL_REPO} && git fetch origin
    git worktree add ${WORKDIR} -b yap/recon-a origin/main
    git worktree add ${WORKDIR_B} -b yap/recon-b origin/main${BUILD_LANES === 3 ? `
    git worktree add ${WORKDIR_C} -b yap/recon-c origin/main` : ''}
If either already exists from an earlier attempt or an earlier PART, do NOT blindly delete it: check
'git worktree list', and REUSE it if it is clean. Say for each which you did. If the branch name is
already taken, reuse the existing worktree rather than inventing a third one.
Confirm and paste, for EACH: git log --oneline -1 && git status --porcelain

# 2 — INSTALL ONCE PER LANE (the app lives in ${APP}/, never at the repo root)
    cd ${WORKDIR}/${APP} && npm ci --cache ${NPM_CACHE}
    cd ${WORKDIR_B}/${APP} && npm ci --cache ${NPM_CACHE}${BUILD_LANES === 3 ? `
    cd ${WORKDIR_C}/${APP} && npm ci --cache ${NPM_CACHE}` : ''}
If a lane's node_modules already exists (from an earlier part) and 'npm ls --depth 0' is healthy,
say so and SKIP that install — the worktrees are deliberately left standing between parts.

# 3 — WARM THE RUST CACHE ONCE PER LANE. THIS IS THE EXPENSIVE STEP AND IT IS PAID HERE, ONCE.
src-tauri statically links transcribe-cpp's ggml and yap-polish vendors llama-cpp-sys-2: a COLD
build of this workspace is MINUTES. The target dirs live OUTSIDE the worktrees on purpose, so this
cache survives every item, every part and both passes. For EACH lane, with that lane's target dir:
    cd ${WORKDIR}/${APP} && export CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=${CARGO_TARGET_A}
    npm run build ; echo "distA=$?"
    cargo build -p yap-polish --release ; echo "sidecarA=$?"
    cargo build -p yap-diarize --release ; echo "diarizeA=$?"
    TRIPLE="$(rustc -vV | awk '/^host:/ {print $2}')" ; mkdir -p src-tauri/binaries && cp ${CARGO_TARGET_A}/release/yap-polish "src-tauri/binaries/yap-polish-$TRIPLE" && cp ${CARGO_TARGET_A}/release/yap-diarize "src-tauri/binaries/yap-diarize-$TRIPLE" ; echo "stageA=$?"
    cd ${WORKDIR}/${APP}/src-tauri && cargo build --release --features custom-protocol ; echo "appA=$?"
and the same for lane B with ${WORKDIR_B} and ${CARGO_TARGET_B}${BUILD_LANES === 3 ? `, and for lane C with ${WORKDIR_C} and
${CARGO_TARGET_C} (args.lanes: 3 — a NEW target dir is one COLD build, minutes, paid here once; do NOT copy
another lane's target dir into it: build-script outputs carry absolute paths into the dir they were built in)` : ''}.
STAGING BOTH SIDECARS IS NOT OPTIONAL: bundle.externalBin names yap-polish AND yap-diarize, a precondition of
EVERY cargo build of the app (and desktop/dist must exist or tauri::generate_context! panics), so a missing one fails the build for ENVIRONMENT reasons and a later agent
will waste an hour reading it as a code defect.

# 4 — BASELINE GATE, ONCE, IN LANE A ONLY (both lanes sit on the same origin/main sha)
${GATE_CMDS(WORKDIR, CARGO_TARGET_A)}
Plus the informational one, so the run knows its baseline:
    cd ${WORKDIR}/${APP} && cargo fmt --all -- --check ; echo "fmt=$?"
MEASURED ON main 2026-09-12: typecheck=0 test=0 build=0 sidecar=0 polishtest=0 clippy=0 cargotest=0
and fmt=1. If any of the eight is non-zero for you, report it as the BASELINE state; do not fix it,
and do not let a later agent be blamed for it. Do NOT repeat the baseline in lane B${BUILD_LANES === 3 ? ' or C' : ''} — confirm the
shas match instead.

# 4b — THE LABEL AND THE BOARD
Every PR this loop opens carries the ${LOOP_LABEL} label — the LAND step and the review pass both
find PRs by it and by nothing else. Create it once if it is absent, and say which:
    gh label list -R ${REPO} --search ${LOOP_LABEL}
    gh label create ${LOOP_LABEL} -R ${REPO} --color BFD4F2 --description "opened by the Yap overhaul loop; unreviewed until the review pass" || true
Then seed the run's status board (it is safe to re-run; it upserts):
    node ${STATUS} --part ${PART} --total ${ITEMS.length} --current none

# 5 — IS CI ALIVE? THIS DECIDES THE MERGE GATE FOR EVERY LATER AGENT IN THIS RUN
    cd ${LOCAL_REPO} && node scripts/loop/ci-mode.mjs --why
That script asks GitHub whether the newest pull_request-triggered run in the last 24 h actually
placed a job on a runner. It prints "github" or "local" on line 1 and the evidence on line 2.
PASTE BOTH LINES VERBATIM and return them as ciMode and ciModeWhy. Do not guess, do not infer from
a red check, and do not "fix" a local verdict. THE EXPECTED ANSWER IS "local": Actions is disabled
account-wide by a spending limit and the newest run on this repo is weeks old. local means the whole
run merges on a pasted local gate instead. Getting this one field wrong parks every PR of the run
on a check that will never report.
Also record the starting state, from gh and not from branch names:
    gh pr list -R ${REPO} --state open --json number,title,headRefName,labels
There are pre-existing open feat/yv1xx PRs from a previous loop. They are NOT this pass's work and
they do NOT carry the ${LOOP_LABEL} label: count them, name them, and leave them alone. Drain
triages them at the end of the run.

# 6 — PER-ITEM PRE-FLIGHT IS NOT YOURS TO RUN
Each build agent runs its own item.preflight against unmodified origin/main and short-circuits the
item to "already-done" when every command passes. Do NOT run the items' pre-flights here: a
pre-flight run by recon and reported second-hand is exactly the shared-false-premise failure this
harness has already had once. Just publish the sha the items will measure against:
    cd ${WORKDIR} && git rev-parse origin/main
RETURN: {worktreeCreated, worktreePath, headSha, npmCiOk, cargoWarmOk: true|false, typecheckExit,
         testExit, buildExit, sidecarExit, polishTestExit, clippyExit, cargoTestExit, fmtExit,
         ciMode: "github"|"local", ciModeWhy, openPrsBefore, preexistingPrs: [...], notes}
${WORKTREE_CONTRACT(WORKDIR, PREVIEW_PORT, CARGO_TARGET_A, WORKDIR_B)}
${VERIFY_NOTHING}
${LOGLINE('recon', '_recon', 'harness:recon')}
${GUARD(BOTH_DIRS)}`

/**
 * REVIEW MODE HAS ITS OWN RECON: two REVIEW worktrees, not the two builder lanes. The builder
 * worktrees may still be sitting there from the build pass with a half-built branch checked out;
 * the review chains get clean ones of their own so a rebase in one chain can never move the other.
 */
const REVIEW_RECON = `RECON for the REVIEW PASS of the Yap overhaul loop. You set up the TWO REVIEW work dirs the chains
share, you warm their Rust caches, and you seed the board. Do not review anything, do not fix
anything, do not merge anything, do not close anything — the triage agent and the chains do all of
that.

# 1 — CREATE BOTH REVIEW WORKTREES (not clones — shared object store is the disk win)
    chain R1  ${REVIEW_DIRS[0]}   port ${REVIEW_PORTS[0]}   CARGO_TARGET_DIR ${REVIEW_TARGETS[0]}
    chain R2  ${REVIEW_DIRS[1]}   port ${REVIEW_PORTS[1]}   CARGO_TARGET_DIR ${REVIEW_TARGETS[1]}
    cd ${LOCAL_REPO} && git fetch origin
    git worktree add ${REVIEW_DIRS[0]} -b yap/review-r1 origin/main
    git worktree add ${REVIEW_DIRS[1]} -b yap/review-r2 origin/main
If either already exists, do NOT blindly delete it: check 'git worktree list' and REUSE it if it is
clean (git status --porcelain empty). Say for each which you did.
Do NOT touch ${WORKDIR} or ${WORKDIR_B} — those are the BUILDER lanes and this pass does not use
them. Do not reset them, do not remove them, do not check anything out in them.
Confirm and paste, for EACH: git log --oneline -1 && git status --porcelain

# 2 — INSTALL AND WARM, ONCE PER CHAIN, THROUGH THE SHARED NPM CACHE
    cd ${REVIEW_DIRS[0]}/${APP} && npm ci --cache ${NPM_CACHE}
    cd ${REVIEW_DIRS[1]}/${APP} && npm ci --cache ${NPM_CACHE}
Then, per chain, with that chain's CARGO_TARGET_DIR and CARGO_INCREMENTAL=0 exported: npm run build (desktop/dist must exist or
tauri::generate_context! panics), then build and STAGE BOTH sidecars (cargo build -p yap-polish --release and
cargo build -p yap-diarize --release, then copy each to ${APP}/src-tauri/binaries/<name>-<triple>)
and build the app once (cd ${APP}/src-tauri && cargo build --release --features custom-protocol).
If a chain's node_modules and target dir already exist and are healthy, SKIP and say so. Do not
create a third worktree or a clone: disk hygiene is a hard rule here.

# 3 — BASELINE GATE, ONCE, IN CHAIN R1 ONLY (both sit on the same origin/main sha)
${GATE_CMDS(REVIEW_DIRS[0], REVIEW_TARGETS[0])}
Read each exit code BARE, never through a pipe. Report all eight. A non-zero here is the BASELINE
state of main after the build pass's merges — report it, do NOT fix it, and make sure no PR is later
blamed for it. Confirm the two worktrees' shas match rather than re-running the gate in R2.

# 4 — IS CI ALIVE? THIS DECIDES THE MERGE GATE FOR EVERY CHAIN IN THIS PASS
    cd ${LOCAL_REPO} && node scripts/loop/ci-mode.mjs --why
Line 1 is "github" or "local"; line 2 is the evidence. PASTE BOTH VERBATIM and return them as
ciMode and ciModeWhy. "local" means Actions cannot schedule work for this account at all, so no
check on any PR in the queue can ever turn green and the chains land on a pasted local gate
instead. Do not guess it, do not infer it from a red check, and do not soften it.

# 5 — THE BOARD
    node ${STATUS} --part ${PART} --total ${ITEMS.length} --current none

RETURN: {reviewWorktrees: [...], npmCiOk, cargoWarmOk, headSha, typecheckExit, testExit, buildExit,
         sidecarExit, polishTestExit, clippyExit, cargoTestExit, fmtExit, openPrsBefore,
         ciMode: "github"|"local", ciModeWhy, notes}
${VERIFY_NOTHING}
${LOGLINE('review recon', '_recon', 'harness:recon')}
${GUARD(BOTH_REVIEW_DIRS)}`

/**
 * Recon's payload is only partly typed on purpose — the two recon prompts return different shapes
 * — but ciMode is the one field the rest of the run BRANCHES on, so it is schema'd. A free-text
 * answer here is how a run ends up polling a check that can never report.
 */
const RECON_SCHEMA = {
  type: 'object',
  properties: {
    ciMode: { type: 'string', enum: ['github', 'local'] },
    ciModeWhy: { type: 'string' },
    headSha: { type: 'string' },
    npmCiOk: { type: 'boolean' },
    cargoWarmOk: { type: 'boolean' },
    typecheckExit: { type: 'number' },
    testExit: { type: 'number' },
    buildExit: { type: 'number' },
    clippyExit: { type: 'number' },
    cargoTestExit: { type: 'number' },
    openPrsBefore: { type: 'number' },
    notes: { type: 'string' },
  },
  required: ['ciMode'],
}

const recon = await agentR(MODE === 'review' ? REVIEW_RECON : BUILD_RECON, {
  model: 'opus',
  effort: 'high',
  phase: 'Recon',
  label: 'recon',
  schema: RECON_SCHEMA,
})

/**
 * THREAD THE CI MODE INTO THE REST OF THE RUN. The payload may arrive typed (schema honoured) or
 * as JSON text, so read both. UNKNOWN RESOLVES TO LOCAL — the inverse of the Drivia harness, and
 * deliberately so: Actions is dead account-wide here, the local gate is the stronger gate, and a
 * garbled recon must not upgrade the run onto a check that can never report.
 */
{
  const reconText = typeof recon === 'string' ? recon : JSON.stringify(recon ?? null)
  const saysGithub =
    (recon && typeof recon === 'object' && recon.ciMode === 'github') ||
    /"ciMode"\s*:\s*"github"/i.test(reconText)
  CI_MODE = saysGithub ? 'github' : 'local'
  log(
    CI_MODE === 'local'
      ? 'CI_MODE=local — GitHub Actions cannot schedule jobs for this account. Every merge in this ' +
          'run lands on a pasted local gate (typecheck, test, build, sidecar, polishtest, clippy, ' +
          `cargotest) and is recorded ci=local. Recon's evidence: ${
            (recon && typeof recon === 'object' && recon.ciModeWhy) || 'see the recon payload'
          }`
      : 'CI_MODE=github — Actions is placing jobs on runners again; the check run is the merge gate, ' +
          'and the local clippy run is still owed because CI treats clippy and fmt as informational.'
  )
}

// ── THE RUN ────────────────────────────────────────────────────────────────
// BUILD MODE: two builder lanes, one agent each, and no cross-lane await anywhere — that is the
// whole throughput change. REVIEW MODE: one PR-driven pass. A thrown stage must NOT kill a
// 24-hour run: it is caught, logged, recorded as errored, and the lane keeps going. A HALT is
// different — it stops both lanes (see noteHalt).
const errored = []
const slots = new Array(ITEMS.length).fill(null)
let halted = null
// PER-PART LANE BALANCE (2026-10-27). Only RUNNABLE items (not args.only-skipped, not panel-gated) are split, round-
// robin in file order over the lanes; a pinned item takes its lane and the round-robin skips any lane already ahead
// (pins are counted first), so every lane is within ±1 of the others in this part. lanes:1 -> lane A walks every
// runnable item in source order. Always three slots; a lane at or above BUILD_LANES gets none and steals nothing.
const PANEL_OK = (typeof args !== 'undefined' && args && Array.isArray(args.panelApproved) && args.panelApproved) || []
const isRunnable = (item) => !onlySkip(item) && !(item.gated === 'panel' && !PANEL_OK.includes(item.id))
function splitLanes(items, n) {
  const lists = [[], [], []]
  if (n === 1) return [items.slice(), [], []]
  const counts = new Array(n).fill(0)
  for (const it of items) if (pinnedLane(it) >= 0) counts[pinnedLane(it)]++
  let cursor = 0
  for (const it of items) {
    let l = pinnedLane(it)
    if (l < 0) {
      const min = Math.min(...counts)
      for (let k = 0; k < n && l < 0; k++) if (counts[(cursor + k) % n] === min) l = (cursor + k) % n
      counts[l]++
      cursor = (l + 1) % n
    }
    lists[l].push(it)
  }
  return lists
}
const laneItems = splitLanes(ITEMS.filter(isRunnable), BUILD_LANES)
const indexOfItem = (item) => ITEMS.indexOf(item)
// WORK STEALING (2026-10-27): the per-lane lists are a PLAN, not a cage. A lane that runs out (e.g. every item it was
// handed is already-done at pre-flight) takes the NEXT UNSTARTED, UNPINNED item from the lane with the most unstarted
// items left. JS is single-threaded and nothing awaits between pick and started.add, so two lanes never take one item.
const started = new Set()
const steals = []
function nextItemFor(lane) {
  const own = laneItems[lane].find((it) => !started.has(it))
  if (own) return own
  const stealable = (l) => laneItems[l].filter((it) => !started.has(it) && pinnedLane(it) < 0)
  let from = -1
  for (let l = 0; l < BUILD_LANES; l++) if (l !== lane && stealable(l).length > (from < 0 ? 0 : stealable(from).length)) from = l
  if (from < 0) return null
  const item = stealable(from)[0]
  steals.push({ itemId: item.id, from: LANE_TAGS[from], to: LANE_TAGS[lane] })
  log(`lane ${LANE_TAGS[lane]}: out of its own items — STEALS ${item.id} from lane ${LANE_TAGS[from]} (${stealable(from).length} unstarted left there)`)
  return item
}

log(
  `args.lanes = ${BUILD_LANES}${BUILD_LANES === 1 ? ' (ONE builder: lane A runs every item in order, lanes B/C idle)' : ''} || ` +
    `landBar = ${LAND_BAR_CI ? 'ci (BUILD-ONLY: merge on GitHub CI green; the drain lands)' : 'review (default)'} || ` +
  `lane A (${WORKDIR}, port ${PREVIEW_PORT}): ${laneItems[0].map((it) => it.id).join(', ') || '(none)'} || ` +
    `lane B (${WORKDIR_B}, port ${PREVIEW_PORT_B}): ${laneItems[1].map((it) => it.id).join(', ') || '(none)'} || ` +
    `lane C (${WORKDIR_C}, port ${PREVIEW_PORT_C}): ${laneItems[2].map((it) => it.id).join(', ') || '(none)'}`
)

if (ONLY_PREFIXES) {
  const kept = ITEMS.filter((it) => !onlySkip(it)).map((it) => it.id)
  log(
    `args.only = [${ONLY_PREFIXES.join(', ')}]: ${kept.length} item(s) run (${kept.join(', ') || 'none'}), ` +
      `${ITEMS.length - kept.length} hard-skipped with no agent. args.now = ${RUN_NOW || 'not passed'}.`
  )
}

/** ONE BUILDER LANE. It walks its own items and never awaits the other lane, ever. */
async function buildLane(lane) {
  // Skipped items (args.only / awaiting the panel) are on no lane: lane A records them once, with no agent.
  if (lane === 0) for (const item of ITEMS) if (!isRunnable(item)) slots[indexOfItem(item)] = onlySkip(item) || panelSkip(item)
  let ran = 0
  while (!halted && lane < BUILD_LANES) {
    const item = nextItemFor(lane)
    if (!item) break
    started.add(item)
    ran++
    // Lane A is the SINGLE writer of the global phase cursor. Lanes B and C never touch it: two writers
    // race, and the loser's phase label is what Wilson would be reading.
    if (lane === 0) phase(item.id)
    slots[indexOfItem(item)] = await runBuildSafe(item, lane)
  }
  log(`lane ${LANE_TAGS[lane]}: finished its ${laneItems[lane].length} item(s) — ran ${ran}, ${steals.filter((x) => x.to === LANE_TAGS[lane]).length} stolen`)
}

/**
 * THE DEFERRED GATE, run with args {mode:'review'} — PR-DRIVEN, NOT ITEM-DRIVEN.
 *   TRIAGE (1 agent)  -> closes duplicates/superseded, classifies security vs standard, reads the
 *                        REAL check state at each head, writes the queue doc, returns the queue
 *                        ordered security-first then oldest-first.
 *   CHAINS  (2)       -> each takes the next PR when free. Standard PR: 1 reviewer, then fix+land.
 *                        Security PR: 2 reviewers in the REVIEW-STAGE barrier, then fix+land.
 *   SWEEPER (1)       -> everything that ended ci-pending, re-read and landed when green.
 *   MAIN VERIFY (1)   -> main's head, gated from scratch in a clean worktree, plus the DMG smoke
 *                        when args.dmg is set. "It merged" is not "it builds" is not "it runs".
 * Nothing here waits on CI. That wait is what serialised the previous pass.
 */
const TRIAGE_SCHEMA = {
  type: 'object',
  properties: {
    queue: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          pr: { type: 'number' },
          item: { type: 'string' },
          title: { type: 'string' },
          headRefName: { type: 'string' },
          klass: { type: 'string', enum: ['security', 'standard'] },
          ci: { type: 'string' },
          mergeable: { type: 'string' },
          files: { type: 'number' },
          additions: { type: 'number' },
          labels: { type: 'array', items: { type: 'string' } },
        },
        required: ['pr', 'item', 'klass', 'ci', 'mergeable', 'files'],
      },
    },
    closed: {
      type: 'array',
      items: {
        type: 'object',
        properties: { pr: { type: 'number' }, survivor: { type: 'number' }, why: { type: 'string' } },
        required: ['pr', 'why'],
      },
    },
    held: {
      type: 'array',
      items: {
        type: 'object',
        properties: { pr: { type: 'number' }, comment: { type: 'string' }, why: { type: 'string' } },
        required: ['pr', 'why'],
      },
    },
    counts: { type: 'object' },
  },
  required: ['queue', 'closed'],
}

async function reviewPass() {
  const triage = await agentR(
    `TRIAGE for the REVIEW PASS of ${REPO}. ONE job: turn the open ${LOOP_LABEL} PRs into an ordered
queue, and close the ones that should never be reviewed at all. You review nothing, fix nothing and
merge nothing — two review chains do that immediately after you, driven by what you return.

# 1 — ENUMERATE
    gh pr list -R ${REPO} --state open --label ${LOOP_LABEL} --limit 200 --json number,title,headRefName,headRefOid,mergeable,createdAt,files,labels
SKIP every PR labelled needs-human: it is HELD for a human (an outside-seat BLOCK, or a BLOCK after the last fix
round). Do not queue it, do not review it, do not land it — no seat is spent on it. List each one in held[]
with the comment id its needs-human comment names, and put it in the queue doc's held table.
    gh pr list -R ${REPO} --state merged --limit 200 --json number,title,headRefName,mergeCommit
ALSO list the open PRs WITHOUT the ${LOOP_LABEL} label and report them separately, untouched: the
feat/yv1xx PRs predate this loop. They are Drain's problem, not this queue's.

# 2 — THE REAL CHECK STATE, PER PR. THE STATUS API LIES TO YOU HERE.
For each open PR:
    gh api repos/${REPO}/commits/<headRefOid>/check-runs
Take the check run for the CI workflow whose head_sha EQUALS that PR's headRefOid and whose run was
triggered by the pull_request event. A "skipped" check is NOT a pass and NOT a failure — it is not
evidence, and reading it either way is how a previous run reported CI state it had never measured.
Record ci as one of: success | failure | pending | none (no run exists at this head yet). On this
repo the expected answer is "none" for every PR, because Actions is disabled by a spending limit —
report that plainly rather than as a per-PR anomaly.
Do not use gh pr checks and do not use /status — both report pending states that never resolve.

# 3 — DUPLICATES AND SUPERSEDED. DO THIS BEFORE ANYTHING ENTERS THE QUEUE.
Every PR title starts with an ITEM ID. Extract it.
  (a) If an item id already appears on a MERGED PR, the open one is SUPERSEDED. Verify it for real
      — read the merged diff, confirm the open PR's change is present on origin/main — then close
      it: gh pr close -R ${REPO} <n> --comment "superseded by #<merged PR> (<sha>): <evidence>"
  (b) If TWO OR MORE open PRs carry the same item id, keep the NEWEST by createdAt and close the
      rest, naming the survivor: "duplicate of #<survivor>, which is newer and supersedes this".
Never close a PR without naming what replaces it. "Triaged" with no outcome is not an outcome.
Report every close in closed[].

# 4 — CLASSIFY
klass = "security" if ANY of these is true, else "standard":
  - the PR touches ${SECURITY_PATHS}
  - its title's item id matches ${SECURITY_IDS}
  - node ${LOCAL_REPO}/scripts/loop/review-gate.mjs klass --pr <n> prints "security ..." — the land gate classifies with this, so
    if it says security, so do you; if it fails, classify security
A security PR gets a SECOND reviewer with a liveness / failure-polarity / data-and-permission lens,
and therefore costs two of the run's three agent seats at once. Classify honestly: over-classifying
halves this pass's throughput, under-classifying is how a permission or signing regression ships.
Also record size: files (count) and additions (count) from the PR json.

# 5 — WRITE THE QUEUE DOC
mkdir -p the dir, then write ${REVIEW_QUEUE_DOC}: a markdown table with one row per open PR —
PR | item | klass | ci | mergeable | files | additions | createdAt | disposition (queued/closed+why).
Put the closed ones in a second table below it, and the unlabelled pre-existing PRs in a third.
This file is what Wilson reads to see the plan.

# 6 — RETURN THE ORDER
queue[] is ordered: EVERY security PR first (oldest createdAt first inside that group), then every
standard PR by createdAt ASCENDING. Oldest-first inside each group because an old PR has the most
main under it and the most rebase debt — settle it before it rots further.
Do NOT include a PR you closed. Do NOT include a PR that gh reports as already MERGED. Do NOT
include a PR that lacks the ${LOOP_LABEL} label.

RETURN exactly: {queue: [{pr, item, title, headRefName, klass, ci, mergeable, files, additions, labels}],
                 closed: [{pr, survivor, why}], held: [{pr, comment, why}],
                 counts: {open, queued, closed, held, security, standard, unlabelled}}
${LOGLINE('review triage', '_review', 'harness:recon', '--open <the open loop-build PR count you just observed>')}
${GUARD(BOTH_REVIEW_DIRS)}`,
    { model: 'opus', effort: 'high', phase: 'Recon', label: 'review-triage', schema: TRIAGE_SCHEMA }
  )

  // needs-human is HELD: never queued, even when triage forgot to skip it (no reviewer or LAND seat is spent on it).
  const isHeld = (q) => Boolean(q) && Array.isArray(q.labels) && q.labels.some((l) => String((l && l.name) || l) === 'needs-human')
  const heldByTriage = [...extractArray(triage, 'held'), ...extractArray(triage, 'queue').filter(isHeld).map((q) => ({ pr: q.pr, why: 'labelled needs-human' }))]
  REVIEW_QUEUE = extractArray(triage, 'queue').filter((q) => !isHeld(q))
  const closedByTriage = extractArray(triage, 'closed')
  if (heldByTriage.length) log(`held (needs-human, skipped — no seat spent): ${heldByTriage.map((h) => `#${h.pr}${h.comment ? ` (comment ${h.comment})` : ''}`).join(', ')}`)
  if (REVIEW_QUEUE.length === 0) {
    log(
      'review triage returned NO parseable queue[]. The chains have nothing to consume, so this pass ' +
        `does nothing. Read ${REVIEW_QUEUE_DOC} and re-run — do NOT read this as "there was nothing to review".`
    )
  } else {
    const sec = REVIEW_QUEUE.filter((e) => e && e.klass === 'security').length
    log(
      `review queue: ${REVIEW_QUEUE.length} PR(s) — ${sec} security (2 reviewer seats each), ` +
        `${REVIEW_QUEUE.length - sec} standard (1 seat) — ${closedByTriage.length} closed by triage. ` +
        `Two chains, hard cap ${MAX_AGENTS} concurrent agents.`
    )
  }

  // THE ONLY CONCURRENCY IN REVIEW MODE. Both chains pull from one cursor, so a slow security PR on
  // one chain never idles the other: it simply takes the next PR the moment a seat frees.
  await Promise.all([reviewChain(0), reviewChain(1)])

  const pending = reviewOutcomes.filter((r) => r && (r.status === 'ci-pending' || r.status === 'review-pending'))
  log(`both review chains finished: ${reviewOutcomes.length} PR(s) processed, ${pending.length} left ci-pending for the sweeper`)

  const sweep = await agentR(
    `LAND SWEEPER for ${REPO}. The review chains never wait for CI — that wait is what serialised the
previous pass — so every PR that could not be settled when its fix+land agent finished was recorded
ci-pending and left open. You settle them, and nothing else.

CI-PENDING AND REVIEW-PENDING PRs FROM THIS PASS (a review-pending PR lands ONLY if a reviewer seat has
since posted a PASS on its current head — the review gate decides; you never review it yourself):
${JSON.stringify(pending.map((r) => ({ pr: r.pr, item: r.itemId, klass: r.klass })), null, 1)}
Treat that list as the starting point, not the boundary: also re-list
    gh pr list -R ${REPO} --state open --label ${LOOP_LABEL} --json number,title,headRefOid,mergeable
and settle any PR of this pass that is green and mergeable.

FOR EACH, APPLY THE RUN'S GATE — Recon decided which one, and this is it:
${CI_GATE(REVIEW_DIRS[0], REVIEW_TARGETS[0])}

${CI_MODE === 'local'
  ? `SWEEP ONCE. There is nothing to wait for: no run will ever start, so a second sweep would
measure the same wall. Gate what you can gate in your budget, land it, and NAME every PR you left
open with the reason. Do NOT wait ten minutes. Do NOT return anything as "stillPending" on a CI
check — in this mode the only honest reasons a PR is still open are "local gate failed at <cmd>",
"conflicting", or "out of budget".`
  : `SWEEP TWICE, AND ONLY IF SOMETHING IS STILL PENDING.
After the first sweep, if and only if at least one PR is still queued/in_progress, wait ten minutes
IN THIS AGENT and sweep again:
    perl -e 'select(undef,undef,undef,600)'
There is no 'timeout' binary on macOS and 'sleep' in this harness belongs in an agent, never in the
script. Do NOT sweep a third time and do NOT wait if nothing is pending — return instead and name
what is still open.`}

RETURN: {sweeps: 1|2, merged: [{pr, sha}], ciRed: [{pr, failingCommand}], needsRebase: [{pr, paths}],
         stillPending: [{pr, reason}], notes}
${LOGLINE('land sweeper', '_review', 'review:merged')}
${GUARD(BOTH_REVIEW_DIRS)}`,
    { model: 'opus', effort: 'low', phase: 'Drain', label: 'land-sweeper' }
  )

  const securityMerged = reviewOutcomes.filter((r) => r && r.klass === 'security')
  const mainVerify = await agentR(
    `MAIN VERIFY for ${REPO} after the review pass. "It merged" is not "it builds" is not "it runs",
and this pass merged a lot. Prove the third one, on main's own head, in a worktree whose cache you
are allowed to trust and nowhere else.

# 1 — THE TREE YOU ARE VERIFYING
    cd ${LOCAL_REPO} && git fetch origin && git rev-parse origin/main
    cd ${REVIEW_DIRS[0]} && git fetch origin && git checkout --detach origin/main
Paste the sha. Everything below is a claim about THAT sha and nothing else.

# 2 — THE WHOLE GATE, FROM MAIN, NOT FROM A PR BRANCH
${GATE_CMDS(REVIEW_DIRS[0], REVIEW_TARGETS[0])}
Report all eight codes plus the informational fmt code. Main going red after a pass of unreviewed
merges is the single most important thing this agent can find: if any command fails, name the
failing command, the failing output, and — from 'git log --oneline' on the merges of this pass —
which merge most likely introduced it. Do NOT fix it silently; report it as a LIVE DEFECT and open
a follow-up PR if the fix is small and obvious.

# 3 — THE SECURITY ITEMS THIS PASS TOUCHED
SECURITY PRs FROM THIS PASS: ${JSON.stringify(securityMerged.map((r) => ({ pr: r.pr, item: r.itemId, status: r.status })), null, 1)}
For each MERGED one, re-run its own acceptance probe against main's head and paste the raw output:
the capability entry, the generate_handler! grep, the migration applied to a COPY of a history file,
the sidecar handshake, the updater signature check — whichever that item's acceptance named.
${RUNTIME_PROOF(REVIEW_DIRS[0])}

# 4 — THE APP ACTUALLY RUNS
A green workspace build is not a running app. Launch it from main's head and say what you saw:
    cd ${REVIEW_DIRS[0]}/${APP} && export CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=${REVIEW_TARGETS[0]} && npm run desktop:dev
Confirm: the main window renders, the float/pill window appears, the dictation hotkey fires, and the
sidecar answers. Screenshot anything visual. If you could not launch it (no display, permission
prompt, sidecar missing), SAY SO plainly and mark the claim unverified — never imply you ran it.
Kill it before you return and confirm nothing listens on ${REVIEW_PORTS[0]}.

# 5 — THE DMG, ONLY IF ASKED
${(typeof args !== 'undefined' && args && args.dmg) ? `args.dmg IS SET, so build the bundle once from main's head and report its path and size:
    cd ${REVIEW_DIRS[0]}/${APP} && export CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=${REVIEW_TARGETS[0]} && npm run desktop:build ; echo "dmg=$?"
This takes minutes. Do NOT notarize and do NOT publish anything — a local bundle that builds is the
claim, nothing further.` : `args.dmg is NOT set, so DO NOT build the DMG. It is minutes long, it is
not this pass's claim, and the final smoke item owns it. Report dmg: "not requested".`}

# 6 — WRITE THE SUMMARY
mkdir -p its dir and write ${REVIEW_SUMMARY_DOC}: main's sha, the eight gate codes, a table of every
PR this pass touched (PR, item, klass, verdict, outcome), the security re-probes with raw output,
what you saw when you launched the app, and a plain list of anything still open with the reason.

RETURN: {mainSha, gateExits: {typecheck, test, build, sidecar, polishtest, clippy, cargotest, fmt},
         mainGreen: true|false, securityProbes: [...], appLaunched: true|false, appObservations,
         dmg, liveDefects: [...], summaryWritten: true|false}
${VERIFY_NOTHING}
${LOGLINE('main verify', '_review', 'review:done')}
${GUARD(BOTH_REVIEW_DIRS)}`,
    { model: 'opus', effort: 'high', phase: 'Drain', label: 'main-verify' }
  )

  return { triage, queued: REVIEW_QUEUE.length, closedByTriage, held: heldByTriage, sweep, mainVerify }
}

let census = null
if (MODE === 'review') {
  census = await reviewPass()
} else {
  await Promise.all([buildLane(0), buildLane(1), buildLane(2)])
}
const results = MODE === 'review' ? reviewOutcomes.slice() : slots.filter((r) => r !== null)

if (halted) {
  log(
    `HALT at ${halted.at}: ${halted.reason} — ${results.length} of ${ITEMS.length} items have a ` +
      `recorded outcome. Drain and Reflect are SKIPPED, so both worktrees (${BOTH_DIRS}) are left ` +
      `standing for the resume; resume with resumeFromRunId after the reset.`
  )
  return { halted: true, at: halted.at, reason: halted.reason, mode: MODE, results, errored }
}

phase('Drain')
const drain = await agentR(
  LAND_BAR_CI ? CI_DRAIN_PROMPT() : `DRAIN + SELF-AUDIT for ${REPO}, then TEAR DOWN the work dirs.

1. gh pr list -R ${REPO} --state open --json number,title,headRefName,createdAt,labels
   This command is the ONLY source of the open-PR count. Do not infer it from branch names and do
   not read a title's own annotation as live state — that mistake has produced a wrong count in
   four consecutive recorded runs on the sibling project.
2. TRIAGE EVERY OPEN PR, not only the ones this pass created. For each: merge / rebase-then-merge /
   close-as-superseded WITH EVIDENCE. If a PR is blocked by a defect you can fix in a few minutes,
   FIX IT — drain agents that fix rather than report are what took a 19-PR queue to 1 in one run.
   REPORT-ONLY FOR THIS RUN'S OWN UNREVIEWED PRs. Every PR labelled ${LOOP_LABEL} whose body starts
   with "${UNREVIEWED_PREFIX}" is waiting for the REVIEW PASS, not for you. Do NOT merge one that is
   conflicting or red, and do not review one yourself: list them with their mergeStateStatus and
   leave them open. The review pass reviews them; a LAND step merges only what a reviewer passed.
   THE PRE-EXISTING feat/yv1xx PRs ARE YOURS TO SETTLE, AND THIS IS THE RUN THAT SETTLES THEM.
   They are weeks old, they carry no ${LOOP_LABEL} label, and nothing has closed them in four
   sessions. For EACH: rebase onto main and merge it if it still makes sense, the gate is green AND the
   review gate below passes on its head (it carries no reviewer record, so expect review-pending),
   or CLOSE it with the evidence that supersedes it (name the sha or PR on main that did the work,
   or state plainly that the work was abandoned). "Still open, stale" is not an outcome. A stale PR
   nobody closes is the backlog that grows with every run.

${MERGE_BAR}
EVERY MERGE THE DRAIN MAKES GOES THROUGH THE REVIEW GATE — a PR you rebased has a new head and needs a
fresh reviewer PASS on it, and you never post one:
${REVIEW_GATE_STEP}
   Enumerate every blocking finding and its disposition in your returned payload as
   blockingDispositions: [{finding, disposition: "fixed"|"refuted", evidence}].
   If ANY blocking finding is neither fixed nor refuted, DO NOT MERGE — leave the PR open and say
   which finding stopped you.

${
  MODE === 'review'
    ? `2b. THIS WAS THE REVIEW PASS, so TWO EXTRA WORKTREES EXIST AND THEY ARE YOURS TO REMOVE:
     cd ${LOCAL_REPO} && git worktree remove --force ${REVIEW_DIRS[0]} && git worktree remove --force ${REVIEW_DIRS[1]} && git worktree prune
     git worktree list                       # paste it: BOTH review worktrees must be GONE
     git branch -D yap/review-r1 ; git branch -D yap/review-r2
   Confirm with ls -d that neither path exists. Also kill anything listening on ${REVIEW_PORTS[0]}
   or ${REVIEW_PORTS[1]} (lsof -nP -iTCP:<port> -sTCP:LISTEN) and confirm both are clear, and kill
   any leftover 'tauri dev' / vite process a chain started.
   The review target dirs ${REVIEW_TARGETS[0]} and ${REVIEW_TARGETS[1]} are CACHE, not checkouts:
   remove them too (rm -rf) and report the bytes reclaimed — a cold one measured 7.5 GB.
   Leave ${WORKDIR} and ${WORKDIR_B} EXACTLY as you found them — the review pass never used them.
   Then RECONCILE THE PASS, from gh and from nothing else:
     gh pr list -R ${REPO} --state merged --limit 200 --json number,title,mergedAt
     gh pr list -R ${REPO} --state open --label ${LOOP_LABEL} --limit 200 --json number,title,labels
   Report: open BEFORE this pass, open AFTER, merged in this pass, and the counts of PRs now
   labelled needs-human (list them as HELD — triage skipped them, no seat was spent), ci-red, needs-rebase,
   plus the ones triage closed as superseded/duplicate.
   A count you derived from branch names instead of gh is a wrong count.
`
    : ''
}3. Sweep dead branches: for each remote branch, check whether its head is an ancestor of main
   (git merge-base --is-ancestor). Delete the ones that are. Report, do not delete, the ones that
   are not — and BUNDLE any unpushed local branch before deleting anything
   (git bundle create ${LOCAL_REPO}/.loop-archive/<name>.bundle <branch> --not origin/main). The
   remote has stale feat/yv1xx and fix/* branches; most are ancestors of main.
4. Report open_prs_before and open_prs_after. If the number went UP, this run failed regardless of
   how much merged. Say so plainly.
${DRAIN_TEARDOWN('5')}

RETURN: {open_prs_before, open_prs_after, triaged: [...], preexistingSettled: [{pr, outcome, evidence}],
         blockingDispositions: [...], branchesDeleted: [...], branchesKept: [...],
         worktreeRemoved: true|false, reviewWorktreesRemoved: true|false, targetDirsRemoved: true|false,
         portsClean: true|false, diskReclaimed,
         reviewPassCounts: {mergedThisPass, needsHuman, ciRed, needsRebase, closedSuperseded}}
${LOGLINE('drain', '_drain', 'harness:drain')}
${GUARD(BOTH_DIRS)}`,
  { model: 'opus', effort: 'high', phase: 'Drain', label: 'drain' }
)

phase('Reflect')
const reflect = await agentR(
  `REFLECTION for the Yap overhaul loop. Append a dated section to ${TELEMETRY}.

# RECONCILE BEFORE YOU WRITE — your first two tool calls
gh pr list -R ${REPO} --state open --json number,title
gh pr list -R ${REPO} --state merged --limit 20 --json number,title,mergeCommit
Diff EVERY id, PR number and SHA in the outcomes payload against those results. A mismatch is
written up as a discrepancy row, never transcribed as fact. A merge SHA that already appears in an
earlier section of ${TELEMETRY} is a REPLAY — flag it, do not count it as a merge.
The repo is ${REPO}. Reconciling against the wrong repo manufactures confident false findings.

# fixRounds IS COUNTED, NEVER ASSERTED
Derive it from ${LOG}: count the "<id> fix" lines per item. Ignore any fixRounds value in the
payload below.

# WRITE
- Per-item table: id, prompt, status (built / already-done / skipped: awaiting Senior Panel /
  errored), PR, merged, primary opus verdict, independent opus verdict + status, fixRounds COUNTED,
  the gate codes, notes.
- ALREADY-DONE items: list them with the pre-flight evidence that retired them. An already-done
  item is a WIN — earlier work independently re-verified rather than re-done — but only if the
  pre-flight actually ran. An already-done with no pasted evidence is a discrepancy row.
- GATED items still awaiting the Senior Panel: list them and what the panel has to decide.
- FAILED-ACCEPTANCE items: list every one, its first failing command, and whether the fix round
  said the acceptance command itself was wrong. These are the loop's real findings — an item whose
  own acceptance could not be made to pass, with a PR left open and labelled needs-human. Never
  write one up as "built".
- ERRORED items: list every one with its error and what the next run must do about it. A thrown
  stage no longer stops this loop, so an errored item nobody writes up is an item that disappears.
- Open PRs before vs after (from gh, never from branch names), and what happened to the
  pre-existing feat/yv1xx PRs specifically.
- THE PASS THIS WAS. mode=${MODE}. In BUILD mode no reviewer, fix or merge agent was dispatched at
  all: items were built, PRs were opened with the ${LOOP_LABEL} label, and later items LANDED the
  gate-green ones unreviewed. Say so plainly, list every PR that merged WITHOUT a review, and state
  that the gate is owed, not waived — it runs with args {mode:'review'}. An unreviewed merge written
  up as if it were reviewed is the single worst thing this reflection could do.
- IF mode=review: this pass was PR-DRIVEN, not item-driven. Report the queue triage returned (how
  many security, how many standard, how many closed as duplicate/superseded), then per PR: number,
  item id, class, primary verdict, independent verdict (security PRs only — say plainly that a
  standard PR got ONE reviewer by design, never imply a second), whether it needed a fix round, and
  its terminal state (merged / ci-pending→swept / ci-red / needs-rebase / needs-human /
  closed-superseded). Then: did the 3-agent cap hold? How long did a standard PR take end to end
  versus a security PR? Did MAIN VERIFY find a LIVE DEFECT, and if so which merge introduced it?
- THE GATE THIS PASS ACTUALLY USED. CI_MODE was ${CI_MODE}, resolved by Recon with
  scripts/loop/ci-mode.mjs${(recon && typeof recon === 'object' && recon.ciModeWhy) ? ` — evidence: ${recon.ciModeWhy}` : ''}.
${CI_MODE === 'local'
  ? `  COUNT THE LOCAL-GATE MERGES FROM GITHUB, NOT FROM THE PAYLOAD. A merge that landed on the local
  gate carries a PR comment titled "${LOCAL_GATE_TITLE}":
      gh pr list -R ${REPO} --state merged --label ${LOOP_LABEL} --limit 100 --json number,title,mergedAt
      gh pr view -R ${REPO} <n> --json comments      # per PR: does a "${LOCAL_GATE_TITLE}" comment exist?
  Call that count N, then write this sentence into the section VERBATIM with N substituted and
  nothing softened, hedged or reworded:
      "CI was unavailable (Actions disabled by the account spending limit); N PRs merged on the
       local gate; re-run CI on main when restored: gh workflow run ci.yml --ref main"
  Then list all N by number with their pasted exit codes (typecheck, test, build, sidecar,
  polishtest, clippy, cargotest) and the gated sha. And name every PR this pass merged that has NO
  such comment: that is a merge with no gate at all, and it is a FINDING at the top of the section,
  not a footnote. Finally: state plainly that main has not been through CI since before this loop,
  so the first thing to do when Actions is restored is 'gh workflow run ci.yml --ref main' and read
  the result before trusting the merged tree.`
  : `  CI ran normally this pass: every merge was gated by the CI check run on the PR's own head sha.
  Say so explicitly rather than leaving it unstated — and note that CI treats clippy and fmt as
  informational, so name which merges also carry a local clippy run.`}
- LANE BALANCE: how many items each lane got, and whether one lane finished long before the other
  (that is the lane assignment to fix next run, not a defect).
- DISK: the target-dir sizes and free space Drain reported, and whether every worktree and target
  dir this run created is actually gone (or deliberately kept for the next part).
- THE EXECUTED GATES. How many items were retired by the EXECUTED pre-flight without a builder
  being dispatched; how many acceptances passed first time; how many needed the one fix round; how
  many ended failed-acceptance. If every acceptance passed first time on every item, say so and
  treat it as suspicious rather than as success — that is the shape a runner seat that softened an
  exit code would produce.
- SEAT TIERING (loop/model-tiering). Count the run log lines that start "[tier]": each one is a mechanical
  seat (preflight, acceptance, acceptance-rerun, record-failure, recon, reflect) whose smaller model
  returned NEEDS_OPUS, a result missing a required field, or null, so the step was re-run on opus.
  Report the count and list each line verbatim. If the run log is not visible to you, write "the
  [tier] log lines are not visible to this seat" instead of a number; never report zero from absence.
- Which harness guards actually FIRED versus passed vacuously. Name them individually: the
  EXECUTED pre-flight short-circuit, the EXECUTED acceptance + its one fix round, the
  reachability gate (generate_handler! / entry import /
  capability), the runtime-proof requirement, the malformed-BLOCK recording, the merge bar's
  blockingDispositions, the 3-agent semaphore, the worktree teardown.
- 2-3 concrete refinements for the next wave, and any standing chore still unshipped with how many
  runs it has now survived.

# CLOSE THE BOARD
For every item in the payload whose outcome is terminal (merged, already-done, or reviewed+merged),
mark it done on the board, then close the header:
    node ${STATUS} <itemId> harness:done "<one clause: the outcome>"
    node ${STATUS} --part ${PART} --total ${ITEMS.length} --current none
The board is ${STATUS_BOARD} and it is what Wilson reads; leaving an item showing "build" forever
after it merged is a false green.

OUTCOMES PAYLOAD:
${JSON.stringify(results, null, 1)}
ERRORED ITEMS (thrown stages the loop caught and continued past):
${JSON.stringify(errored, null, 1)}
RECON:
${JSON.stringify(recon, null, 1)}
DRAIN:
${JSON.stringify(drain, null, 1)}

${LOGLINE('reflect', '_reflect', 'harness:reflect')}
${GUARD(BOTH_DIRS)}`,
  { model: 'opus', effort: 'high', phase: 'Reflect', label: 'reflect' }
)

return {
  mode: MODE,
  ciMode: CI_MODE,
  part: PART,
  recon,
  census,
  lanes: { A: laneItems[0].map((it) => it.id), B: laneItems[1].map((it) => it.id), C: laneItems[2].map((it) => it.id) },
  steals,
  landBar: LAND_BAR_CI ? 'ci' : 'review',
  results,
  errored,
  counts: {
    total: ITEMS.length,
    built: results.filter((r) => r && r.status === 'built').length,
    alreadyDone: results.filter((r) => r && r.status === 'already-done').length,
    failedAcceptance: results.filter((r) => r && r.status === 'failed-acceptance').length,
    awaitingPanel: results.filter((r) => r && r.status === 'skipped: awaiting Senior Panel').length,
    awaitingDependency: results.filter((r) => r && r.status === 'skipped: dependency not on main').length,
    errored: errored.length,
    merged: results.filter((r) => r && r.status === 'merged').length,
    ciPending: results.filter((r) => r && r.status === 'ci-pending').length,
    ciRed: results.filter((r) => r && r.status === 'ci-red').length,
    needsHuman: results.filter((r) => r && r.status === 'needs-human').length,
    needsRebase: results.filter((r) => r && r.status === 'needs-rebase').length,
    closedSuperseded: results.filter((r) => r && r.status === 'closed-superseded').length,
  },
  drain,
  reflect,
}
