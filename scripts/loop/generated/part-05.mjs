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
  name: "yap-overhaul-all-part-05",
  description:
    "Part 05 of the Yap (wilson-voice) overhaul, two or three builder lanes (args.lanes): land whatever is already gate-green, pre-flight on unmodified main (already-done items are skipped), build, open a labelled PR. The adversarial review, the independent second-Opus gate and the merge bar run afterwards in the same script with args {mode:'review'}. 25 items (Y9-A..yap-fmt-sweep-2026-10-26), 5 awaiting the Senior Panel. The gate is local (Actions is disabled by the account spending limit); the DMG is not in it.",
  phases: [
    { title: "Recon", detail: "two lane worktrees, one npm ci + one warm cargo build each, the loop-build label, ci-mode measured" },
    { title: "Y9-A", detail: "A named transform library over the existing sidecar, with an observable status enum" },
    { title: "Y9-B", detail: "Writing samples become a local style profile injected into the polish prompt" },
    { title: "Y9-C", detail: "Spoken preference rules: say a rule once, it applies where it matches — with an explicit Apply step" },
    { title: "PERM-F", detail: "Deeper accessibility context: the selection and the text after the caret, read in-process" },
    { title: "PERM-G", detail: "IDE context: the identifiers in the open file bias the transcription — the highest personal-ROI item" },
    { title: "Y9-D", detail: "A denylist where the hotkey is inert — the honest complement to reading your context" },
    { title: "Y9-E", detail: "CSV round-trip for the dictionary and snippets, and the usage-frequency ranking that is only half there" },
    { title: "Y10-A", detail: "Multi-language: expose what the engine can already do, with the picker in the bar" },
    { title: "PERM-H", detail: "A ranked microphone preference list, forget-device, and the AirPods and clamshell warnings" },
    { title: "Y10-B", detail: "Rich-text snippets: RTF and HTML flavours on the pasteboard without racing the receipt-sequenced paste" },
    { title: "Y10-D", detail: "A non-primary mouse button as push-to-talk" },
    { title: "Y10-E", detail: "Insights v2: the numbers Wispr computes in the cloud, computed in SQLite, feeding Yappy's dialogue" },
    { title: "Y10-F", detail: "Measure and publish Yap's idle RAM and CPU — the free marketing line the research asked for" },
    { title: "Y11-A", detail: "Rebase the six parked branches onto main so each is evaluated against the shipped min_embed, not against what main was" },
    { title: "Y11-B", detail: "Issue #150: the impossibility framing survives in transcript.ts where the guard cannot see it" },
    { title: "DB-E", detail: "Issue #151: speaker_profiles stores a catalog id where it must store the pinned weights digest" },
    { title: "Y11-C", detail: "Issue #152: bands tuned on utterance pairs, applied to roster-max centroid scoring — FAR 1.000 on the shipped path" },
    { title: "Y11-D", detail: "Issue #153: split_partition's farthest-pair seeding does not separate speakers when an outlier is the far point" },
    { title: "Y11-E", detail: "Issues #154 and #155: a false mechanism claim in a shipped asset, and a comment naming call sites that do not exist" },
    { title: "Y11-F", detail: "A real-voice eval corpus to replace the synthetic one — the numbers are only floors until it exists" },
    { title: "yap24-PKG1", detail: "One local release command, by EXTENDING scripts/sign-local.sh: --dry-run, --check-only, DMG, notarize, staple, verify and the updater manifest through the existing stage-updater-manifest.sh" },
    { title: "yap24-PKG2", detail: "A first launch on ANY network gets an honest wait: one download with size, progress and an ETA, a plain offline sentence, and the first dictation the moment the model lands. No bundled second model." },
    { title: "yap24-X1", detail: "Your notes outlive the app: every meeting and (optionally) every dictation mirrors to Markdown in a folder you choose" },
    { title: "yap24-X3", detail: "Ask your meeting: \"what did I miss / what did we decide\" answered by the local model, every answer citing its transcript lines" },
    { title: "yap-fmt-sweep-2026-10-26", detail: "Format-only sweep: `cargo fmt --all` on main, zero semantic change; then rustfmt goes back to BLOCKING in the land gate, loop-smoke and CI" },
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
const LOG = '/Users/wilsonguenther/Obsidian/Wilson-Brain/Projects/Loop-Logs/2026-09-12-yap-part05.md'
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
const KEEP_WORKTREE = false
/** Stamped by build.mjs like LOG: which part this is. The status board is keyed on it. */
const PART = 'part-05'
/**
 * Stamped by build.mjs: item id -> the item FILE's ordinal (informational) or a pinned lane letter
 * ('A'|'B'|'C', item field `lane`). Only the PIN is read: since 2026-10-27 lanes are assigned PER PART
 * at run time (splitLanes below) — the old per-file round-robin left lane C idle for a whole part.
 */
const LANE_BY_ID = {"Y9-A":23,"Y9-B":23,"Y9-C":23,"PERM-F":23,"PERM-G":23,"Y9-D":23,"Y9-E":23,"Y10-A":24,"PERM-H":24,"Y10-B":24,"Y10-D":24,"Y10-E":24,"Y10-F":24,"Y11-A":25,"Y11-B":25,"DB-E":25,"Y11-C":25,"Y11-D":25,"Y11-E":25,"Y11-F":25,"yap24-PKG1":26,"yap24-PKG2":26,"yap24-X1":27,"yap24-X3":27,"yap-fmt-sweep-2026-10-26":28}
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
// ── 45-y9-parity-p1-intelligence.mjs ──────────────────────────────────────
// Y9 — THE P1 PARITY QUEUE: "closes the 'intelligence' gap without leaving the
// machine." Source of record:
// ~/Obsidian/Wilson-Brain/Notes/Wispr-Full-Parity-Research-2026-08-09.md §3 P1
// items 7-14, quoted verbatim per item below. Every ❌/🟡 score cited there was
// re-verified against origin/main @ 4e8c9adf by grep, and the greps are named.
//
// These are a separate file from Y8 because they touch the polish sidecar, the
// AX layer and the dictionary — not the pill — so the two groups can run on
// opposite lanes without conflicting.
//
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.
// Depends on Y4 (the formatting stage must actually run before any of this is
// observable) — sequence this file after 20-y4.

ITEMS.push({
  id: 'Y9-A', prompt: 'Y9', branch: 'loop/y9-a-named-transform-library', gated: null,
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
  title: 'A named transform library over the existing sidecar, with an observable status enum',
  preflight: `
    test -f desktop/src-tauri/src/transforms.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test transforms
  `,
  spec: `
    P1 #7, VERBATIM: "Transform library. (M · L) Named prompts (built-ins:
    *Polish*, *Prompt Engineer*, *Concise*, *Formal*), each with an optional
    shortcut, run through the existing \`yap-polish\` sidecar. Persist the
    Wispr-shaped status enum (\`succeeded | timeout | error | no_changes |
    not_editable | …\`) so failures are observable rather than silent. Accept
    (L): golden fixtures per transform; a forced sidecar timeout yields
    \`status=timeout\` **and the original text unchanged** (never-lose-text)."

    VERIFIED ABSENT: \`git grep "transform_library\\|named_transform"
    -- desktop\` -> 0. The teardown scores "LLM transform over selection" 🟡
    "sidecar exists (YV60/61), not exposed as a transform library" and "Named
    transforms w/ per-transform hotkeys" ❌.

    What DOES exist and must be reused: YV49's nine pure deterministic Rust
    transforms over a selection (\`command_mode.rs\`, 625 lines — the teardown
    scores those ✅), and the whole validated polish stage
    (\`polish.rs\`: \`PolishClient\` seam, \`validate_polish\` V1-V7,
    \`SidecarPool\`, the 1200 ms deadline).

    Do:
      * \`desktop/src-tauri/src/transforms.rs\`: a TABLE of named transforms
        (id, label, prompt, whether it is deterministic or model-backed),
        so a new transform is a row and not a new code path. The four built-ins
        above, plus the nine existing deterministic ones registered in the same
        table so there is ONE list the UI reads.
      * Model-backed transforms go through \`polish::polish_stage\` — the SAME
        validator, the SAME deadline, the SAME never-lose-text rule. A transform
        must not be a second, unvalidated path to the pasteboard.
      * The status enum, persisted per invocation:
        \`succeeded | no_changes | timeout | rejected | error | not_editable\`.
        \`rejected\` is Yap-specific and important: it is what
        \`validate_polish\` returning None means, and conflating it with \`error\`
        would hide the validator's work.
      * Per-transform shortcuts are registered through Y8-A's validated table.
      * Golden fixtures per transform under
        \`tests/fixtures/transforms/*.jsonl\`, following the conventions in
        tests/fixtures/README.md.

    Tests \`tests/transforms.rs\`, using polish.rs's injectable clients:
      * a golden fixture per transform
      * a forced timeout yields \`timeout\` AND byte-identical input
      * a garbage-returning client yields \`rejected\` AND byte-identical input
      * a panicking client yields \`error\` AND byte-identical input
      * \`no_changes\` when the model returns the input
      * every table row has a label and a non-empty prompt

    Depends on SEC-C (a model to run), Y8-A (the shortcut table).

    What NOT to do:
      - Do NOT add a second path to the pasteboard that skips \`validate_polish\`.
      - Do NOT let a transform silently fail. The enum exists so it cannot.
  `,
  acceptance: `
    test -f desktop/src-tauri/src/transforms.rs
    test -d desktop/src-tauri/tests/fixtures/transforms
    test -f desktop/src-tauri/tests/transforms.rs
    grep -q 'not_editable' desktop/src-tauri/src/transforms.rs
    grep -q 'rejected' desktop/src-tauri/src/transforms.rs
    grep -q 'a_forced_timeout_leaves_the_input_byte_identical' desktop/src-tauri/tests/transforms.rs
    grep -q 'a_garbage_client_yields_rejected_not_error' desktop/src-tauri/tests/transforms.rs
    grep -q 'every_table_row_has_a_label_and_a_prompt' desktop/src-tauri/tests/transforms.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test transforms ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y9-B', prompt: 'Y9', branch: 'loop/y9-b-writing-samples-local-style-profile', gated: null,
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
  title: 'Writing samples become a local style profile injected into the polish prompt',
  preflight: `
    grep -q 'writing_samples' desktop/src-tauri/src/db.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test style_profile
  `,
  spec: `
    P1 #9, VERBATIM: "Writing samples → local style profile. (M · L) A
    \`user_context(writing_samples, custom_rules)\` table, samples pasted by the
    user, injected as few-shot context into the polish prompt. **Fully local —
    Wispr sends yours to Baseten.** Accept (L): the built prompt contains the
    samples; with zero samples the prompt is byte-identical to today's (no
    regression on the fixture corpus)."

    VERIFIED ABSENT: \`git grep "writing_samples" -- desktop\` -> 0. The
    teardown's §2.5 row: Wispr's \`UserContext.writingSamples\` with "max N, min
    word count, 'paste an email you've written'" feeding their Polish prompt;
    Yap ❌.

    Do:
      * A \`user_context\` table (migration, idempotent) with writing samples and
        free-text custom rules. Bounded: a max sample count and a max total
        length, both named constants, because the prompt has a token budget
        (\`polish_protocol.rs\`'s \`max_out_for\` already reasons about budgets —
        read it and respect the same accounting).
      * A settings surface to paste samples in, with the min-word-count guard
        Wispr uses (a two-word "sample" is noise in a few-shot prompt).
      * Injected into \`polish::build_request\` as few-shot context. THE
        REGRESSION GUARD IS THE ACCEPTANCE: with zero samples the built request
        must be byte-identical to today's, so the whole existing formatting
        corpus is untouched. Assert that directly against a serialized request.
      * Samples are user text: they must be excluded from the support bundle and
        from every log, like transcripts already are
        (tests/support_bundle_redaction.rs). Extend that test.
      * They never leave the machine — PRIV-A's outbound sweep must still pass,
        and the samples table must be named in PRIVACY.md.

    Tests \`tests/style_profile.rs\`: zero samples -> byte-identical request;
    one sample -> it appears in the request; over-budget samples are truncated
    deterministically (oldest dropped, said in the doc comment); samples never
    appear in a support bundle.

    Depends on SEC-C, PRIV-A.

    What NOT to do:
      - Do NOT send a sample anywhere.
      - Do NOT let samples grow the prompt past the budget — a silently
        truncated prompt is a silently different formatter.
      - Do NOT include samples in a crash report.
  `,
  acceptance: `
    grep -q 'writing_samples' desktop/src-tauri/src/db.rs
    grep -q 'user_context' desktop/src-tauri/src/db.rs
    test -f desktop/src-tauri/tests/style_profile.rs
    grep -q 'zero_samples_yields_a_byte_identical_request' desktop/src-tauri/tests/style_profile.rs
    grep -q 'samples_never_appear_in_a_support_bundle' desktop/src-tauri/tests/style_profile.rs
    grep -q 'writing samples' PRIVACY.md
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test style_profile           ; test $? -eq 0
    cargo test --features custom-protocol --test formatting_fixtures     ; test $? -eq 0
    cargo test --features custom-protocol --test support_bundle_redaction ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y9-C', prompt: 'Y9', branch: 'loop/y9-c-spoken-preference-rules', gated: null,
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
  title: 'Spoken preference rules: say a rule once, it applies where it matches — with an explicit Apply step',
  preflight: `
    grep -q 'voice_preferences' desktop/src-tauri/src/db.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test voice_preferences
  `,
  spec: `
    P1 #10, VERBATIM: "Spoken preference rules (the \`UserVoicePreferences\`
    idea). (M · L) Let the user say 'never use exclamation marks in email' and
    store \`(preference, filter)\` where filter is \`app:*\` / \`mode:email\` /
    \`lang:*\`; apply matching rules to the polish prompt. Requires an explicit
    **Apply** step (Wispr does this too) so nothing changes behaviour silently.
    Accept (L): rule with \`mode:email\` alters the email fixture output and
    leaves the chat fixture unchanged."

    The teardown calls this "the single most interesting mechanic found" and
    quotes the competitor's own migration doc comment as the source
    (§2.5, [BUNDLE] \`20260528120000-create-user-voice-preferences-table.js\`):
    append-only spoken rules — "never use 'awesome' in German", "sound formal
    with John in Gmail" — turned into structured filter strings like
    \`app:gmail\`, \`language:de\`. And: "fully implementable locally"
    (memory index line on this reference).

    VERIFIED ABSENT: \`git grep "user_voice_pref" -- desktop\` -> 0.

    Do:
      * A \`voice_preferences\` table, APPEND-ONLY: the raw spoken sentence, the
        derived filter, the derived rule text, a timestamp, and an active flag.
        Append-only matters — it is what lets a user see what they said and
        retract it, rather than trusting an opaque profile.
      * Derivation is LOCAL: the spoken sentence goes through the polish sidecar
        with a dedicated prompt that emits a structured
        \`{filter, rule}\`, validated against an allowlist of filter shapes
        (\`app:<bundle-id>\` / \`mode:<one of the six>\` / \`lang:<code>\` / \`*\`).
        An unparseable rule is REJECTED and shown back to the user, never
        stored half-derived.
      * AN EXPLICIT APPLY STEP. The derived rule is shown — "I heard: never use
        exclamation marks. I'll apply it to: email" — and only takes effect when
        the user confirms. The teardown is explicit that this is required so
        nothing changes behaviour silently, and a dictation app that silently
        rewrites your voice on a misheard sentence is the worst version of this
        feature.
      * Matching rules are appended to the polish prompt for takes whose
        mode/app/language match. Bounded, sharing Y9-B's token budget.

    Tests \`tests/voice_preferences.rs\`:
      * the acceptance verbatim: a \`mode:email\` rule alters the email fixture
        output and leaves the chat fixture unchanged
      * an unparseable rule is rejected and not stored
      * a filter outside the allowlist is rejected
      * no rule takes effect before Apply
      * retracting a rule stops its effect and leaves the append-only history
      * zero active rules -> byte-identical request (the Y9-B guard again)

    Depends on Y9-B (the budget accounting and the request-identity test).

    What NOT to do:
      - Do NOT apply a rule before the user confirms it.
      - Do NOT delete rows on retract. Append-only means append-only.
      - Do NOT let a rule reach the prompt without passing the filter allowlist.
  `,
  acceptance: `
    grep -q 'voice_preferences' desktop/src-tauri/src/db.rs
    test -f desktop/src-tauri/tests/voice_preferences.rs
    grep -q 'a_mode_email_rule_alters_email_and_leaves_chat_unchanged' desktop/src-tauri/tests/voice_preferences.rs
    grep -q 'no_rule_takes_effect_before_apply' desktop/src-tauri/tests/voice_preferences.rs
    grep -q 'a_filter_outside_the_allowlist_is_rejected' desktop/src-tauri/tests/voice_preferences.rs
    grep -q 'retract_leaves_the_append_only_history' desktop/src-tauri/tests/voice_preferences.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test voice_preferences   ; test $? -eq 0
    cargo test --features custom-protocol --test formatting_fixtures ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'PERM-F', prompt: 'Y9', branch: 'loop/perm-f-deeper-ax-context-selection-and-after-caret', gated: null,
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
  title: 'Deeper accessibility context: the selection and the text after the caret, read in-process',
  preflight: `
    grep -q 'AXSelectedText' desktop/src-tauri/src/focus.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test ax_context
  `,
  spec: `
    P1 #11, VERBATIM: "Deeper AX context: selected + after-caret + app URL.
    (M · L) Yap reads before-caret today (YV50). Add \`AXSelectedText\`, text
    after the caret, and (for browsers) the focused window's URL, all consumed
    **in-process**. Accept (L): harness reads a known selection from a test
    target; absent AX permission the pipeline no-ops to today's path."

    MEASURED: \`git grep "AXSelectedText" -- desktop\` -> ONE hit, and it is a
    comment, not a call. The teardown's §2.4 scores "Selected text + text after
    caret" 🟡 "selection only in Command Mode" and "Browser URL resolution" ❌.

    THE PRIVACY FRAME IS THE POINT and must be in the PR body: the teardown's
    §2.4 table shows the competitor sends \`textbox_contents\`, \`axText\`,
    \`axHTML\` and a base64 screenshot to their API. Yap reads the same AX fields
    and they "die in a Rust function" (§5.2). This item widens what Yap reads,
    so it also widens the promise it has to keep.

    Do:
      * \`focus.rs\` gains \`AXSelectedText\` and after-caret text, alongside the
        existing before-caret read (YV50). In-process, no helper, no subprocess
        — the bundle stays the only TCC row (permissions.rs:4-6 explains why
        that matters).
      * Browser URL resolution for the focused window, for the three big
        browsers, used ONLY to disambiguate the dictation mode (Y4-F left this
        as a named limitation — this item closes it). Never stored, never
        logged, never in a support bundle, and never sent.
      * NO-OP WITHOUT THE GRANT. Absent Accessibility, the pipeline must behave
        exactly as it does today — asserted, not assumed, because a new AX read
        on a path that previously did not need one is a new way for a
        permission failure to break dictation.
      * Bound the reads: a huge AX field (a whole document after the caret) must
        be truncated to a named budget before it touches the prompt.
      * PRIV-A's outbound sweep must still pass, and PRIVACY.md must name the
        three new fields and the URL, with the sentence that they never leave.

    Tests \`tests/ax_context.rs\`: the no-grant no-op is byte-identical; an
    oversized field is truncated to the budget; the URL never reaches the DB, a
    log or a support bundle; the browser-mode disambiguation resolves Gmail to
    email and a docs URL to document.

    Depends on PERM-A/PERM-E (the grant model), Y4-F (the mode table), PRIV-A.

    What NOT to do:
      - Do NOT read \`axHTML\` or take a screenshot. The teardown flags both as
        the competitor's uploads and Yap's local promise makes them a liability
        (§2.4 marks screen OCR "❌ ➖ would break the local promise").
      - Do NOT store the URL.
      - Do NOT make the dictation path depend on a successful AX read.
  `,
  acceptance: `
    grep -q 'AXSelectedText' desktop/src-tauri/src/focus.rs
    test -f desktop/src-tauri/tests/ax_context.rs
    grep -q 'without_the_grant_the_pipeline_is_byte_identical' desktop/src-tauri/tests/ax_context.rs
    grep -q 'an_oversized_ax_field_is_truncated_to_the_budget' desktop/src-tauri/tests/ax_context.rs
    grep -q 'the_url_never_reaches_the_db_a_log_or_a_bundle' desktop/src-tauri/tests/ax_context.rs
    test 0 -eq "$(grep -c 'axHTML' desktop/src-tauri/src/focus.rs)"
    grep -q 'after the caret' PRIVACY.md
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test ax_context                        ; test $? -eq 0
    cargo test --features custom-protocol --test no_outbound_on_the_dictation_path ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'PERM-G', prompt: 'Y9', branch: 'loop/perm-g-vibe-coding-identifier-bias', gated: null,
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
  title: 'IDE context: the identifiers in the open file bias the transcription — the highest personal-ROI item',
  preflight: `
    grep -q 'ide_identifiers\\|vibe_context' desktop/src-tauri/src/vocab.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test ide_bias
  `,
  spec: `
    P1 #12, VERBATIM: "Vibe-coding context. (M · L) File tagging + variable
    recognition for VS Code / Cursor / Windsurf, via the IDE's screen-reader AX
    mode — exactly Wispr's mechanism, but the terms never leave the box. Feed
    identifiers into the whisper \`initial_prompt\` bias (the YV47 machinery
    already exists). Why: Wilson lives in Claude Code / Cursor; this is the
    highest personal-ROI item on the list. Accept (L): with a file open
    containing \`learn_from_transcript_tx\`, that identifier appears in the built
    bias prompt and is transcribed correctly in a fixture WAV."

    The machinery is genuinely all there:
      \`asr_engine.rs:37-49\` — the bias window, joined with ", ", "the shape
      upstream Handy feeds \`initial_prompt\`", with an explicit note that
      "overflowing it would silently drop the terms at the head".
      \`asr_engine.rs:549\` — "200 twelve-character terms is ~2.6 KB — far past
      the window", so the budget is already measured.
      \`vocab.rs\`, \`vocab_extract.rs\` — the YV47 dictionary/bias machinery.
      \`focus.rs\` — the frontmost app, and PERM-F's AX reads.

    Do:
      * When the frontmost app is VS Code, Cursor or Windsurf, read the open
        file's visible text via AX and extract IDENTIFIERS (snake_case,
        camelCase, PascalCase, SCREAMING_SNAKE tokens over a length floor).
      * Rank and TRUNCATE to the measured bias window. The window comment is
        explicit that overflow silently drops the terms at the HEAD, so ranking
        matters: identifiers near the caret first. Assert the truncation is
        deterministic and that the window is never exceeded — a silently
        dropped bias is a silently worse transcript.
      * Merge with the user's dictionary terms without either starving the
        other: a named split of the window between dictionary terms and IDE
        identifiers, stated in the doc comment.
      * Fixture test with the exact acceptance above:
        \`learn_from_transcript_tx\` in the open file appears in the built bias
        prompt. Use a committed synthetic "open file" fixture, not a live IDE.
      * The identifiers never leave the machine and never enter the DB, a log or
        a support bundle. They are the most sensitive thing on this list — they
        are the user's source code.
      * No-op without AX, and no-op for any app not on the IDE list.

    Tests \`tests/ide_bias.rs\`: the named identifier appears; the window is
    never exceeded; truncation is deterministic and caret-proximate;
    identifiers never reach the DB or a bundle; a non-IDE frontmost app
    contributes nothing; no-grant is byte-identical.

    Depends on PERM-F.

    What NOT to do:
      - Do NOT store the identifiers.
      - Do NOT exceed the bias window. Measure it from the constant, do not
        guess it.
      - Do NOT read a file from disk. AX only — reading the project off disk is
        a different and much larger promise.
  `,
  acceptance: `
    grep -qE 'ide_identifiers|vibe_context' desktop/src-tauri/src/vocab.rs
    test -f desktop/src-tauri/tests/ide_bias.rs
    grep -q 'learn_from_transcript_tx' desktop/src-tauri/tests/ide_bias.rs
    grep -q 'the_bias_window_is_never_exceeded' desktop/src-tauri/tests/ide_bias.rs
    grep -q 'truncation_is_deterministic_and_caret_proximate' desktop/src-tauri/tests/ide_bias.rs
    grep -q 'identifiers_never_reach_the_db_or_a_bundle' desktop/src-tauri/tests/ide_bias.rs
    grep -q 'a_non_ide_frontmost_app_contributes_nothing' desktop/src-tauri/tests/ide_bias.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test ide_bias                 ; test $? -eq 0
    cargo test --features custom-protocol --test asr_capabilities_probe   ; test $? -eq 0
    cargo test --features custom-protocol --test vocab_corpus_is_literals_only ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y9-D', prompt: 'Y9', branch: 'loop/y9-d-blocked-apps-and-focus-denylist', gated: null,
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
  title: 'A denylist where the hotkey is inert — the honest complement to reading your context',
  preflight: `
    grep -q 'denylist' desktop/src-tauri/src/focus.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test denylist
  `,
  spec: `
    P1 #14, VERBATIM: "Blocked apps / blocked URLs. (S · L) A denylist where the
    hotkey is inert. Why: the honest complement to 'we read your context' — and
    a natural home for password managers and banking sites. Accept (L):
    frontmost app on the denylist → PTT records nothing and shows a muted-bar
    state."
    And P2 #21: "Focus Mode. (M · L) \`opt+F\` blocks distracting apps/sites.
    Adjacent to #14; ships on the same denylist plumbing."

    VERIFIED ABSENT: \`git grep "blocked_apps\\|denylist\\|blocklist" -- desktop\`
    -> 0. The teardown scores it ❌ against Wispr's Experimental → Blocked Apps /
    Blocked URLs.

    Do:
      * A denylist of bundle identifiers and (with PERM-F's URL read) URL
        patterns. SEEDED with sensible defaults the user can remove: the common
        password managers. A denylist that ships empty is a feature nobody turns
        on.
      * On a denylisted frontmost app, the PTT is INERT: no capture is started,
        nothing is written, and the pill shows a muted state (Y5-C's vocabulary
        — reuse \`blocked\`, or add \`muted\` if the distinction reads better;
        decide and say why). Never a silent no-op, which is indistinguishable
        from a broken hotkey.
      * The check runs in Y1/PERM-C's single gate in \`start_recording\`
        alongside the microphone and license checks, in a stated order. Three
        gates in one place, one refusal reason each.
      * Secure-input fields are ALREADY refused by \`secure_input.rs\` at PASTE
        time (Y6-C tests it). The denylist is the complement at RECORD time.
        State the difference in the doc comment so the two are not merged.
      * Focus Mode rides the same list with an inverted sense and its own
        binding through Y8-A's table. Ship the plumbing; the blocking behaviour
        is a small step once the list exists.

    Tests \`tests/denylist.rs\`: a denylisted app starts no recorder; the pill
    receives a muted state; the default seed is non-empty; a URL pattern matches
    only with PERM-F's read available and no-ops without it; the three gates
    refuse in the stated order.

    Depends on PERM-C, PERM-F, Y5-C, Y8-A.

    What NOT to do:
      - Do NOT record and then discard. Nothing may be captured.
      - Do NOT ship an empty default list.
      - Do NOT log the denylisted app's name with the user's context attached.
  `,
  acceptance: `
    grep -q 'denylist' desktop/src-tauri/src/focus.rs
    test -f desktop/src-tauri/tests/denylist.rs
    grep -q 'a_denylisted_app_starts_no_recorder' desktop/src-tauri/tests/denylist.rs
    grep -q 'the_default_seed_is_not_empty' desktop/src-tauri/tests/denylist.rs
    grep -q 'the_three_gates_refuse_in_the_stated_order' desktop/src-tauri/tests/denylist.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test denylist ; test $? -eq 0
    cargo test --features custom-protocol --test mic_gate ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y9-E', prompt: 'Y9', branch: 'loop/y9-e-dictionary-and-snippet-bulk-io', gated: null,
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
  title: 'CSV round-trip for the dictionary and snippets, and the usage-frequency ranking that is only half there',
  preflight: `
    grep -q 'import_csv' desktop/src-tauri/src/db.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test bulk_io
  `,
  spec: `
    P1 #13, VERBATIM: "Bulk import/export (CSV) for dictionary + snippets.
    (S · L) Round-trip test."
    Plus the two 🟡s from the same §2.5 table, which belong with it:
    "Usage-frequency ranking 🟡" and "Replacement rules incl. HTML replacement
    🟡 plain text".

    VERIFIED ABSENT: \`git grep "csv_import\\|bulk_import" -- desktop\` -> 0.

    Do:
      * CSV import and export for the dictionary and for snippets, with a strict
        parser: a documented column set, a row that fails validation is
        REPORTED with its line number and skipped, and the import is atomic
        per-file (either the valid rows all land or none do — a half-imported
        dictionary is worse than a failed one). Write the parser; do not add a
        CSV dependency for a comma-splitter you can test.
      * Quoting and escaping are the whole difficulty: a replacement value
        containing a comma, a quote, a newline. Round-trip every one of those in
        the test, which is the acceptance the teardown asks for.
      * Usage-frequency ranking: the dictionary already learns from corrections
        (YV47) — add the frequency column and use it to rank the bias terms fed
        to PERM-G's window, where ranking now matters because the window
        overflows. That makes the 🟡 a ✅ and improves the transcript.
      * Rich-text/HTML snippet replacement is P2 #18 and is NOT in this item:
        the teardown notes it "interacts with the YV39 receipt-sequenced paste —
        needs its own slice". Say so in the doc comment so it is not
        half-attempted here.

    Tests \`tests/bulk_io.rs\`: round-trip with commas, quotes and newlines in
    values; a malformed row is reported with its line number; the import is
    atomic; export contains no license key and no home path; frequency ranking
    orders the bias terms.

    Depends on PERM-G (the ranking consumer).

    What NOT to do:
      - Do NOT add an xlsx or CSV library.
      - Do NOT half-import on a bad row.
      - Do NOT attempt HTML/rich-text replacement here.
  `,
  acceptance: `
    grep -q 'fn import_csv' desktop/src-tauri/src/db.rs
    grep -q 'fn export_csv' desktop/src-tauri/src/db.rs
    test -f desktop/src-tauri/tests/bulk_io.rs
    grep -q 'round_trips_commas_quotes_and_newlines' desktop/src-tauri/tests/bulk_io.rs
    grep -q 'a_malformed_row_is_reported_with_its_line_number' desktop/src-tauri/tests/bulk_io.rs
    grep -q 'the_import_is_atomic' desktop/src-tauri/tests/bulk_io.rs
    node -e "const d=require('./desktop/package.json').dependencies;process.exit(Object.keys(d).some(k=>/csv|xlsx|papaparse/.test(k))?1:0)"
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test bulk_io  ; test $? -eq 0
    cargo test --features custom-protocol --test ide_bias ; test $? -eq 0
  `,
})

// ── 50-y10-parity-p2-surface.mjs ──────────────────────────────────────────
// Y10 — THE P2 PARITY QUEUE: "surface & polish". Source of record:
// ~/Obsidian/Wilson-Brain/Notes/Wispr-Full-Parity-Research-2026-08-09.md §3 P2
// items 15-23, quoted per item. Two of the nine already left this file: #17
// earcons and #20 coaching nudges went into 40-y8 because they are the cheapest
// answers to "the app looks broken". #18 rich-text snippets is here. #21 Focus
// Mode shipped its plumbing in Y9-D.
//
// Separate file from Y9 so the two run on opposite lanes.
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'Y10-A', prompt: 'Y10', branch: 'loop/y10-a-multi-language-and-the-in-bar-picker', gated: null,
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
  title: 'Multi-language: expose what the engine can already do, with the picker in the bar',
  preflight: `
    grep -q 'language_set' desktop/src-tauri/src/lib.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test languages
  `,
  spec: `
    P2 #15, VERBATIM: "Multi-language + in-bar language picker. (M · L) Whisper
    is already multilingual; expose a language set + auto-detect and put the
    picker in the bar."

    MEASURED: \`language: "en"\` is the shipped default (lib.rs:400) and the
    teardown's §2.1 row scores Yap "❌ English-only path" against Wispr's
    "99-language auto-detect, or pick a language subset; language picker lives
    in the Flow Bar". The capability exists in the engine —
    \`asr_engine.rs:394\` carries \`supports_language_detect\` and
    \`supports_streaming\` in the probed capabilities, and
    \`tests/asr_capabilities_probe.rs\` already reads them.

    Do:
      * A language SET, not a single language: the user picks the languages they
        actually speak, and auto-detect chooses among that set. A 99-language
        auto-detect is worse than a 2-language one — it mis-detects.
      * Drive it off the probed capability, not a hardcoded list: if
        \`supports_language_detect\` is false for the installed model, the UI must
        say so and fall back to the single selected language. A picker that
        silently does nothing is the defect pattern this whole plan is about.
      * The picker is a Y8-B slot in the bar, and also a Settings control.
      * The formatting pipeline is English-shaped in places (spoken punctuation
        names, \`format_email_shape\`). Do NOT pretend otherwise: state per
        cleanup stage whether it is language-agnostic, and for a non-English
        take skip the stages that are not, rather than applying English rules to
        German. Assert that in a test.
      * The meeting path is English-only by design and has a test for it
        (\`tests/meeting_english_only_gate.rs\`). Keep it green; this item is
        dictation only.

    Tests \`tests/languages.rs\`: a model without detect support falls back and
    says so; auto-detect only chooses within the set; English-shaped cleanup
    stages are skipped for a non-English take; the default remains English for
    an existing install.

    What NOT to do:
      - Do NOT expose 99 languages.
      - Do NOT run the English spoken-punctuation table over a non-English take.
      - Do NOT touch the meeting English-only gate.
  `,
  acceptance: `
    grep -q 'language_set' desktop/src-tauri/src/lib.rs
    test -f desktop/src-tauri/tests/languages.rs
    grep -q 'a_model_without_detect_support_falls_back_and_says_so' desktop/src-tauri/tests/languages.rs
    grep -q 'english_shaped_stages_are_skipped_for_a_non_english_take' desktop/src-tauri/tests/languages.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test languages                 ; test $? -eq 0
    cargo test --features custom-protocol --test meeting_english_only_gate ; test $? -eq 0
    cargo test --features custom-protocol --test asr_capabilities_probe    ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'PERM-H', prompt: 'Y10', branch: 'loop/perm-h-microphone-ranking-and-device-intelligence', gated: null,
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
  title: 'A ranked microphone preference list, forget-device, and the AirPods and clamshell warnings',
  preflight: `
    grep -q 'mic_ranking' desktop/src-tauri/src/lib.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test mic_devices
  `,
  spec: `
    P2 #16, VERBATIM: "Mic ranking + device intelligence. (S · L) Ordered
    preference list, 'forget device', AirPods/clamshell/lid warnings. Yap
    already has cpal device enumeration (YV35)."
    The teardown's §2.1 row details what Wispr ships [BUNDLE]: "Ranked
    preference list, drag to reorder, 'forget device', AirPods warning,
    clamshell/lid-closed detection, Jabra wear-detection auto-switch, separate
    Notetaker mic", Yap "🟡 single device pick".

    Do:
      * An ordered preference list persisted in settings; the highest-ranked
        PRESENT device wins at take start. Drag to reorder. "Forget device"
        removes a row so an old headset stops winning.
      * AirPods warning: Bluetooth input is low-bandwidth and noticeably worse
        for ASR. Say so once, when an AirPods-class device is selected, and
        offer the built-in mic. This is the single most common cause of a bad
        transcript that looks like a model problem.
      * Clamshell / lid-closed: the built-in mic is unavailable or muffled.
        Detect and warn. The teardown names Wispr's
        \`settings_microphone_airpods_warning\` and
        \`NoClamshellBuiltInMic\` notification as the [BUNDLE]/[LOCAL] evidence
        that both cases are real enough to ship copy for.
      * A DEVICE CHANGE MID-TAKE must not lose the take.
        \`tests/matrix_row14_output_device_change.rs\` covers OUTPUT; input is
        untested. Assert the take either continues on the new device or is
        parked in recovery (Y3/DB-B), never silently truncated.
      * Interaction with PERM-A: a device being present is a HARDWARE question
        (\`input_device_present\`) and authorization is a TCC question
        (\`authorization_status\`). This item must not re-merge them; PERM-A split
        them deliberately.
      * No Jabra wear-detection. Vendor-specific and out of scope; say so.

    Tests \`tests/mic_devices.rs\`: the highest-ranked present device wins;
    forget removes it from selection; the AirPods warning fires once per
    selection; a mid-take input change does not silently truncate; ranking
    persists across a restart (in Y4-H's round-trip).

    Depends on PERM-A, DB-B, Y4-H.

    What NOT to do:
      - Do NOT conflate device presence with permission.
      - Do NOT switch devices mid-take without telling the user.
      - Do NOT add vendor-specific wear detection.
  `,
  acceptance: `
    grep -q 'mic_ranking' desktop/src-tauri/src/lib.rs
    test -f desktop/src-tauri/tests/mic_devices.rs
    grep -q 'the_highest_ranked_present_device_wins' desktop/src-tauri/tests/mic_devices.rs
    grep -q 'the_airpods_warning_fires_once_per_selection' desktop/src-tauri/tests/mic_devices.rs
    grep -q 'a_mid_take_input_change_does_not_silently_truncate' desktop/src-tauri/tests/mic_devices.rs
    grep -q 'presence_and_authorization_stay_separate' desktop/src-tauri/tests/mic_devices.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test mic_devices     ; test $? -eq 0
    cargo test --features custom-protocol --test mic_auth_status ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y10-B', prompt: 'Y10', branch: 'loop/y10-b-rich-text-snippets-on-the-pasteboard', gated: null,
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
  title: 'Rich-text snippets: RTF and HTML flavours on the pasteboard without racing the receipt-sequenced paste',
  preflight: `
    grep -q 'rtf\\|public.rtf' desktop/src-tauri/src/paste.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test rich_snippets
  `,
  spec: `
    P2 #18, VERBATIM: "Rich-text snippets. (M · L) RTF/HTML flavours on
    \`NSPasteboard\`; interacts with the YV39 receipt-sequenced paste — needs its
    own slice."
    The teardown's §2.5 rows: Wispr's snippets are "rich text
    (bold/italic/links/lists)" against Yap's "✅ plain-text (YV48)", and
    "Replacement rules incl. HTML replacement" against Yap's "🟡 plain text".

    The hazard named in the spec is the whole item. \`paste_tx.rs\` (641 lines,
    YV39) sequences pastes by receipt precisely so two takes cannot interleave;
    writing MULTIPLE pasteboard flavours is several writes where there was one,
    and a half-written pasteboard is a paste of the wrong thing.

    Do:
      * Write all flavours for one paste as a single atomic pasteboard
        declaration — declare the types, then set each — inside the existing
        receipt transaction. Never a second transaction, never a write outside it.
      * Flavours: \`public.utf8-plain-text\` always (so every target works), plus
        \`public.rtf\` and/or \`public.html\` when the snippet carries markup. A
        target that cannot take rich text must still get the plain text.
      * Snippet storage gains a content-type. A plain snippet stays byte-for-byte
        what it is today; assert that, because YV48's existing snippet tests are
        the regression floor.
      * The signature block is copied BYTE FOR BYTE after polish
        (\`snippets::append_signature\`, lib.rs:283) — if a signature can now be
        rich, it must stay byte-identical in the plain flavour and the rich
        flavour must be derived, never re-authored by a model. Assert it.
      * Y6-C's paste_target_e2e must be extended, not duplicated: two takes in
        quick succession with rich flavours cannot interleave.

    Tests \`tests/rich_snippets.rs\`: all flavours land in one transaction; a
    plain snippet is byte-identical to today; a rich snippet degrades to plain
    on a plain-only target; a signature stays byte-identical; no interleaving.

    Depends on Y6-C.

    What NOT to do:
      - Do NOT write the pasteboard outside the receipt transaction.
      - Do NOT drop the plain-text flavour.
      - Do NOT let a model author the rich version of a signature.
  `,
  acceptance: `
    grep -qE 'public.rtf|public.html' desktop/src-tauri/src/paste.rs
    test -f desktop/src-tauri/tests/rich_snippets.rs
    grep -q 'all_flavours_land_in_one_receipt_transaction' desktop/src-tauri/tests/rich_snippets.rs
    grep -q 'a_plain_snippet_is_byte_identical_to_today' desktop/src-tauri/tests/rich_snippets.rs
    grep -q 'a_signature_stays_byte_identical' desktop/src-tauri/tests/rich_snippets.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test rich_snippets    ; test $? -eq 0
    cargo test --features custom-protocol --test paste_target_e2e ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y10-D', prompt: 'Y10', branch: 'loop/y10-d-mouse-button-push-to-talk', gated: null,
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
  title: 'A non-primary mouse button as push-to-talk',
  preflight: `
    grep -q 'mouse_ptt\\|MouseBinding' desktop/src-tauri/src/ptt_macos.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test mouse_ptt
  `,
  spec: `
    P2 #22, VERBATIM: "Mouse-button PTT. (M · L) Non-primary mouse button as
    the hotkey."
    The teardown's §2.1 row on Wispr's version ([BUNDLE]
    \`settings_hotkey_dialog_mx_master_*\`, [OFFICIAL] whats-new 2026-03-31):
    "Bind a non-primary mouse button as PTT; dedicated MX Master setup flow;
    also an Enter rebind so a mouse button sends the message", Yap ❌.

    \`ptt_macos.rs\` (590 lines) already owns the CGEvent tap for the
    modifier-only fn / fn⌃ hold, which is the hard part — a mouse button is
    another event type on the same tap.

    Do:
      * Bind button 3+ (never the primary or secondary button — stealing a
        right-click is unacceptable and must be impossible, not merely
        discouraged). Enforce it in the binding validator (Y8-A).
      * The tap must not swallow the event for other apps. A PTT mouse button
        that also fires in the game or the design tool the user is in is worse
        than no feature; a PTT button that is swallowed everywhere is also wrong.
        Decide, state the decision, and test the pass-through behaviour.
      * Input Monitoring is required for a raw button tap the same way it is for
        the modifier-only hold — PERM-E added Input Monitoring to the permission
        report; this item consumes it. Without the grant, the feature must be
        visibly unavailable rather than silently dead.
      * No MX-Master-specific setup flow. Generic, any mouse.

    Tests \`tests/mouse_ptt.rs\`: primary and secondary buttons are rejected by
    the validator; a bound button starts and stops a take; pass-through is as
    decided; without Input Monitoring the feature reports unavailable.

    Depends on Y8-A, PERM-E.

    What NOT to do:
      - Do NOT allow binding the primary or secondary button.
      - Do NOT ship it silently dead without Input Monitoring.
      - Do NOT add a vendor-specific setup flow.
  `,
  acceptance: `
    grep -qE 'mouse_ptt|MouseBinding' desktop/src-tauri/src/ptt_macos.rs
    test -f desktop/src-tauri/tests/mouse_ptt.rs
    grep -q 'primary_and_secondary_buttons_are_rejected' desktop/src-tauri/tests/mouse_ptt.rs
    grep -q 'without_input_monitoring_the_feature_reports_unavailable' desktop/src-tauri/tests/mouse_ptt.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test mouse_ptt           ; test $? -eq 0
    cargo test --features custom-protocol --test shortcut_validation ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y10-E', prompt: 'Y10', branch: 'loop/y10-e-local-insights-v2-and-the-yappy-profile', gated: null,
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
  title: 'Insights v2: the numbers Wispr computes in the cloud, computed in SQLite, feeding Yappy\'s dialogue',
  preflight: `
    grep -q 'most_corrected_word' desktop/src-tauri/src/db.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test insights_v2
  `,
  spec: `
    P2 #23, VERBATIM: "Local Insights v2 / Yappy profile. (M · D) 'most
    corrected word', 'peak hour', 'catch phrase' — Wispr computes these in the
    cloud; Yap can do it in SQLite. Feeds Yappy's dialogue."
    The teardown's §2.5 row on Wispr's gamified Voice Profile [LOCAL]/[BUNDLE]:
    \`superpower_title\`, \`catch_phrase\`, \`persona\`, \`most_used_word\`,
    \`most_removed_word\`, \`peak_time_top_app\`, \`word_count_milestone\`, with the
    note "Yappy is the better version of this idea".

    Yap has the raw material and is not using it: the dictionary learns from
    corrections (YV47), \`raw_text\` and the formatted text are both stored
    (YV10/51), \`get_insights\` and a day-series exist (lib.rs:2420-2432), and
    DB-A's usage rollup lands the per-day words and voiced seconds.

    Do:
      * Computed in SQL over the existing tables, no new capture: most-used
        word (stopword-filtered), most-CORRECTED word (from the raw-vs-final
        diff Y4-G's diff module already computes — reuse it, do not write a
        second differ), peak hour, peak app, longest take, current streak,
        word-count milestones.
      * Bounded cost: these run on demand when the Insights view opens, not on
        every take, and DB-C's volume test must still pass with them
        (10,000 takes, no full scans on the take path).
      * FEEDS YAPPY. Y5-H's habitat reacts to real state; these are the richest
        real state Yap has. Wire at least three of them into the habitat's
        reaction table and into the pill's commentary via \`pill/tone.ts\`, so
        the numbers become personality rather than a dashboard
        (feedback_no_generic_ui: reject AI-dashboard aesthetics).
      * Nothing is transmitted and nothing is a leaderboard. The teardown's
        "explicitly not wanted" list includes teams and leaderboards.
      * Insights must render honestly at ZERO takes — Y5-B's empty state for
        this view exists because today the view renders blank (App.tsx:2953
        renders nothing when \`insights\` is falsy).

    Tests \`tests/insights_v2.rs\`: each metric on a seeded corpus with a known
    answer; stopwords excluded from most-used; most-corrected uses the stored
    raw-vs-final pair; zero takes yields a defined empty result, never a panic
    and never a divide-by-zero; no query added to the take path.

    Depends on DB-A, DB-C, Y4-G, Y5-H.

    What NOT to do:
      - Do NOT compute these on every take.
      - Do NOT write a second diff implementation.
      - Do NOT transmit any of it, and do not build a leaderboard.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - DB-A (usage metering) was KILLED today. Strike "DB-A's usage rollup lands the per-day words and voiced seconds" and "Depends on DB-A": read the
        per-day words and voiced seconds from the EXISTING get_insights day-series (lib.rs:2420-2432), which this item's own EVIDENCE already cites.
        Still depends on DB-C, Y4-G, Y5-H. No new rollup table.
  `,
  acceptance: `
    grep -q 'most_corrected_word' desktop/src-tauri/src/db.rs
    test -f desktop/src-tauri/tests/insights_v2.rs
    grep -q 'stopwords_are_excluded_from_most_used' desktop/src-tauri/tests/insights_v2.rs
    grep -q 'zero_takes_yields_a_defined_empty_result' desktop/src-tauri/tests/insights_v2.rs
    grep -q 'no_query_was_added_to_the_take_path' desktop/src-tauri/tests/insights_v2.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test insights_v2       ; test $? -eq 0
    cargo test --features custom-protocol --test history_at_volume ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y10-F', prompt: 'Y10', branch: 'loop/y10-f-publish-the-idle-cost-number', gated: null,
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop. 2026-09-26 evidence: yap23 install measured idle RSS ~135-138 MB (pid 87890); wakeups were never measured — yap24-OS5 depends on this item.`,
  title: 'Measure and publish Yap\'s idle RAM and CPU — the free marketing line the research asked for',
  preflight: `
    test -f desktop/src-tauri/tests/idle_cost.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test idle_cost
  `,
  spec: `
    From the teardown §2.11, verbatim: "Idle cost measured by users at ~800 MB
    RAM / ~8% CPU; 8-10 s cold start ... **Yap's position:** a Rust/Tauri binary
    with a warm GGUF engine and idle-throttled canvases (YV24) should beat that
    by an order of magnitude. **Measure it and publish the number — it is free
    marketing.**"

    Yap's own recorded numbers: "138MB idle" for the build carrying the diarize
    sidecar (project_yap_build_state, 2026-08-16), and an explicit energy pass
    (YV80 lazy model load, YV81 no busy timers / idle animations park / the
    polish sidecar unloads when unused).

    Do:
      * \`tests/idle_cost.rs\`: launch the built binary headless, let it settle,
        and assert resident memory and CPU are under named ceilings — with the
        machine and date in a comment, and the ceilings set with enough headroom
        that they fail on a REGRESSION and not on a different Mac. Express CPU
        as a ceiling over a sampling window, never an instantaneous read.
      * Assert the specific things the energy pass bought, so they cannot erode:
        no ASR model resident before the first take (YV80 — and
        \`tests/meeting_no_model_resident.rs\` is the existing sibling
        assertion, follow its shape), the polish sidecar not running when unused
        (YV81), and no timer firing faster than the documented floor while idle.
      * Cold start: measure and assert a ceiling.
      * Publish the numbers in README.md and on the site copy, next to the
        claim, with the measurement method in one sentence. A published number
        with no method is a number nobody believes.

    Depends on Y3-G (which establishes the long-take budget harness — reuse its
    measurement helpers rather than writing a second sampler).

    What NOT to do:
      - Do NOT publish a number you did not measure on a build from this repo.
      - Do NOT name the competitor's number in Yap's own marketing copy
        (no competitor jabs). Publish Yap's number and let it stand alone.
      - Do NOT set a ceiling so tight it goes red on a different Mac.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/idle_cost.rs
    grep -q 'no_asr_model_is_resident_before_the_first_take' desktop/src-tauri/tests/idle_cost.rs
    grep -q 'the_polish_sidecar_is_not_running_when_unused' desktop/src-tauri/tests/idle_cost.rs
    grep -q 'no_timer_fires_faster_than_the_documented_floor_while_idle' desktop/src-tauri/tests/idle_cost.rs
    grep -qE 'idle' README.md
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test idle_cost                ; test $? -eq 0
    cargo test --features custom-protocol --test meeting_no_model_resident ; test $? -eq 0
  `,
})

// ── 55-y11-diarization-carryforward.mjs ───────────────────────────────────
// Y11 — THE PARKED DIARIZATION QUEUE. The yap23 loop was stopped mid-finisher on
// a fast-track order; six items were PARKED with real blocking defects, each with
// a tracking issue on the repo carrying the `audit-carryforward` label, and six
// PRs left open. Verified live on 2026-09-12 with
// `gh issue list --repo wilsonguenther-dev/wilson-voice --state open` (6 open,
// #150-#155) and the refs `origin/feat/yv127`…`origin/feat/yv134`.
// docs/loop/HARNESS.md records the same six PRs and says Drain settles them.
//
// The resume note is explicit that PARKED = the FIRST work of the next Yap
// session, and that ALL SIX also need a rebase picking up `min_embed`.
//
// Also carried forward, and the reason Y11-G exists: the eval numbers standing
// in the repo are REGRESSION FLOORS measured on a `say`-generated synthetic
// corpus (DER 0.34-0.45, EER 0.272 — the embedder hears the synthesizer), and a
// real-voice corpus is a known, named next need.
//
// This file runs LAST. Diarization is the notetaker's accuracy layer; nothing in
// Wilson's 2026-09-12 list depends on it, and it must not delay the dictation
// work. It is in the plan because leaving six blocking defects untracked in a
// shipped subsystem is how they become folklore.
//
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'Y11-A', prompt: 'Y11', branch: 'loop/y11-a-rebase-the-six-parked-branches-on-min-embed', gated: null,
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop. AMENDED 2026-09-26: the pre-flight used to require ZERO open PRs in the whole repo, which can never pass while loop-build PRs are open; it now counts only the six parked feat/yv PRs (#142-#146, #149, still open 2026-09-26).`,
  title: 'Rebase the six parked branches onto main so each is evaluated against the shipped min_embed, not against what main was',
  preflight: `
    test 0 -eq "$(git branch -r --list 'origin/feat/yv1*' | wc -l)"
    gh pr list --repo wilsonguenther-dev/wilson-voice --state open --json headRefName --jq '[.[] | select(.headRefName | startswith("feat/yv"))] | length' | grep -qx 0
  `,
  spec: `
    MEASURED on 2026-09-12:
      git for-each-ref  ->  origin/feat/yv127 (d6667ac), yv128 (6e386e1),
                            yv129 (3d78900), yv130 (77a0c95), yv131 (5353261),
                            yv134 (de0c4d6), all dated 2026-08-15/16.
      origin/main       ->  4e8c9adf (YV126).
      gh issue list     ->  #150-#155, all labelled audit-carryforward.

    The resume note's instruction, which is the whole of this item: all six
    "need a rebase picking up min_embed". Until they do, every review of them is
    a review of a tree that no longer exists — and the sibling lesson here is
    exact: main moves under long loops, so rebase before evaluating checks.

    Do, per branch, in issue order (#150 first):
      1. Rebase onto current main. Rerun the full gate
         (docs/loop/HARNESS.md "The gate", all eight commands, exit codes read
         bare).
      2. Re-read its tracking issue and decide ONE of three outcomes, recorded
         in the PR:
            - the defect is fixed by the rebase (main moved past it) -> close the
              issue with the evidence and merge or close the PR accordingly;
            - the defect survives -> the PR stays open, the issue stays open, and
              the specific item below (Y11-B..F) owns the fix;
            - the branch is superseded -> close the PR naming the commit that
              supersedes it, and keep the issue if the defect is still live.
      3. Never merge a rebased branch whose tracking issue is still open and
         unaddressed. That is the exact mistake that created six carry-forwards.
      * Also settle the one open watch item: docs/loop/HARNESS.md and the resume
        note both flag a transient red main run on c3fd5d89 (cargo test exit
        101, logs expired, two green runs since). Run
        \`cargo test --features custom-protocol\` ten times in a row on current
        main and record the pass count in the PR body. If it recurs, capture the
        log immediately — "CI is flaky" must not be allowed to take root.

    What NOT to do:
      - Do NOT merge a parked branch to clear the queue. Six open PRs is not the
        problem; six unaddressed defects is.
      - Do NOT close a tracking issue without the evidence in the comment.
      - Do NOT squash the six into one branch. Each has its own defect and its
        own issue.
  `,
  acceptance: `
    test 0 -eq "$(git branch -r --list 'origin/feat/yv12*' | wc -l)"
    test 0 -eq "$(git branch -r --list 'origin/feat/yv13*' | wc -l)"
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol ; test $? -eq 0
    cargo clippy --all-targets --features custom-protocol ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y11-B', prompt: 'Y11', branch: 'loop/y11-b-overlap-honesty-in-the-shipped-string', gated: null, builder: 'sonnet',
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
  title: 'Issue #150: the impossibility framing survives in transcript.ts where the guard cannot see it',
  preflight: `
    gh issue view 150 --repo wilsonguenther-dev/wilson-voice --json state --jq .state | grep -qx CLOSED
  `,
  spec: `
    Tracking issue #150, title verbatim: "YV127 overlap honesty (#142): the
    impossibility framing survives in transcript.ts and the guard cannot see it".
    Labelled \`audit-carryforward\` = "Verified finding carried forward from a
    reflection/cleanup pass".

    Read #150 and PR #142 in full before writing code. The shape of the defect,
    which is the shape this whole plan keeps finding: a guard was added, a
    string it was supposed to police lives in a file the guard's scope does not
    cover, and the guard therefore passes while the wrong sentence ships. A grep
    proving absence with the wrong SCOPE proves nothing.

    Do:
      * Fix the surviving string in \`desktop/src/meetings/transcript.ts\` so the
        user-facing framing is accurate.
      * WIDEN THE GUARD so it could not have missed it: the guard must cover
        every file that can produce user-facing transcript copy — the TS
        rendering modules and the Rust export path both — and the test must name
        its scope explicitly rather than relying on a default search root.
      * Prove it non-vacuously: reintroduce the offending sentence in a scratch
        copy and assert the guard now fails. Record that in the PR body (the
        repo's own convention, docs/pr-screenshots/YV105/…/non-vacuous-mutations.txt),
        and add the row to Y7-C's docs/loop/MUTATIONS.md table.
      * Keep \`tests/meeting_transcript_render_two_track.rs\`,
        \`meeting_transcript_render_single_track_unchanged.rs\` and the vitest
        \`src/meetings/transcript.test.ts\` green.
      * Close #150 with the evidence.

    Depends on Y11-A.

    What NOT to do:
      - Do NOT widen the guard by searching the whole repo without a scope. A
        repo-wide grep that matches a doc or a test fixture is a guard that will
        be disabled the first time it is inconvenient.
      - Do NOT fix the string without fixing the guard. The guard is the defect.
  `,
  acceptance: `
    gh issue view 150 --repo wilsonguenther-dev/wilson-voice --json state --jq .state | grep -qx CLOSED
    grep -q 'transcript.ts' docs/loop/MUTATIONS.md
    cd ${APP} && npm ci
    npm test ; test $? -eq 0
    cd src-tauri
    cargo test --features custom-protocol --test meeting_transcript_render_two_track ; test $? -eq 0
    cargo test --features custom-protocol --test meeting_transcript_render_single_track_unchanged ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'DB-E', prompt: 'Y11', branch: 'loop/db-e-speaker-profiles-store-the-weights-digest', gated: null,
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
  title: 'Issue #151: speaker_profiles stores a catalog id where it must store the pinned weights digest',
  preflight: `
    gh issue view 151 --repo wilsonguenther-dev/wilson-voice --json state --jq .state | grep -qx CLOSED
  `,
  spec: `
    Tracking issue #151, verbatim: "YV128 speaker_profiles (#143): the model
    skip stores a catalog id, not the pinned weights digest".

    Why it is blocking and not cosmetic: a speaker embedding is only comparable
    to another embedding produced by the SAME WEIGHTS. A catalog id is a
    mutable label — the same id can point at re-quantized or updated weights —
    so a profile enrolled under one set of weights will be silently scored
    against another, and the failure looks like a bad match rather than a
    schema bug. The repo already pins digests elsewhere: YV123 vendored the
    models via a mirror and \`tests/supply_chain.rs\` asserts the sidecar pins
    sherpa-onnx exactly. This is that same discipline, one layer up.

    Do:
      * A migration adding the weights DIGEST to \`speaker_profiles\` (and to the
        skip record the issue names), idempotent
        (tests/db_migration_idempotent.rs must stay green).
      * Enrollment records the digest of the weights that produced the
        embedding. Scoring REFUSES to compare across digests — refuses, not
        silently re-embeds and not silently scores anyway. A refusal is a
        visible state the UI can explain; a cross-weights score is a wrong
        answer.
      * A re-enrollment path so a user whose model changed can re-enroll rather
        than lose their roster.
      * There are no users of this feature to migrate (it shipped days before
        the loop stopped), so prefer the correct schema over a compatibility
        shim — but do not destroy existing rows: mark them as
        unknown-digest and refuse to score them until re-enrolled.

    Tests \`tests/speaker_profile_digest.rs\`: enrollment stores the digest;
    scoring across digests refuses; an unknown-digest row is not scored and not
    deleted; the migration is idempotent; re-enrollment clears the refusal.

    Close #151 with the evidence. Depends on Y11-A.

    What NOT to do:
      - Do NOT store the catalog id as a proxy for the digest.
      - Do NOT silently re-embed on a digest mismatch.
      - Do NOT delete existing profile rows.
  `,
  acceptance: `
    gh issue view 151 --repo wilsonguenther-dev/wilson-voice --json state --jq .state | grep -qx CLOSED
    grep -qE 'weights_digest|weights_sha' desktop/src-tauri/src/db.rs
    test -f desktop/src-tauri/tests/speaker_profile_digest.rs
    grep -q 'scoring_across_digests_refuses' desktop/src-tauri/tests/speaker_profile_digest.rs
    grep -q 'an_unknown_digest_row_is_not_scored_and_not_deleted' desktop/src-tauri/tests/speaker_profile_digest.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test speaker_profile_digest  ; test $? -eq 0
    cargo test --features custom-protocol --test db_migration_idempotent ; test $? -eq 0
    cargo test --features custom-protocol --test supply_chain            ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y11-C', prompt: 'Y11', branch: 'loop/y11-c-enrollment-bands-retuned-on-the-scored-population', gated: null,
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
  title: 'Issue #152: bands tuned on utterance pairs, applied to roster-max centroid scoring — FAR 1.000 on the shipped path',
  preflight: `
    gh issue view 152 --repo wilsonguenther-dev/wilson-voice --json state --jq .state | grep -qx CLOSED
  `,
  spec: `
    Tracking issue #152, verbatim: "YV129 enrollment matching (#144): bands
    tuned on utterance pairs, applied to roster-max centroid scoring
    (FAR 1.000)". The resume note flags it with an exclamation mark for a
    reason: "FAR 0.034 tuned vs 1.000 shipped path".

    This is the single most serious defect in the parked queue. A false-accept
    rate of 1.000 on the shipped scoring path means the matcher accepts
    EVERY candidate — the feature reports confident speaker identities that are
    not identities at all. And the tuned number that was recorded, 0.034, was
    measured on a DIFFERENT scoring population (utterance-to-utterance pairs)
    than the one that ships (max over a roster of centroids). A threshold tuned
    on one population and applied to another is not a threshold.

    Do:
      * Retune the bands on the POPULATION THAT SHIPS: roster-max over
        centroids, at the roster sizes the product actually produces. Report FAR
        and FRR at the chosen operating point, and report them per roster size —
        roster-max FAR grows with roster size and a single number hides that.
      * Where the honest answer is "this mechanism cannot reach a usable FAR at
        roster size N", SAY SO and gate the feature at N rather than shipping a
        threshold that cannot hold. A refused identification is a product state;
        a wrong one is a defect. YV126's own resolution was "a floor instead of
        a reject", so the precedent for an honest floor already exists in this
        subsystem.
      * Every number written into the repo must name the population, the corpus,
        the roster size and the date. The standing failure mode here is a
        measurement outliving the thing that produced it, and ci.yml already
        carries a guard built for exactly that
        (\`meeting_eval_anti_alias_eer_measurement_stays_backed_by_a_real_backend\`)
        — keep it green and follow its example.
      * The corpus caveat is not optional: the numbers standing in the repo are
        floors on a \`say\`-generated synthetic corpus where the embedder hears
        the synthesizer. Any number produced here inherits that caveat until
        Y11-G lands a real-voice corpus, and the PR body must say so in one
        sentence.

    Tests: extend \`tests/diarization_metrics.rs\` and \`tests/meeting_eval.rs\`
    with the roster-max arm, and add
    \`bands_are_tuned_on_the_population_that_ships\` asserting the tuning
    harness and the shipped scorer call the SAME function.

    Close #152 with the numbers. Depends on Y11-A, DB-E.

    What NOT to do:
      - Do NOT re-report the 0.034 figure. It describes a path that does not ship.
      - Do NOT ship a threshold you could not measure.
      - Do NOT present a synthetic-corpus number as a real-voice number.
  `,
  acceptance: `
    gh issue view 152 --repo wilsonguenther-dev/wilson-voice --json state --jq .state | grep -qx CLOSED
    grep -q 'bands_are_tuned_on_the_population_that_ships' desktop/src-tauri/tests/diarization_metrics.rs
    grep -qE 'roster' desktop/src-tauri/tests/meeting_eval.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test diarization_metrics ; test $? -eq 0
    cargo test --features custom-protocol --test meeting_eval        ; test $? -eq 0
    cargo test --features custom-protocol --test meeting_cluster_attribution ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y11-D', prompt: 'Y11', branch: 'loop/y11-d-split-partition-seeding-and-the-outlier', gated: null,
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
  title: 'Issue #153: split_partition\'s farthest-pair seeding does not separate speakers when an outlier is the far point',
  preflight: `
    gh issue view 153 --repo wilsonguenther-dev/wilson-voice --json state --jq .state | grep -qx CLOSED
  `,
  spec: `
    Tracking issue #153, verbatim: "YV130 correction UX (#145):
    split_partition's farthest-pair seeding does not separate speakers when an
    outlier is the far point".

    A farthest-pair seed is the textbook 2-means initialisation and it has a
    textbook failure: if the two most distant embeddings are one real speaker
    and one outlier (a cough, a door, a clipped segment), the split separates
    the outlier from everything else and the two speakers stay merged. The
    user-visible symptom is a correction action that appears to do nothing,
    which is worse than an absent feature — this lives under "correction UX"
    for that reason.

    Do:
      * Replace or guard the seeding. Options, in the order worth trying:
        k-means++ style probabilistic seeding; a trimmed farthest pair that
        excludes points beyond a robust distance quantile; or a seed pair chosen
        to maximise the resulting partition's separation rather than the pair
        distance. Implement ONE, measured against the others on the eval corpus,
        and say in the PR body which you tried and what each scored.
      * The outlier itself must go somewhere defensible: assign it to its
        nearest cluster or to a named noise bucket, and NEVER let one outlier
        become a "speaker" in the roster. A phantom speaker in the transcript is
        the worst outcome of this bug.
      * Reuse \`tests/diarize_cluster_distance_threshold.rs\` and
        \`diarize_cluster_rank_and_floor.rs\` — YV126 established the distance
        threshold and the floor, and this fix must not move either.
      * Regression case, committed: a synthetic partition with one extreme
        outlier and two genuinely distinct speakers, where the old seeding
        provably fails and the new one provably separates. That fixture IS the
        proof; without it this is an unverifiable refactor.

    Tests \`tests/diarize_split_seeding.rs\`: the outlier fixture separates the
    two speakers; one outlier never becomes a roster speaker; the seeding is
    deterministic for a given input (no unseeded randomness — the loop's
    generated scripts forbid nondeterminism and an eval that changes per run is
    not an eval); the YV126 threshold and floor are unchanged.

    Close #153 with the numbers. Depends on Y11-A, Y11-C.

    What NOT to do:
      - Do NOT introduce unseeded randomness. If the seeding is probabilistic,
        the seed is an explicit parameter.
      - Do NOT let an outlier become a speaker.
      - Do NOT change the YV126 distance threshold in this item.
  `,
  acceptance: `
    gh issue view 153 --repo wilsonguenther-dev/wilson-voice --json state --jq .state | grep -qx CLOSED
    test -f desktop/src-tauri/tests/diarize_split_seeding.rs
    grep -q 'the_outlier_fixture_separates_the_two_speakers' desktop/src-tauri/tests/diarize_split_seeding.rs
    grep -q 'one_outlier_never_becomes_a_roster_speaker' desktop/src-tauri/tests/diarize_split_seeding.rs
    grep -q 'seeding_is_deterministic_for_a_given_input' desktop/src-tauri/tests/diarize_split_seeding.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test diarize_split_seeding              ; test $? -eq 0
    cargo test --features custom-protocol --test diarize_cluster_distance_threshold ; test $? -eq 0
    cargo test --features custom-protocol --test diarize_cluster_rank_and_floor     ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y11-E', prompt: 'Y11', branch: 'loop/y11-e-false-mechanism-sentence-in-the-shipped-asset', gated: null, builder: 'sonnet',
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
  title: 'Issues #154 and #155: a false mechanism claim in a shipped asset, and a comment naming call sites that do not exist',
  preflight: `
    gh issue view 154 --repo wilsonguenther-dev/wilson-voice --json state --jq .state | grep -qx CLOSED
    gh issue view 155 --repo wilsonguenther-dev/wilson-voice --json state --jq .state | grep -qx CLOSED
  `,
  spec: `
    Two issues, one class of defect, so one item.

    #154, verbatim: "YV131 cross-device drift (#146): the false-mechanism
    sentence survives in the shipped cohort asset and its generator".
    #155, verbatim: "YV134 phase-closing E2E (#149): the 0.35 comment claims
    shipped call sites that do not exist".

    Both are FALSE CAPABILITY CLAIMS in the tree: prose asserting a mechanism or
    a call site that is not there. That class is a BLOCKING finding by the
    reviewer contract, and it is corrosive beyond its size — the next engineer
    reads the comment, believes the mechanism, and builds on it. The repo has
    already been bitten: ci.yml carries a long note about an earlier revision of
    that very file claiming a precondition it never reached
    ("It was not, and could not be ... The arm returns on its first line here").

    Do:
      * #154: fix the sentence in the shipped cohort asset AND in its GENERATOR,
        which is the part that matters — fixing only the asset means the next
        regeneration restores the false claim. Then add the guard that would
        have caught it, scoped to include generated assets, and prove it
        non-vacuously.
      * #155: delete or correct the 0.35 comment, and — the real fix — add the
        assertion that a comment naming a call site must name one that exists.
        The repo has the precedent:
        \`meeting_eval_anti_alias_eer_ci_does_not_declare_what_it_never_reaches\`
        is exactly this kind of guard. Follow it, and keep
        \`tests/matrix_coverage.rs\` green.
      * Both rows go into Y7-C's docs/loop/MUTATIONS.md.
      * Then sweep once for the same class across the diarization modules: any
        doc comment naming a constant, a call site or a mechanism must name one
        that exists. Report the count found in the PR body, fix them, and say
        plainly if the sweep is not exhaustive and why — an honest partial sweep
        beats a claimed complete one.

    Close #154 and #155 with the evidence. Depends on Y11-A.

    What NOT to do:
      - Do NOT fix the asset without fixing the generator.
      - Do NOT delete a comment to pass a guard. Either the mechanism exists and
        the comment is right, or the comment goes and the guard proves it went.
      - Do NOT claim an exhaustive sweep you did not run.
  `,
  acceptance: `
    gh issue view 154 --repo wilsonguenther-dev/wilson-voice --json state --jq .state | grep -qx CLOSED
    gh issue view 155 --repo wilsonguenther-dev/wilson-voice --json state --jq .state | grep -qx CLOSED
    grep -q 'cohort' docs/loop/MUTATIONS.md
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test matrix_coverage ; test $? -eq 0
    cargo test --features custom-protocol --test meeting_eval    ; test $? -eq 0
    cargo test --features custom-protocol ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y11-F', prompt: 'Y11', branch: 'loop/y11-f-real-voice-eval-corpus', gated: 'panel',
  notes: `STATUS 2026-09-26: GATED (owner decision pending) — unchanged.`,
  title: 'A real-voice eval corpus to replace the synthetic one — the numbers are only floors until it exists',
  preflight: `
    grep -q 'corpus: synthetic' desktop/src-tauri/tests/meeting_eval.rs
  `,
  spec: `
    GATED: 'panel'. The mechanism is clear; what a corpus of real human voices
    may contain and where it may live are Wilson's calls, not an agent's.

    THE PROBLEM, from the yap23 close-out verbatim: "Eval numbers so far are
    REGRESSION FLOORS on a synthetic say-voice corpus (DER 0.34-0.45,
    EER 0.272 — CAM++ hears the synthesizer; real-voice corpus is a known next
    need)." A speaker embedder scored on text-to-speech output is being asked to
    tell apart one synthesizer from itself; the numbers bound regressions and
    say nothing about accuracy on people. Every number Y11-C produces inherits
    that ceiling.

    WHAT THE PANEL AND WILSON MUST DECIDE before this item is built:
      1. WHOSE VOICES. A public research corpus under a licence that permits
         this use, voices recorded with explicit consent, or both. Each has a
         different consent and licence posture.
      2. WHERE IT LIVES. It cannot be committed to a public repo. The existing
         scan corpus precedent is an external drive plus a manifest with
         checksums in the repo — the eval already works this way
         (tests/fixtures/meeting_eval_manifest.json plus its .sha256, and
         meeting_eval.rs opening on a corpus path no runner has).
      3. CONSENT AND RETENTION. Recorded voices are biometric data. How long is
         it kept, who can access it, and what does a contributor's withdrawal
         require? Yap's whole position is local-only privacy; an eval corpus of
         real voices sitting on a build box is the one place that promise could
         be undone by its own tooling.
      4. WHETHER CI EVER SEES IT. The answer is almost certainly no, and the
         existing pattern already handles that honestly: the corpus arm returns
         on its first line in CI, and a separate guard asserts the measurement
         stays backed by a real backend. Confirm rather than assume.

    BUILD REGARDLESS, because it is useful the moment a corpus exists:
      * A manifest schema + checksum file for a real-voice corpus, mirroring the
        existing meeting_eval manifest exactly, with an EMPTY manifest committed.
      * The eval arm that consumes it, skipping with a NAMED reason when the
        corpus is absent — never a silent skip, which is how an unmeasured
        number gets reported as measured.
      * A report that prints DER/EER per corpus and refuses to print a single
        blended number across a synthetic and a real corpus. Blending them is
        how the synthetic ceiling would disappear from the record.
      * Every published number carries its corpus name. Update the existing
        recorded numbers to say "synthetic" explicitly, which is a one-line
        honesty fix that does not need the panel.

    Depends on Y11-C.

    What NOT to do:
      - Do NOT commit voice audio to the repo.
      - Do NOT record anyone without explicit consent.
      - Do NOT report a blended number.
      - Do NOT skip the arm silently.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - HONESTY HALF ONLY, DEFERRED to after the first 100 users (not in panelApproved). Its dependency Y11-C is not on main. The half that may be built
        without a decision: every printed DER/EER and every recorded eval number carries its corpus name, and the numbers already recorded in
        docs/BUDGETS.md and meeting_eval.rs say "corpus: synthetic" explicitly. The preflight and acceptance now check that, and no longer require
        real_voice_manifest.* (the empty-manifest and named-skip mechanism belong to the consent half).
      - THE CONSENT HALF IS WILSON'S DECISION, not an agent's: whose voices (a licensed public corpus, recorded-with-consent, or both), where they live
        (never the repo), how long they are kept and what a contributor's withdrawal requires. The pre-existing note that podcast episodes 10-11 carry
        Wilson's consent covers HIS voice only, not the other speakers, so it does not settle this.
  `,
  acceptance: `
    grep -q 'corpus: synthetic' desktop/src-tauri/tests/meeting_eval.rs
    grep -qi 'synthetic' docs/BUDGETS.md
    test 0 -eq "$(find desktop/src-tauri/tests/fixtures -name '*.wav' -newer desktop/src-tauri/tests/fixtures/README.md | wc -l)"
    cd desktop && npm ci && cd src-tauri
    cargo test --features custom-protocol --test meeting_eval        ; test $? -eq 0
    cargo test --features custom-protocol --test diarization_metrics ; test $? -eq 0
  `,
})

// ── 60-yap24-packaging.mjs ────────────────────────────────────────────────
// yap24-PKG — SHIP IT WITHOUT HANDS. Written 2026-09-26 by the architecture audit
// (docs/ARCHITECTURE-AUDIT-2026-09-26.md §1.7 and §7). Runs after the Y-series packaging items
// (UPD-A/UPD-B in 30-y6). LIC-A (Stripe -> Supabase issuer) stays the licensing item; the audit
// only re-verified its premise: the revocation host (license.rs:117) no longer answers.
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap24-PKG1', prompt: 'yap24-PKG', branch: 'loop/yap24-pkg1-one-command-local-release', gated: 'panel',
  title: 'One local release command, by EXTENDING scripts/sign-local.sh: --dry-run, --check-only, DMG, notarize, staple, verify and the updater manifest through the existing stage-updater-manifest.sh',
  preflight: `
    ( if ! ( test -x scripts/qa/release-smoke.sh ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 242 (release smoke in sign-local.sh) has not landed; land it, then build PKG1"; exit 0; fi; grep -q -- '--check-only' scripts/sign-local.sh && grep -q -- '--dry-run' scripts/sign-local.sh && bash scripts/sign-local.sh --tauri-build --dry-run --check-only )
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26 (SECURITY-class: signing and notarization)
    DEPENDS: UPD-A (MERGED, 8782b4e: scripts/stage-updater-manifest.sh), UPD-B, and PR #242 (sign-local.sh --tauri-build plus the signed-app release smoke; the preflight holds this item until scripts/qa/release-smoke.sh exists)

    EVIDENCE
      - Releases are hand-run from docs/RELEASE.md; .github/workflows/release.yml is disabled
        (Actions is off account-wide — HARNESS.md "CI mode").
      - memory reference_yap_dmg_notarization: bundle_dmg.sh fails headless (Finder osascript),
        the manual hdiutil path is the reliable one; the notary keychain profile is yap-notary;
        releases must be built from a non-iCloud clone.
      - Tauri updater: signatures are mandatory and cannot be disabled; latest.json needs
        version, platforms.<target>.url and .signature (https://v2.tauri.app/plugin/updater/).

    DO: scripts/release-local.sh <version> [--dry-run] [--check-only]: fresh clone to a scratch
    dir -> npm ci -> stage both sidecars -> tauri build --bundles app -> codesign Developer ID
    with --options runtime --timestamp and Entitlements.plist -> hdiutil UDZO with /Applications
    link -> codesign the DMG -> xcrun notarytool submit --keychain-profile yap-notary --wait ->
    stapler staple -> spctl -a -t open -> .app.tar.gz + .sig + latest.json. --dry-run stops before
    notarize and prints each command; --check-only validates tools, identity presence and the
    keychain profile without building. Never prints a secret; never enables app-sandbox.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - REWRITTEN AS AN EXTENSION, NOT A SECOND SCRIPT: scripts/sign-local.sh already has --tauri-build (identity resolution, signing, verify) and #242 adds the
        signed-app release smoke. A parallel scripts/release-local.sh would be a second release path to keep honest; the acceptance fails if it exists.
        Extend sign-local.sh with: --dry-run (print each command, stop before notarize), --check-only (validate tools, identity presence and the
        yap-notary keychain profile without building), the DMG (hdiutil UDZO with an /Applications link), codesign of the DMG, notarytool submit
        --keychain-profile yap-notary --wait, stapler staple, spctl -a -t open, and the updater artifacts. latest.json comes from the EXISTING
        scripts/stage-updater-manifest.sh <tag> <stage-dir> (UPD-A): reuse it, do not re-implement it.
      - ONE ARGUMENT SHAPE: the preflight and the acceptance both run exactly bash scripts/sign-local.sh --tauri-build --dry-run --check-only (the old pair disagreed: no
        version in one, 0.0.0-ci in the other). The preflight guards with a grep for the new flags FIRST, because on today's main sign-local.sh
        forwards unknown arguments to npx tauri build, and an unguarded preflight would start a real build.
      - THE DOC is docs/RELEASING.md (the permission-identity runbook that exists), not docs/RELEASE.md; the acceptance greps it for the command.
      - Why it is top money: no notarized DMG on launch day means $0. Sign inside-out (sidecars first, no entitlements, then the app without --deep).
  `,
  acceptance: `
    test -x scripts/sign-local.sh
    bash -n scripts/sign-local.sh
    grep -q -- '--check-only' scripts/sign-local.sh
    grep -q -- '--dry-run' scripts/sign-local.sh
    bash scripts/sign-local.sh --tauri-build --dry-run --check-only
    grep -q "keychain-profile yap-notary" scripts/sign-local.sh
    grep -q 'stage-updater-manifest.sh' scripts/sign-local.sh
    grep -q "yap-polish" scripts/sign-local.sh
    grep -q "yap-diarize" scripts/sign-local.sh
    grep -q 'sign-local.sh --tauri-build' docs/RELEASING.md
    ! test -e scripts/release-local.sh
  `,
})

// Panel revisions 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, HIGH, GROUNDED): the DO text
// above already specifies --options runtime --timestamp and signing the app before the DMG,
// which is correct — but tauri.conf.json's signingIdentity is null, so a plain "tauri build"
// leaves the externalBin sidecars (yap-polish, yap-diarize) UNSIGNED, and notarization then
// rejects them (or, signed with --deep, they wrongly inherit the app's JIT entitlements). Sign
// INSIDE-OUT: each Contents/MacOS/yap-polish-* and yap-diarize-* binary first, with
// --options runtime --timestamp and NO entitlements, then the .app itself WITHOUT --deep. The
// acceptance above now runs the script's own --dry-run --check-only and greps for both sidecar
// names, so a plan that skips them fails the gate instead of only failing notarization later.
// Reword the audit's D27 to: keep manual notarized builds (product ledger, non-negotiable);
// PKG1 is a hand-invoked local script, never CI, never automatic.

ITEMS.push({
  id: 'yap24-PKG2', prompt: 'yap24-PKG', branch: 'loop/yap24-pkg2-first-run-without-a-network', gated: 'panel',
  title: 'A first launch on ANY network gets an honest wait: one download with size, progress and an ETA, a plain offline sentence, and the first dictation the moment the model lands. No bundled second model.',
  preflight: `
    grep -q "fn first_run_offline_" desktop/src-tauri/src/models.rs
    grep -q 'etaLabel' desktop/src/ModelSetup.tsx
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26. DECIDED: option B (no bundle). Option A (bundle whisper-tiny) is DEFERRED to after the first 100 users, see the last paragraph for the evidence.
    DEPENDS: Y6-A (onboarding ends in one pasted dictation)

    EVIDENCE
      - models.rs:134 "NOTHING SHIPS IN THE DMG": every model (ASR, polish, diarize, Silero VAD
        at vad.rs:491) is fetched on first use from huggingface.co (HEAD 200 on 2026-09-26 for the
        pinned parakeet, Qwen polish and yap-diarize-models revisions) or github.com, each
        sha256-verified against catalog.json / vad.rs constants.
      - A first launch on a plane (Wispr "cannot work on a plane" is Yap's own positioning line)
        cannot dictate at all until a download succeeds.

    DO: option A (default if the panel says yes) — bundle whisper-tiny-Q8_0 as a resource,
    sha256-verified on first launch, used only until the recommended model finishes downloading;
    option B — no bundle, and the onboarding model step detects offline (one HEAD with a 5 s
    timeout) and says "Yap needs one download (N MB) before it can transcribe; connect once."
    Either way a test covers the offline branch.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - DUEL, seat B (bundle whisper-tiny, option A) vs seat A (detect offline, one sentence, option B). WINNER: B, with seat B's reframing kept. Evidence:
        (1) catalog.json: parakeet Q8_0 is 731,357,568 bytes, whisper-tiny Q8_0 is 45,981,088 bytes. True, but (2) the 731 MB download is ALREADY
        deferred as far as it can go: Onboarding.tsx:157 mounts useModelSetup({ autoDownload: true }), which starts the recommended download the
        moment the overlay appears, in the background, while the user walks the permission steps; nothing blocks on it until the final dictation
        step (YV54 makes the button wait with its reason). At 100 Mbps that is about a minute, hidden behind the permission steps; at 25 Mbps
        about 4 minutes. So option A buys seconds on a typical network, not minutes. (3) The pre-mortem risk is real: the FIRST dictation of
        every new user would come from whisper-tiny (multilingual, much worse than parakeet), and the first review reads "worse than Apple
        dictation". That is the highest-stakes sample Yap ever gets. (4) The 8 GB budget does NOT decide it (tiny adds about 60 MB, DMG +46 MB). (5) A
        bundled model needs a second engine path, sha verification, switch-over, and an asr_model guard to keep tiny out of meetings; the
        guards would be gates, but the cost buys nothing on the common path. Seat B's real insight stays: the problem is ANY slow network, not only "no
        network". So:
      - CHANGE: the model step shows size, percent and a plain ETA (pure function etaLabel in ModelSetup.tsx, vitest over it), says "Yap needs one
        download (N MB) before it can transcribe; it has started" while waiting, and ONE HEAD with a 5 s timeout detects offline and says "Connect once,
        Yap finishes the download and starts working" (never an error, the download resumes on reconnect). Rust tests first_run_offline_* cover the
        offline branch; the acceptance requires at least one of them to RUN.
      - GATES: the whisper-tiny resource must NOT appear in tauri.conf.json (option A cannot be smuggled in). Meetings stay pinned to the recommended
        model via NT2 R4 (asr_model); a meeting recorded before the download lands waits in waiting_for_model (NT2 R3).
      - REGEX BUG FIXED: the old acceptance used grep -c '0 passed', which matches "10 passed" and "20 passed" and so FAILED a green run, and it
        PASSED a run with failures. It is now: ... 2>&1 | grep -qE 'test result: ok\. [1-9][0-9]* passed'.
      - DEFERRED (after the first 100 users): option A, bundling whisper-tiny for dictation-only first use, with the guards written as gates (dictation
        only, never meetings, asr_model stored, pill says quick model now). Revisit if the install funnel shows drop-off at the model step.
  `,
  acceptance: `
    grep -q "fn first_run_offline_" desktop/src-tauri/src/models.rs
    grep -q 'etaLabel' desktop/src/ModelSetup.tsx
    ! grep -rn 'whisper-tiny' desktop/src-tauri/tauri.conf.json
    cd desktop && npm ci
    npx vitest run src/ModelSetup
    cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib models::tests::first_run_offline_ 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\\. [1-9][0-9]* passed'
    cargo clippy --all-targets --features custom-protocol
  `,
})

// Panel revision 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, LOW, GROUNDED): the
// pre-flight/acceptance named no test, so cargo test --lib models passed on a branch containing
// only the first_run_offline IDENTIFIER with zero matching tests (an unmatched cargo filter
// exits 0 and prints "0 passed"). The acceptance above now filters on the concrete test-name
// prefix and fails when nothing matched.

// ── 65-yap24-expansions.mjs ───────────────────────────────────────────────
// yap24-X — EXPANSIONS: the things that make Yap better than Wispr Flow rather than equal to it.
// Written 2026-09-26 by the architecture audit (docs/ARCHITECTURE-AUDIT-2026-09-26.md §7). Wispr is
// a cloud write-buffer that "literally cannot work on a plane" (Notes/Wispr-Full-Parity-Research-
// 2026-08-09.md); Yap's moat is local. Each item keeps every byte on the Mac.
// All gated on the panel; none starts before the notetaker chain (01) closes.
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap24-X1', prompt: 'yap24-X', branch: 'loop/yap24-x1-notes-mirror-to-a-markdown-folder', gated: 'panel',
  title: 'Your notes outlive the app: every meeting and (optionally) every dictation mirrors to Markdown in a folder you choose',
  preflight: `
    grep -q "notes_mirror_dir" desktop/src-tauri/src/lib.rs
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26, DEFERRED to the next release (not in panelApproved).
    DEPENDS: yap24-NT4 (notes exist), yap24-NT9

    EVIDENCE: Wilson 2026-08-09 (memory project_yap_build_state): "full transcript/file management
    layer so AIs can always retrieve transcripts even if Yap breaks". Today the only copy is the
    SQLite DB under Application Support.

    DO: Settings -> Notes folder (default off). When set, each completed meeting writes
    <folder>/Yap Meetings/<date> <title>.md (the NT4 export) and re-writes it when notes change;
    an optional daily dictation log <folder>/Yap Dictations/<date>.md. Atomic writes; never
    deletes a user file; a missing folder pauses the mirror with one sentence in Settings.
    Obsidian-friendly front matter (date, duration, kind, attendees if known).

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - DUEL, seat B (DEFER next release) vs seat A (rescope onto #250's Markdown export). WINNER: both, and not by averaging: the scope is seat A's, the
        timing is seat B's. Evidence: this item builds on the "NT4 export", but the notes export it mirrors is PR #250's (Markdown sections in
        meetings.summary; not on main yet, 11 tests red at audit time), so it cannot be built today; and a mirror folder is a nicety for a launch
        with zero users beyond Wilson (a manual Markdown export already exists).
      - RESCOPED: DEPENDS is now PR #250's stable Markdown export (export_meeting_markdown plus the summary sections), NOT yap24-NT4 rows (NT4 no
        longer creates rows) and not NT9. Mirror writes exactly what that export produces. DEFER: next release.
  `,
  acceptance: `
    cd desktop && npm ci && cd src-tauri
    cargo test --features custom-protocol --lib notes_mirror
    cargo clippy --all-targets --features custom-protocol
  `,
})

// yap24-X2 — CUT 2026-10-26 by the Senior Panel plan-audit (object removed; the harness has no retired/status field, removal is the mechanism).
// Why: it duplicates the "Set up MCP" row of the ungated yap-notetaker-onboard (a stdio MCP server over read-only SQLite), and it contradicts that item's
// rule that dictation history is never exposed. MCP as a whole is DEFERRED to after the first 100 users: the yap-notetaker-onboard MCP step was struck
// today (the row shows as Coming), so there is still exactly ONE owner of the MCP deliverable, and it is not built in v1. Spec is in git history.

ITEMS.push({
  id: 'yap24-X3', prompt: 'yap24-X', branch: 'loop/yap24-x3-ask-your-meeting-locally', gated: 'panel',
  title: 'Ask your meeting: "what did I miss / what did we decide" answered by the local model, every answer citing its transcript lines',
  preflight: `
    test -f desktop/src-tauri/tests/meeting_ask_cites_segments.rs
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26, DEFERRED to after the first 100 users (not in panelApproved).
    DEPENDS: yap24-NT4, yap24-NT6

    EVIDENCE: Wispr's 2026 notetaker ships "What did I miss" and a notetaker chat (parity note
    §2.7, lines 214-215) — cloud. Yap has the yap-polish sidecar and GBNF-constrained output
    (summarize.rs) already.

    DO: an Ask box on a meeting; retrieval = FTS5 over that meeting's segments (no embeddings in
    v1); the sidecar answers with JSON {answer, citations:[segment ids]}; an answer without a
    valid citation is refused and shown as "not in this meeting". Runs on the summary worker,
    never concurrently with dictation.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - DEFER: after the first 100 users. Wispr's "What did I miss" is cloud; here a 1.5B model on an 8 GB floor machine is conditional on serialization
        (see NT4 R4). DEPENDS: yap24-NT4 (rescoped: the measured notes eval and RSS ceiling) and the #250 notes; no rows exist to retrieve from until then.
  `,
  acceptance: `
    cd desktop && npm ci && cd src-tauri
    cargo test --features custom-protocol --test meeting_ask_cites_segments
    cargo clippy --all-targets --features custom-protocol
  `,
})

// ── 99-yap-fmt-sweep.mjs ──────────────────────────────────────────────────
// yap-fmt-sweep-2026-10-26 — written 2026-10-26 by the pre-launch fix seat after review pass w7sv7tozp was
// stopped. Named 99 so it sorts LAST and builds last: a tree-wide `cargo fmt --all` conflicts every open
// branch that edits the large Rust files, so it must run only after every other open PR has landed.
// Panel: not required (trivial, mechanical, format-only). Class STANDARD.
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap-fmt-sweep-2026-10-26', prompt: 'yap-fmt-sweep-2026-10-26', branch: 'loop/yap-fmt-sweep-2026-10-26', gated: null,
  title: 'Format-only sweep: `cargo fmt --all` on main, zero semantic change; then rustfmt goes back to BLOCKING in the land gate, loop-smoke and CI',
  preflight: `
    (cd desktop/src-tauri && cargo fmt --all -- --check) && ! grep -q 'fmt IS ADVISORY' scripts/loop/template.mjs
  `,
  spec: `
    Panel: not required — trivial, mechanical. Class STANDARD. deps: none.
    ORDER: build only after every other open PR has landed; the loop runs it last by file order (99-). If any
    loop-build PR is still OPEN when you reach this item, stop and return status "needs-human" with the open
    PR numbers — a sweep under open branches conflicts all of them.

    WHY (2026-10-26): the b4b8d06 sweep (152 hunks / 21 files) drifted; on a740614 \`cargo fmt --all -- --check\`
    exits 1 with 65 hunks in src-tauri/src/{db,hotkey_table,hotkeys,lib,perm_truth,ptt_macos,record}.rs and
    tests/{db_migration_idempotent,hotkey_no_tcc,matrix_row1_tap_permission_denied}.rs. Requiring it in the
    LAND gate turned every PASS/PASS PR ci-red (review pass w7sv7tozp), so the gate made fmt ADVISORY per the
    Y0-A ledger (PR #234 ci.yml). This item restores it.

    DO
      1. Commit 1, format only: cd desktop/src-tauri && cargo fmt --all. Prove zero semantic change:
         \`cargo fmt --all -- --check\` exits 0; \`git diff --stat\` touches only .rs files; \`git diff -w
         --ignore-blank-lines\` is empty or only re-wrapped lines (paste the stat); cargo build and clippy
         produce the same result as on the base sha (paste both exit codes, before and after); the full gate's
         tests pass. Append the commit sha to .git-blame-ignore-revs in commit 2.
      2. Commit 2, flip fmt back to blocking, all three places, same PR:
         - scripts/loop/template.mjs GATE_CMDS: fmt rejoins the gate ("ALL ELEVEN MUST EXIT 0"), the
           ADVISORY wording, the fmtHunks line, the "fmt: advisory" gate-comment line and the "(fmt is
           advisory and excluded)" / "a non-zero fmt ALONE is NEVER ci-red" land-outcome clauses come out.
         - scripts/loop-smoke.sh stage_fmt: blocking by default again (drop YAP_SMOKE_FMT_STRICT or default it to 1).
         - .github/workflows/ci.yml: the rustfmt step loses \`continue-on-error\` (the Y0-A comment says this
           sweep is the trigger).
         - scripts/loop/land-gate-fmt.test.mjs: invert it to assert fmt is BLOCKING (keep the clippy assertions).
         Then npm run loop:build && npm run loop:validate in desktop/, both exit 0, and commit the regenerated parts.
    WHAT NOT TO DO: no code change of any kind in commit 1; no rustfmt.toml change; no #[rustfmt::skip].
  `,
  acceptance: `
    (cd desktop/src-tauri && cargo fmt --all -- --check)
    ! grep -q 'fmt IS ADVISORY' scripts/loop/template.mjs
    grep -q 'ALL ELEVEN MUST EXIT 0' scripts/loop/template.mjs
    ! sed -n '/rustfmt/,/run: cargo fmt/p' .github/workflows/ci.yml | grep -q 'continue-on-error: true'
    node --test scripts/loop/land-gate-fmt.test.mjs
    (cd desktop && npm run loop:validate)
    (cd desktop && npm ci && npx tsc --noEmit && npm test)
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
