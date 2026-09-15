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
 *     the __LANES__ marker -> item id -> lane index, round-robin over the SOURCE ITEM FILES.
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
 *   'review' — the SAME script run again afterwards with args {mode:'review'}, and ONLY part-01
 *              runs it: the pass is PR-DRIVEN. One TRIAGE agent turns the open loop-build PRs into
 *              an ordered queue (closing duplicates and superseded work first), then TWO REVIEW
 *              CHAINS consume that shared queue under a HARD CAP OF THREE CONCURRENT AGENTS.
 */

export const meta = {
  name: "yap-overhaul-all-part-02",
  description:
    "Part 02 of the Yap (wilson-voice) overhaul, two builder lanes: land whatever is already gate-green, pre-flight on unmodified main (already-done items are skipped), build, open a labelled PR. The adversarial review, the independent second-Opus gate and the merge bar run afterwards in the same script with args {mode:'review'}. 25 items (Y8-A..Y11-F), 1 awaiting the Senior Panel. The gate is local (Actions is disabled by the account spending limit); the DMG is not in it.",
  phases: [
    { title: "Recon", detail: "two lane worktrees, one npm ci + one warm cargo build each, the loop-build label, ci-mode measured" },
    { title: "Y8-A", detail: "The hotkey suite: hands-free, cancel, copy-last, paste-last, scratchpad, with real validation rules" },
    { title: "DB-D", detail: "Scratchpad: a second window on a hotkey, dictate-into-note, versions — the half-built feature finished" },
    { title: "Y8-B", detail: "The pill becomes a bar: five affordance slots, each with a tooltip and a vertical-dock layout" },
    { title: "Y8-C", detail: "Optional earcons for start, stop, paste and achievement — off by default, Yappy-voiced" },
    { title: "Y8-D", detail: "Coaching nudges: the bar teaches the app, in Yappy's voice, without becoming nagware" },
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
/**
 * THE TWO LANES. Both are worktrees of LOCAL_REPO, both are created in Recon, both are removed by
 * the LAST part's Drain. Two builders cannot share one checkout, one cargo target dir or one
 * preview port, which is the whole reason there are two. They are written as literal
 * single-quoted consts because build.mjs scrapes exactly that shape to give the item files their
 * ${WORKDIR} / ${PREVIEW_PORT} / ${CARGO_TARGET_A} values.
 */
const WORKDIRS = [WORKDIR, WORKDIR_B]
/**
 * CARGO TARGET DIRS LIVE OUTSIDE THE WORKTREES, ON PURPOSE. src-tauri statically links
 * transcribe-cpp's ggml and yap-polish vendors llama-cpp-sys-2: a COLD build of this workspace is
 * minutes, a warm one is well under one. A target dir inside the worktree dies with the worktree,
 * so every part would pay the cold build again. These persist across items, parts and passes.
 */
const CARGO_TARGET_A = '/Users/wilsonguenther/code/wilson-voice-loop/target-a'
const CARGO_TARGET_B = '/Users/wilsonguenther/code/wilson-voice-loop/target-b'
const CARGO_TARGETS = [CARGO_TARGET_A, CARGO_TARGET_B]
const LOG = '/Users/wilsonguenther/Obsidian/Wilson-Brain/Projects/Loop-Logs/2026-09-12-yap-part02.md'
const TELEMETRY = '/Users/wilsonguenther/Obsidian/Wilson-Brain/Projects/Forge-CICD-Telemetry.md'
const SPEC_SOURCES = 'the repo docs (PRODUCT.md, ROADMAP.md, ARCHITECTURE.md, docs/) and the item spec below'
const PREVIEW_PORT = '5273' // lane A's vite preview / local frontend server
const PREVIEW_PORT_B = '5274' // lane B's
const PREVIEW_PORTS = [PREVIEW_PORT, PREVIEW_PORT_B]
const BOTH_DIRS = `${WORKDIR} and ${WORKDIR_B}`
const LANE_TAGS = ['A', 'B']
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
  'desktop/src-tauri/capabilities/, desktop/src-tauri/tauri.conf.json, the updater/signing path, desktop/src-tauri/src/db or any migration, the polish_protocol/sidecar IPC, or any path or diff hunk matching updater|sign|notariz|keychain|permission|accessibility|microphone|network|http|upload|telemetry'
const SECURITY_IDS = 'SEC-|UPD-|PRIV-|DB-|PERM-'
/**
 * Stamped by build.mjs: true for every part of a multi-part run EXCEPT the last. Parts share the
 * worktrees, their node_modules and their cargo target dirs, so only the LAST part's Drain is
 * allowed to remove them.
 */
const KEEP_WORKTREE = false
/** Stamped by build.mjs like LOG: which part this is. The status board is keyed on it. */
const PART = 'part-02'
/**
 * Stamped by build.mjs: item id -> lane index, round-robin over the SOURCE ITEM FILES so that a
 * whole prompt group (whose items often depend on one another) stays sequential on one lane.
 */
const LANE_BY_ID = {"Y8-A":0,"DB-D":0,"Y8-B":0,"Y8-C":0,"Y8-D":0,"Y9-A":1,"Y9-B":1,"Y9-C":1,"PERM-F":1,"PERM-G":1,"Y9-D":1,"Y9-E":1,"Y10-A":0,"PERM-H":0,"Y10-B":0,"Y10-D":0,"Y10-E":0,"Y10-F":0,"Y11-A":1,"Y11-B":1,"DB-E":1,"Y11-C":1,"Y11-D":1,"Y11-E":1,"Y11-F":1}
const laneOf = (item) => (LANE_BY_ID[item.id] === 1 ? 1 : 0)
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
THE WORK DIR IS A SHARED WORKTREE, NOT YOUR CLONE — AND THERE ARE TWO OF THEM.
This loop runs TWO LANES side by side. YOUR LANE IS:
    WORKDIR           ${dir}
    APP DIRECTORY     ${dir}/${APP}          <- every build command runs HERE, never at the root
    CARGO_TARGET_DIR  ${target}
    PREVIEW PORT      ${port}
The other lane (${other}) belongs to a DIFFERENT item that another agent is building RIGHT NOW.
Never cd into it, never check out a branch in it, never kill a server on its port, never remove it.

EXPORT THE TARGET DIR IN EVERY SHELL THAT RUNS CARGO:
    export CARGO_TARGET_DIR=${target}
It lives outside the worktree so the vendored-ggml build cache survives every item and every part.
Forgetting it means a COLD build inside the worktree: minutes you did not need to spend, and a
7.5 GB directory that dies with the worktree.

IGNORE ANY OTHER WORKDIR, TARGET DIR OR PORT NAMED BELOW THIS LINE. Item text may hard-code
${WORKDIR}, ${CARGO_TARGET_A} and port ${PREVIEW_PORT}; wherever it does, read ${dir}, ${target}
and ${port}. This substitution is not optional: two lanes on one port or one target dir is two
builds fighting over one lock, and the evidence you gathered would be the other item's.

Both lanes are worktrees of ${LOCAL_REPO}, created once in Recon with one 'npm ci' and one warm
cargo build each, and every item reuses them. Therefore:
- Start in YOUR lane: cd ${dir} && git fetch origin && git checkout -B <branch> origin/main
- Do not 'rm -rf' it, do not re-clone it, and do not run 'npm ci' again unless
  ${APP}/package-lock.json changed on main (say so if you do).
- Do not create additional worktrees or clones. Disk hygiene is a hard rule: a previous loop left
  26 clones / 78 GB behind on another repo.
- Leave the branch pushed. Drain removes BOTH worktrees at the end of the run.
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
          cd ${dir}/${APP} && export CARGO_TARGET_DIR=<your lane's target dir> && npm run desktop:dev
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
 * fmt IS A GATE CONJUNCT NOW. The tree was swept (`cargo fmt --all`, 152 hunks / 21 files, listed
 * in .git-blame-ignore-revs) and `cargo fmt --all -- --check` exits 0 on main as of that commit.
 * clippy is NOT run with `-- -D warnings`: a mechanical `cargo clippy --fix` sweep landed, and 16
 * lints survive it that need real refactors (very-complex-type, too-many-arguments, clamp-like
 * pattern, const assertions in tests). See PLAN.md's Panel revisions section.
 */
const GATE_CMDS = (dir, target) => `
    cd ${dir}/${APP} && export CARGO_TARGET_DIR=${target}
    npx tsc --noEmit ; echo "typecheck=$?"
    npm test ; echo "test=$?"
    npm run build ; echo "build=$?"
    cargo build -p yap-polish --release ; echo "polish=$?"
    cargo build -p yap-diarize --release ; echo "diarize=$?"
    TRIPLE="$(rustc -vV | awk '/^host:/ {print $2}')" ; mkdir -p src-tauri/binaries && cp ${target}/release/yap-polish "src-tauri/binaries/yap-polish-$TRIPLE" && cp ${target}/release/yap-diarize "src-tauri/binaries/yap-diarize-$TRIPLE" ; echo "stage=$?"
    cargo test -p yap-polish --release ; echo "polishtest=$?"
    cargo test -p yap-diarize --release ; echo "diarizetest=$?"
    cargo fmt --all -- --check ; echo "fmt=$?"
    cd ${dir}/${APP}/src-tauri && cargo clippy --all-targets --features custom-protocol ; echo "clippy=$?"
    cd ${dir}/${APP}/src-tauri && cargo test --features custom-protocol ; echo "cargotest=$?"
ALL ELEVEN MUST EXIT 0. Run them SEPARATELY and read every code BARE, never through a pipe.
BOTH SIDECARS ARE BUILT AND BOTH ARE STAGED, and that step comes BEFORE any cargo command against
the app: bundle.externalBin makes binaries/yap-polish-<triple> AND binaries/yap-diarize-<triple>
preconditions of every cargo build of wilson-voice. If a cargo command against the app dies with
"resource path ... doesn't exist", you skipped the staging step — that is an ENVIRONMENT failure,
never a code defect, and never a reason to edit tauri.conf.json.
clippy is deliberately WITHOUT '-- -D warnings' (16 known lints on main need real refactors, which
is a loop item, not a gate). Do not add the flag, and do not silence lints to make it green.
NOT IN THIS GATE: 'npm run desktop:build' (the DMG). It is minutes long and belongs to the final
smoke item. Do not run it here and do not claim it.
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

A PR IS LANDABLE ONLY WHEN YOU — THIS AGENT, IN THIS WORKTREE, ON THIS PR'S REBASED HEAD — HAVE
RUN THE WHOLE GATE YOURSELF AND EVERY COMMAND EXITED 0:
${GATE_CMDS(dir, target)}
SOMEONE ELSE'S GATE IS NOT YOUR GATE. A gate pasted by another agent, or on an earlier sha, or in
an earlier section of this run, is evidence about a tree that is not the one you are about to
merge. If you did not run those commands yourself on the current head, you have no gate.

NEVER MERGE ON A STALE HEAD. Bind the gate to a sha:
    cd ${dir} && git rev-parse HEAD                       # the sha your commands measured
    gh pr view -R ${REPO} <n> --json headRefOid,mergeable,mergeStateStatus
headRefOid at merge time MUST equal that sha. If you rebase, push, amend or add so much as a
comment-only commit after the gate, THE GATE IS VOID — run it all again on the new head. GitHub
must also still report mergeable == MERGEABLE (not CONFLICTING, not UNKNOWN).

THEN PASTE THE GATE ON THE PR, AS A COMMENT TITLED EXACTLY "${LOCAL_GATE_TITLE}":
    gh pr comment -R ${REPO} <n> --body "## ${LOCAL_GATE_TITLE}
ci=local — GitHub Actions cannot schedule jobs for this account, so this PR was gated locally.
gated sha: <the headRefOid you measured>
worktree:  ${dir}

typecheck=<code>
test=<code>
build=<code>
polish=<code>
diarize=<code>
stage=<code>
polishtest=<code>
diarizetest=<code>
fmt=<code>
clippy=<code>
cargotest=<code>

<the raw tail of each command's output — real pasted output, never prose>"
Post that comment BEFORE you merge. With no CI check to point at, this comment is the only
auditable record that the merge had a gate at all.

THEN MERGE:
    gh pr merge -R ${REPO} <n> --squash --delete-branch --admin
--delete-branch is not optional: an undeleted branch is the stale-branch backlog this loop's Drain
then has to sweep. Use --admin so a self-opened PR with no approving review can land.
Every merge comment and every status-board row you write carries ci=local.

OUTCOMES IN THIS MODE:
  - every command 0, sha matches, MERGEABLE -> merge. status "merged", ciCheckConclusion
    "local-gate", and localGate: {sha, typecheck, test, build, polish, diarize, stage, polishtest,
    diarizetest, fmt, clippy, cargotest}.
  - any command non-zero  -> this is a red gate. Fix it if this PR is yours to fix; otherwise label
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
  - that job == success AND mergeable == MERGEABLE AND every blocking finding disposed
        gh pr merge -R ${REPO} <n> --squash --delete-branch --admin
    status "merged" with the squash sha; the board row carries ci=github.
  - it is queued / in_progress / not yet created -> status "ci-pending", reason "ci-pending".
    DO NOT POLL, DO NOT SLEEP, DO NOT WAIT — the land sweeper settles it.
  - it == failure -> label ci-red and comment the FAILING STEP NAME. status "ci-red".
  - mergeable == CONFLICTING after your rebase -> label needs-rebase, status "needs-rebase".
NOTE: even in this mode the CI workflow does NOT run clippy or fmt as a gate (both are '|| true'),
and it does not build or stage yap-diarize at all, so a github-mode merge still owes the local
clippy run, the local fmt check and both sidecar builds. Run them and paste them.
`

// ── verdict helpers (string-safe; agent returns may be objects or strings) ──
const asText = (v) => (typeof v === 'string' ? v : JSON.stringify(v ?? null))
const saysBlock = (v) => /"verdict"\s*:\s*"BLOCK"/i.test(asText(v))
const saysReviewed = (v) => /"status"\s*:\s*"reviewed"/i.test(asText(v))
const hasEmptyBlocking = (v) => /"blocking"\s*:\s*\[\s*\]/.test(asText(v))
/** A BLOCK verdict with an empty blocking[] is malformed → recorded as PASS + an advisory note. */
const isMalformedBlock = (v) => saysBlock(v) && hasEmptyBlocking(v)
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
async function agentR(prompt, opts) {
  let r = null
  for (let attempt = 1; attempt <= 3; attempt++) {
    r = await agent(prompt, opts)
    if (r !== null) return r
    log(`${(opts && opts.label) || 'agent'}: attempt ${attempt}/3 returned null (API failure) — ${attempt < 3 ? 'retrying' : 'giving up; halt follows'}`)
  }
  return r
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
    cd ${dir}/${APP} && export CARGO_TARGET_DIR=${target}
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
  const other = WORKDIRS[lane === 1 ? 0 : 1]
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
  if (runPassed(pre)) {
    log(`${p} (lane ${tag}): already-done — EXECUTED pre-flight passed on unmodified origin/main; no builder dispatched, no branch, no PR`)
    return { itemId: item.id, status: 'already-done', lane: tag, preflight: pre }
  }
  log(`${p} (lane ${tag}): pre-flight FAILED at ${(pre && pre.firstFailure) || 'a command it named'} — this item is real work; dispatching the builder`)

  const build = await agentR(
    `You are a SENIOR RUST + TAURI v2 + TYPESCRIPT/REACT ENGINEER working on Yap, a macOS dictation
app. Build exactly one item. No scope creep.

${WORKTREE_CONTRACT(dir, port, target, other)}

# ═══ LAND FIRST — MERGE WHAT IS ALREADY GREEN (${CI_MODE === 'local' ? '≤ 14 TOOL CALLS, ONE PR' : '≤ 6 TOOL CALLS'}, THEN MOVE ON) ═══
Nothing else in this pass merges anything. If you skip this, the PRs pile up and every item after
you branches off a main that is missing its predecessors — which is how a loop produces fifteen
conflicting PRs and zero shipped work.

    gh pr list -R ${REPO} --state open --label ${LOOP_LABEL} --json number,headRefName,mergeable,statusCheckRollup

MERGE every PR in that list that clears the gate below, and NOTHING else. The gate depends on
whether GitHub Actions can run at all in this run; Recon has already decided, and this is it:
${CI_GATE(dir, target)}
SKIP — do not force, do not fix, do not investigate — anything pending, failing or conflicting.
NAME each one you skipped and why, in one line each. A conflicting PR is the review pass's problem,
not yours; you have an item to build.
THIS IS A BUDGET, NOT AN INVITATION: ${CI_MODE === 'local' ? 'at most 14 tool calls, and at most ONE PR gated and landed — the local gate is eight separate command runs against one worktree, so gate and land the OLDEST eligible PR, name the ones you left, and move on. The review pass settles the rest; do not try to clear the queue here' : 'at most 6 tool calls'}. Do not read diffs, do not
review, do not chase a red check. These PRs are UNREVIEWED by design and merging them gate-green
only is a decision Wilson has already made for this pass.
DO NOT TOUCH the pre-existing feat/yv1xx PRs from the previous loop unless they carry the
${LOOP_LABEL} label. They are weeks old, they are not this pass's work, and Drain triages them.

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
DO NOT WAIT FOR CI. Push, open the PR, write your log line and your board row, and return. A later
item's LAND step merges this PR once it is gate-green. Sitting on 'gh run watch' is the wait this
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
- an explicit "What I did NOT do" section listing what a reader might assume you did and you did not

RETURN: {itemId, status: "built", branch, prNumber, resumedPr,
         landed: [{pr, sha}], landSkipped: [{pr, why}], openLoopPrs,
         gateExits: {typecheck, test, build, polish, diarize, stage, polishtest, diarizetest, fmt,
                     clippy, cargotest},
         runtimeProof: "test"|"app"|"probe"|"none", acceptanceOutput, notDone: [...]}
${LOGLINE(`${p} build`, p, `${tag}:build`, '--open <the open loop-build PR count you just observed>')}
${GUARD(dir)}`,
    { model: 'opus', effort: 'high', phase: p, label: `build:${tag}:${p}` }
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
    { model: 'opus', effort: 'high', phase: p, label: `fix:${tag}:${p}` }
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

    gh pr edit -R ${REPO} <n> --add-label needs-human
    gh pr comment -R ${REPO} <n> --body "<the comment below, verbatim, with the output pasted in>"
    gh pr edit -R ${REPO} <n> --body "<the existing body, with this same block appended at the TOP>"

THE COMMENT, verbatim, and the raw output goes in an indented block (four spaces per line — do not
use a markdown fence, the output may contain one):

## Acceptance FAILED — executed twice by the loop, not merged
This PR is NOT mergeable by the loop and nothing in it has been reviewed. The item's acceptance
commands were run verbatim on this branch by a command-runner seat, once after the build and once
after one fix round, and they did not all exit 0.
first failure: ${(accept && accept.firstFailure) || 'see the output below'}
acceptance output:
<the output, every line indented by four spaces>

Then the board, and it must say failed:
    node ${STATUS} ${p} ${tag}:failed "acceptance failed twice; PR open, needs-human, ci=${CI_MODE}" <n>
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
 * in the REVIEW-STAGE barrier). (b) fix+land — one seat: rebase FIRST, fix every blocking finding or
 * refute it with evidence, push, post the verdicts, and merge if the gate is green. It NEVER waits
 * for CI. Exactly ONE fix round per PR in this pass.
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

  const primaryPrompt = `You are the PRIMARY ADVERSARIAL REVIEWER of PR #${pr} on ${REPO} (${itemId}, classified ${klass}).
Yap is a macOS dictation app: Rust/Tauri v2 backend, a Vite/React frontend, a yap-polish sidecar.
Your job is to TRY TO DISPROVE this PR's claims. Not to falsify them — to test them honestly and
hard, and to report what survives. A finding you cannot substantiate is not a finding.

Read it: gh pr view -R ${REPO} ${pr} --json number,title,body,headRefOid,files
and gh pr diff -R ${REPO} ${pr}. You may read code in ${dir} (do NOT check out a branch there —
the fix agent owns that worktree next; read-only greps only).
TRIAGE ALREADY ESTABLISHED: ${facts}

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
is malformed; it is recorded as a PASS with an advisory note and you will NOT be re-dispatched, so
a finding you fail to write down is a finding you did not make.

RETURN: {verdict: "PASS"|"BLOCK", blocking: [...], advisory: [...], reachabilityProofRun: true|false,
         evidenceClass: "test"|"app"|"probe"|"compiles-only"|"none", supersededByMain: true|false}
${LOGLINE(`${itemId} review-opus`, itemId, 'review:review-r1', `${pr}`)}
${GUARD(dir)}`

  const lensPrompt = `You are the INDEPENDENT ADVERSARIAL REVIEWER of PR #${pr} on ${REPO} (${itemId}) — a second
reviewer with a DIFFERENT LENS, seated because triage classified this PR SECURITY.
A primary adversarial reviewer is reviewing it right now. ITS VERDICT IS HIDDEN FROM YOU BY DESIGN.
Do not ask for it, do not guess it, do not try to agree with it. Two reviewers that share a premise
are one reviewer: this harness has already shipped a dead-code fix that TWO independent reviewers
both approved, because both accepted the same false premise. You exist to break that.

Read it: gh pr view -R ${REPO} ${pr} --json number,title,body,headRefOid,files
and gh pr diff -R ${REPO} ${pr}. Read-only greps in ${dir}; never check out a branch there.
TRIAGE ALREADY ESTABLISHED: ${facts}

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

If you cannot read the PR at all (gh fails, the PR is missing), return status "unavailable" with
verdict "PASS" — NEVER BLOCK on your own inability to review. "unavailable" never blocks a merge.
A BLOCK with an empty blocking[] is malformed and is recorded as a PASS; you will not be re-dispatched.

RETURN: {status: "reviewed"|"unavailable", verdict: "PASS"|"BLOCK", blocking: [...], advisory: [...],
         reachabilityChain: "<entry -> import -> symbol, or why none exists>", failurePolarity: [...],
         dataOrPermissionRisk: "<what and for whom, or none>", whyUnavailable}
${LOGLINE(`${itemId} review-opus2`, itemId, 'review:review-r1', `${pr}`)}
${GUARD(dir)}`

  let rA = null
  let rB = null
  await withAgents(seats, async () => {
    if (seats === 2) {
      /* REVIEW-STAGE-PARALLEL */
      // Both prompts are built ABOVE, before either is dispatched, precisely so neither reviewer can
      // depend on the other's outcome. This barrier is the ONLY concurrent dispatch in the harness
      // and it costs TWO of the three seats, which is why the other chain can hold at most one.
      const both = await parallel([
        () => agentR(primaryPrompt, { model: 'opus', effort: 'high', phase: itemId, label: `review-opus:${itemId}` }),
        () => agentR(lensPrompt, { model: 'opus', effort: 'high', phase: itemId, label: `review-opus2:${itemId}` }),
      ])
      rA = both[0]
      rB = both[1]
    } else {
      rA = await agentR(primaryPrompt, { model: 'opus', effort: 'high', phase: itemId, label: `review-opus:${itemId}` })
    }
  })

  // A NULL SEAT IS UNAVAILABLE, NEVER A BLOCK. A reviewer that died produced no finding, and a
  // broken seat masquerading as a security verdict has halted a whole loop before.
  const UNAVAILABLE = { status: 'unavailable', verdict: 'PASS', blocking: [], advisory: [], whyUnavailable: 'seat returned null (API failure)' }
  if (rA === null && (seats === 1 || rB === null)) {
    noteHalt(`${itemId} PR#${pr} review`, 'every reviewer seat returned null — API failure or session/usage limit')
    return { itemId, pr, klass, status: 'halted-in-review' }
  }
  const primary = rA === null ? UNAVAILABLE : rA
  const lens = seats === 2 ? (rB === null ? UNAVAILABLE : rB) : null
  // A BLOCK with an empty blocking[] is a refusal with no content. It is NOT re-dispatched: it is
  // recorded as a PASS carrying one advisory note. Re-dispatching a malformed refusal is how a
  // previous pass spent two opus/high seats to learn nothing twice.
  const malformed = []
  if (isMalformedBlock(primary)) malformed.push('primary')
  if (lens && isMalformedBlock(lens)) malformed.push('independent-lens')
  for (const who of malformed) log(`PR#${pr} (${itemId}): the ${who} reviewer returned BLOCK with an empty blocking[] — recorded as PASS + an advisory note, NOT re-dispatched`)
  const primaryBlocked = saysBlock(primary) && !isMalformedBlock(primary)
  const lensBlocked = lens !== null && saysReviewed(lens) && saysBlock(lens) && !isMalformedBlock(lens)
  const blocked = primaryBlocked || lensBlocked
  const verdicts = { primary, lens, primaryBlocked, lensBlocked, malformed, seats }

  const landPrompt = `You are the FIX + LAND agent for PR #${pr} on ${REPO} (${itemId}, ${klass}). You are the last gate,
and you are the ONLY fix round this PR gets in this pass. Be paranoid, and be fast.

# 1 — REBASE FIRST, ALWAYS. THIS IS STEP ONE AND IT IS NOT OPTIONAL.
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

# 2 — DISPOSE OF EVERY BLOCKING FINDING. ONE ROUND ONLY.
PRIMARY REVIEWER: ${JSON.stringify(primary)}
INDEPENDENT REVIEWER (security PRs only; null means it was not seated): ${JSON.stringify(lens)}
Fix ONLY the BLOCKING findings. Do NOT address advisory findings — they are logged once and never
re-litigated. Do NOT expand scope. If you believe a blocking finding is WRONG, do not silently drop
it: post a PR comment with the evidence that refutes it. A reviewer being wrong is a legitimate
outcome; a finding quietly dropped is not.
${MERGE_BAR}
blockingDispositions is REQUIRED in your return, one entry per blocking finding raised by either
reviewer. If any blocking finding is neither fixed nor refuted: do NOT merge, leave the PR open,
    gh pr edit -R ${REPO} ${pr} --add-label needs-human
and comment exactly which finding stopped you. THERE IS NO SECOND REVIEW ROUND IN THIS PASS: a
finding you cannot dispose of ends as needs-human, never as a re-dispatch.

# 3 — THE GATE, EVERY COMMAND, SEPARATELY, ON THE REBASED HEAD
${GATE_CMDS(dir, target)}
Then git push --force-with-lease.
${VERIFY_NOTHING}

# 4 — POST BOTH VERDICTS AS REAL PR REVIEWS, BEFORE YOU MERGE
gh pr review -R ${REPO} ${pr} --comment --body "Primary adversarial review (Opus)

<verdict + every finding, BLOCKING and ADVISORY, with its disposition>"
${seats === 2 ? `gh pr review -R ${REPO} ${pr} --comment --body "Independent adversarial review (Opus, liveness/polarity/data lens)

<verdict + every finding, the reachability chain, and the per-expression fails-open/fails-closed/panics call>"` : `This is a STANDARD PR: one reviewer was seated, and that is the design. Say so in the review body — do not imply a second verdict that does not exist.`}
Post them even when every verdict is PASS. A verdict that lives only in a loop log is unauditable
from GitHub. Advisory findings go in a comment on THIS PR — never a new PR; one-PR-per-advisory is
how a loop grows its own backlog with every success.

# 5 — LAND WHEN GREEN. DO NOT WAIT FOR CI. NOT FOR ONE MINUTE.
This PR is #${pr}; wherever the gate below says <n>, that is ${pr}. Every blocking finding must be
disposed of before you even open the gate — a green gate never substitutes for the merge bar.
${CI_GATE(dir, target)}

# 6 — LEAVE THE LANE CLEAN
If you started a server on ${port} or left the app running, kill it and confirm nothing listens
there (lsof -nP -iTCP:${port} -sTCP:LISTEN). Never touch ${other} — the other review chain is in it.

RETURN: {itemId: "${itemId}", pr: ${pr || 0}, status: "merged"|"ci-pending"|"ci-red"|"needs-rebase"|"needs-human"|"closed-superseded",
         landed: true|false, reason, mergeCommit, rebased: true|false, conflictsResolved: [...],
         blockingDispositions: [{finding, disposition: "fixed"|"refuted", evidence}],
         gateExits: {typecheck, test, build, sidecar, polishtest, clippy, cargotest, fmt},
         verdictsPosted, advisoryPosted, ciCheckConclusion}
${LOGLINE(`${itemId} fix+land PR#${pr}`, itemId, 'review:fixed', `${pr}`)}
Then write the board ONE more time with the OUTCOME — stage review:merged, review:ci-pending,
review:needs-human or review:closed-superseded — and the PR number as the 4th argument.
${WORKTREE_CONTRACT(dir, port, target, other)}
${RUNTIME_PROOF(dir)}
${GUARD(dir)}`
  const land = await withAgents(1, () => agentR(landPrompt, { model: 'opus', effort: 'high', phase: itemId, label: `fix-land:${itemId}` }))

  const landText = asText(land)
  const statusOf = (re, name) => (re.test(landText) ? name : null)
  const status =
    statusOf(/"status"\s*:\s*"merged"/i, 'merged') ||
    statusOf(/"status"\s*:\s*"closed-superseded"/i, 'closed-superseded') ||
    statusOf(/"status"\s*:\s*"ci-red"/i, 'ci-red') ||
    statusOf(/"status"\s*:\s*"needs-rebase"/i, 'needs-rebase') ||
    statusOf(/"status"\s*:\s*"needs-human"/i, 'needs-human') ||
    'ci-pending'
  log(`PR#${pr} (${itemId}, ${klass}, chain ${REVIEW_TAGS[chain]}): ${blocked ? 'BLOCKED then fixed' : 'PASS'} -> ${status}`)
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
// ── 40-y8-parity-p0.mjs ───────────────────────────────────────────────────
// Y8 — THE P0 PARITY QUEUE. Six items Wilson explicitly asked for, left on the
// line when the yap23 loop stopped. Source: the Wispr parity teardown
// (docs source of record: ~/Obsidian/Wilson-Brain/Notes/Wispr-Full-Parity-Research-2026-08-09.md,
// §3 "Prioritized implementation backlog for Yap", P0 items 1-6), quoted below.
// Every claim there is tagged [BUNDLE] = read out of the shipping competitor
// bundle, which is why the numbers are exact and must be used as given.
//
// P0 #1 (vertical reflow) is NOT here — it is Y5-I, because it depends on the
// token layer and the full state machine. P0 #2 and #3 are Y5-D and Y5-E for the
// same reason. This file is the remaining three: Scratchpad, Flow-Bar affordance
// slots, and the hotkey suite. Grouped in one file because all three land on the
// pill/window surface and must not race each other.
//
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'Y8-A', prompt: 'Y8', branch: 'loop/y8-a-hotkey-suite-completion', gated: null,
  title: 'The hotkey suite: hands-free, cancel, copy-last, paste-last, scratchpad, with real validation rules',
  preflight: `
    grep -q 'pub const HANDS_FREE' desktop/src-tauri/src/shortcuts.rs
    grep -q 'pub const COPY_LAST' desktop/src-tauri/src/shortcuts.rs
    grep -q 'pub const OPEN_SCRATCHPAD' desktop/src-tauri/src/shortcuts.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test shortcut_validation
  `,
  spec: `
    P0 #6, VERBATIM: "Hotkey suite completion. (S · L) Add: hands-free toggle,
    cancel, copy-last-transcript, paste-last-transcript, open-scratchpad, and
    per-transform shortcuts. Reuse the YV15 capture control; port Wispr's
    validation rules (<=3 keys, modifier required, reject left/right duplicates,
    detect overlap with the PTT chord). Accept (L): pure-fn tests for each
    validation rule; an overlap warning fires for \`fn\` vs \`fn+ctrl\`."

    MEASURED at 4e8c9adf: \`shortcuts.rs:143 ALL\` contains four bindings —
    PASTE_LAST (:86), UNDO_AI_EDIT (:98), DICTATION_TOGGLE_LEGACY (:110),
    MEETING_TOGGLE (:129). \`git grep "hands_free_binding\\|cancel_binding\\|
    copy_last"\` finds no hands-free hotkey, no cancel hotkey and no
    copy-last command. §2.1 of the teardown scores hands-free 🟡 "tray toggle
    only, no dedicated hotkey", cancel ❌, copy-last ❌, paste-last ❌ "hotkey +
    menu item" (the command exists at lib.rs:2714; the MENU item is Y6-B).

    Do:
      * Add to the existing \`shortcuts.rs\` table — it exists precisely so a
        binding is declared once (shortcuts.rs:3-8 explains the duplication bug
        it was created to kill): HANDS_FREE, COPY_LAST, OPEN_SCRATCHPAD.
        CANCEL is Y3-D's; if Y3-D has landed, do not add a second one.
      * A pure validation module \`shortcuts::validate\` with the four Wispr
        rules above, each its own function and its own test:
        at most 3 keys · a modifier is required · left/right variants of one
        modifier are not two keys · a chord that overlaps the PTT chord warns.
        The overlap case must fire for \`fn\` vs \`fn⌃\` specifically, which is
        Yap's own default pair (\`ptt_binding: "fn_control"\`, lib.rs:404) and
        therefore the one a user will actually hit.
      * Every new binding is remappable through the YV15 capture control and
        PERSISTS — the 2026-07-24 audit's top user bug was that half of settings
        never persist, PTT remap included. Y4-H's exhaustive round-trip test
        must cover each new binding; if Y4-H has not landed, add them to
        \`tests/settings_kv.rs\`.
      * Per-transform shortcuts are OUT of scope here: the transform library is
        Y9-A and a shortcut for a transform that does not exist is dead config.
        Say so in the doc comment.

    Tests \`tests/shortcut_validation.rs\` (pure, no app) plus keep
    \`tests/tray_hotkey_no_collision.rs\` green — it is the existing guard
    against two bindings claiming one chord and it must now cover seven.

    What NOT to do:
      - Do NOT register a global shortcut without a modifier. A bare letter
        global hotkey breaks typing everywhere.
      - Do NOT add per-transform shortcuts in this item.
  `,
  acceptance: `
    grep -q 'pub const HANDS_FREE' desktop/src-tauri/src/shortcuts.rs
    grep -q 'pub const COPY_LAST' desktop/src-tauri/src/shortcuts.rs
    grep -q 'pub const OPEN_SCRATCHPAD' desktop/src-tauri/src/shortcuts.rs
    test -f desktop/src-tauri/tests/shortcut_validation.rs
    grep -q 'at_most_three_keys' desktop/src-tauri/tests/shortcut_validation.rs
    grep -q 'a_modifier_is_required' desktop/src-tauri/tests/shortcut_validation.rs
    grep -q 'left_and_right_variants_are_not_two_keys' desktop/src-tauri/tests/shortcut_validation.rs
    grep -q 'fn_versus_fn_control_warns_of_overlap' desktop/src-tauri/tests/shortcut_validation.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test shortcut_validation      ; test $? -eq 0
    cargo test --features custom-protocol --test tray_hotkey_no_collision ; test $? -eq 0
    cargo test --features custom-protocol --test settings_kv              ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'DB-D', prompt: 'Y8', branch: 'loop/db-d-scratchpad-as-a-real-second-window', gated: null,
  title: 'Scratchpad: a second window on a hotkey, dictate-into-note, versions — the half-built feature finished',
  preflight: `
    grep -q 'note_versions' desktop/src-tauri/src/db.rs
    grep -q 'scratchpad' desktop/src-tauri/tauri.conf.json
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test scratchpad
  `,
  spec: `
    P0 #4, VERBATIM: "Scratchpad. (M · L) Second Tauri window, global hotkey
    (\`⌥S\`), \`notes(id,title,content,preview,pinned,created,modified)\` +
    \`note_versions\`, tabs, search, and dictate-into-note using the existing PTT
    path. Ship a Flow-Bar Scratchpad button (see #5). Rich text can be v2 —
    plain text + markdown first. Why: the #1 thing Wilson named, and it is the
    cheapest large-surface win Yap has left. Accept (L): hotkey opens the panel
    without stealing focus from the prior app; a dictation started inside the
    note lands in the note, not the previous app (this is the YV21 paste-target
    guard, re-pointed); notes survive restart; version rows written on every
    finalize."

    MEASURED: Yap has HALF of this and it is invisible. \`db.rs:648\` creates a
    \`scratchpad\` table; \`db.rs:2667-2721\` are its list/insert/delete queries;
    \`src/App.tsx:60\` has \`"scratchpad"\` in the \`Nav\` union, so there is a
    view inside the main window. There is NO second window
    (\`tauri.conf.json app.windows\` has exactly one, label "main"), no ⌥S
    hotkey, and no \`note_versions\` table.

    Do:
      * A second window in tauri.conf.json, label \`scratchpad\`, its own HTML
        entry (the repo already builds two entries — \`index.html\` and
        \`float.html\` — so follow that vite multi-entry pattern exactly).
      * NON-ACTIVATING open on ⌥S (Y8-A's OPEN_SCRATCHPAD binding). The
        acceptance above is explicit: it must not steal focus from the prior
        app, because the whole point is to jot without leaving what you are
        doing.
      * \`note_versions\` migration + a version row on every finalize. Migrations
        must stay idempotent (tests/db_migration_idempotent.rs).
      * DICTATE INTO THE NOTE. This is the load-bearing part and the one that
        can go wrong dangerously: a dictation started while the scratchpad has
        focus must land in the NOTE, and a dictation started anywhere else must
        still land in the previous app. That is the YV21 paste-target guard
        re-pointed, not bypassed — and Y6-C's paste_target_e2e tests must be
        extended to cover it rather than a second target mechanism appearing.
      * Tabs, a notes rail, search over \`searchable_content\` via the existing
        FTS setup. Plain text + markdown. No rich text, no editor library.
      * \`db.rs:1191\` already classifies the scratchpad as user-typed text —
        so Clear History's treatment of notes, which DB-C forces a decision on,
        applies here. Honour whatever DB-C decided; do not decide it twice.

    Tests \`tests/scratchpad.rs\`: notes survive restart; a version row per
    finalize; FTS finds a word in a note body; delete cascades to versions;
    and the target test named above.

    Depends on Y8-A (the binding), Y6-C (the paste-target tests), DB-C.

    What NOT to do:
      - Do NOT add Lexical, ProseMirror, TipTap or any editor library. The
        teardown says plain text first and the CSP blocks external assets.
      - Do NOT let the scratchpad window activate and steal focus.
      - Do NOT sync anything anywhere. Local-only is the brand
        (the teardown marks Wispr's note sync as explicitly not wanted).
  `,
  acceptance: `
    grep -q 'note_versions' desktop/src-tauri/src/db.rs
    grep -q '"scratchpad"' desktop/src-tauri/tauri.conf.json
    test -f desktop/scratchpad.html
    test -f desktop/src-tauri/tests/scratchpad.rs
    grep -q 'notes_survive_restart' desktop/src-tauri/tests/scratchpad.rs
    grep -q 'a_version_row_is_written_on_every_finalize' desktop/src-tauri/tests/scratchpad.rs
    grep -q 'dictation_in_the_note_lands_in_the_note' desktop/src-tauri/tests/scratchpad.rs
    grep -q 'opening_the_scratchpad_does_not_steal_focus' desktop/src-tauri/tests/scratchpad.rs
    node -e "const d=require('./desktop/package.json').dependencies;process.exit(Object.keys(d).some(k=>/lexical|prosemirror|tiptap|slate|quill/.test(k))?1:0)"
    cd ${APP} && npm ci
    npx tsc --noEmit ; test $? -eq 0
    npm run build    ; test $? -eq 0
    cd src-tauri
    cargo test --features custom-protocol --test scratchpad              ; test $? -eq 0
    cargo test --features custom-protocol --test db_migration_idempotent ; test $? -eq 0
    cargo test --features custom-protocol --test paste_target_e2e        ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y8-B', prompt: 'Y8', branch: 'loop/y8-b-flow-bar-affordance-slots', gated: null,
  title: 'The pill becomes a bar: five affordance slots, each with a tooltip and a vertical-dock layout',
  preflight: `
    grep -q 'AffordanceSlot' desktop/src/pill/slots.tsx
    cd ${APP} && npm ci && npm test
  `,
  spec: `
    P0 #5, VERBATIM: "Flow-Bar affordance slots. (M · D+L) Wispr's bar hosts 8
    named widgets. Yap's minimum viable set: Dictate (center) · Scratchpad ·
    Cancel/Stop · Word-count + live WPM · Mic/Settings menu. Each must have a
    tooltip and each must have a vertical-dock layout. Why: this is what makes
    the bar a *bar* and not a dot. Accept (L): every affordance renders in all
    three dock positions within the 30 px strip; tooltips flip side based on
    dock edge."

    MEASURED: the pill is dictate-only (the teardown scores in-bar affordances
    ❌ "pill is dictate-only"). \`ClassicPill.tsx\` renders a mic/stop glyph, a
    9-bar waveform and — since YV95 — a \`MeetingBadge\`. That badge is the
    existing precedent for a slot; generalise it rather than bolting on four
    more one-off children.

    Do:
      * \`desktop/src/pill/slots.tsx\`: a slot contract
        \`{ id, glyph, tooltip, visible(ctx), onPress }\` and a layout that
        places slots along the bar's LENGTH axis — which Y5-I made
        orientation-neutral (\`--flow-bar-length\` / \`--flow-bar-thickness\`), so
        one layout serves all three docks.
      * The five slots: Dictate (centre, always), Scratchpad (only when DB-D
        landed and the user enabled it — Wispr gates theirs behind an "Add to
        Flow Bar" toggle and so should Yap), Cancel/Stop (only while a take is
        live — Y3-D's cancel), Word-count + live WPM (only while listening or
        transcribing; the number must come from Y3-C's REAL chunk words during
        transcribing and may be the estimate only while listening — the same
        honesty rule), and a Mic/Settings menu.
      * Tooltips flip side based on dock edge. A tooltip that opens off-screen
        is the specific failure the acceptance calls out.
      * Slot visibility is a pure function of context so it is testable: no slot
        may occupy space when invisible (a 30 px strip has no room for a gap),
        and the trial numeral from Y2-B is NOT a slot — it rides the capsule.
        Say so, so the two systems do not fight over the same pixels.

    Tests \`desktop/src/pill/slots.test.ts\`:
      * every slot renders within the thickness bound in all three docks
        (reuse Y5-I's \`dock-geometry.json\`, do not write a second table)
      * tooltip side flips per dock edge
      * an invisible slot occupies zero length
      * the WPM slot uses real words while transcribing and the estimate only
        while listening

    Depends on Y5-I, Y3-C, Y3-D, DB-D.

    What NOT to do:
      - Do NOT ship eight slots. Five, per the spec, and no empty ones.
      - Do NOT show a slot whose feature has not landed.
      - Do NOT let a slot steal the drag gesture from \`pill/drag.ts\` (YV65).
  `,
  acceptance: `
    test -f desktop/src/pill/slots.tsx
    test -f desktop/src/pill/slots.test.ts
    grep -q 'tooltip_side_flips_per_dock_edge' desktop/src/pill/slots.test.ts
    grep -q 'an_invisible_slot_occupies_zero_length' desktop/src/pill/slots.test.ts
    grep -q 'wpm_uses_real_words_while_transcribing' desktop/src/pill/slots.test.ts
    grep -q 'dock-geometry.json' desktop/src/pill/slots.test.ts
    test 5 -eq "$(grep -c 'id: "' desktop/src/pill/slots.tsx)"
    cd ${APP} && npm ci
    npm test         ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build    ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y8-C', prompt: 'Y8', branch: 'loop/y8-c-earcons-and-sound-design', gated: null,
  title: 'Optional earcons for start, stop, paste and achievement — off by default, Yappy-voiced',
  preflight: `
    test -d desktop/src-tauri/assets/sounds
    grep -q 'earcons' desktop/src-tauri/src/lib.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test earcons
  `,
  spec: `
    P2 #17, VERBATIM: "Earcons. (S · D) start/stop/paste/achievement sounds, off
    by default, Yappy-voiced." The teardown's §2.1 row records what Wispr ships
    and why it matters: "\`dictation-start.wav\`, \`dictation-stop.wav\`,
    \`paste.wav\`, \`achievement.wav\`, \`popo-lock.wav\`, \`Notification.wav\` + 20
    versioned variants (\`v1…v12\`) — they A/B-tested earcons", against Yap's
    "❌ no audio feedback".

    It is listed P2 and it is in this P0 file for one reason: it is the cheapest
    fix available for "the app looks broken". A hold-to-talk app with no audible
    confirmation gives the user nothing to trust when the pill is in the corner
    of their eye, and Wilson's complaint is largely about not knowing what the
    app is doing.

    Do:
      * Four short sounds, BUNDLED in the app (no CDN, no download — the CSP
        blocks external media and a missing asset must be impossible). Generate
        them procedurally or author them; either way they ship in the repo and
        \`tauri.conf.json\`'s bundle resources include them.
      * OFF by default, one setting, with the level respecting the system
        volume. Add it to Y4-H's exhaustive settings round-trip.
      * INTERACTION WITH YV28, which is the trap: Yap MUTES the whole Mac's
        output while dictating (\`mute_while_dictating: true\` by default,
        lib.rs:421) and restores the exact prior state on stop/cancel/error/exit.
        A start earcon must therefore play BEFORE the mute and a stop earcon
        AFTER the restore, or the user hears nothing and the feature looks
        broken in a new way. Assert the ordering; this is the whole item.
      * Never play during a meeting recording — it would land on the recording.
        \`tests/meeting_no_automute.rs\` already encodes the sibling rule for
        muting; follow its shape.
      * Yappy-voiced, chunky and short (under 200 ms for start/stop), matching
        the pixel-pet aesthetic. No orchestral swells, no default macOS sounds.

    Tests \`tests/earcons.rs\`: assets exist and are bundled; off by default;
    start plays before the mute and stop after the restore; nothing plays while
    a meeting records; nothing plays when the setting is off.

    Depends on Y4-H.

    What NOT to do:
      - Do NOT play a sound on every state change. Four events, no more.
      - Do NOT default them on.
      - Do NOT fetch audio at runtime.
  `,
  acceptance: `
    test -d desktop/src-tauri/assets/sounds
    test 4 -le "$(ls desktop/src-tauri/assets/sounds | wc -l | tr -d ' ')"
    test -f desktop/src-tauri/tests/earcons.rs
    grep -q 'earcons_are_off_by_default' desktop/src-tauri/tests/earcons.rs
    grep -q 'start_plays_before_the_mute_and_stop_after_the_restore' desktop/src-tauri/tests/earcons.rs
    grep -q 'nothing_plays_while_a_meeting_records' desktop/src-tauri/tests/earcons.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test earcons             ; test $? -eq 0
    cargo test --features custom-protocol --test meeting_no_automute ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y8-D', prompt: 'Y8', branch: 'loop/y8-d-coaching-nudges-in-yappys-voice', gated: null,
  title: 'Coaching nudges: the bar teaches the app, in Yappy\'s voice, without becoming nagware',
  preflight: `
    test -f desktop/src/pill/nudge.ts
    cd ${APP} && npm ci && npm test
  `,
  spec: `
    P2 #20, VERBATIM: "Coaching nudges (pixel-voiced). (M · D) Yappy's version
    of \`hub_status_pulsar_*\`: 'click a textbox and hold fn'. This is Yappy's
    natural job and it is free personality."
    The teardown's §2.2 row on what Wispr's bar actually says: "Bar teaches you:
    'Click a textbox, hold \`fn\` to dictate', 'Open Cursor or any IDE', 'Try
    dictating by double tapping' — app-type-aware suggestions", Yap ❌. §4.2
    lists \`growthNudgeActive\` as a first-class bar state with its own geometry
    (fit-content x 32, a row on side docks).

    Do:
      * \`desktop/src/pill/nudge.ts\`: a pure engine
        \`nextNudge(ctx, shown) -> Nudge | null\` where ctx is what the app
        already knows — frontmost app category (\`mode_for_app\`), whether a
        text field has focus, takes so far, days since install, which features
        have never been used.
      * A nudge fires at most once per condition, ever, and the whole engine is
        capped: at most one nudge per session and none at all after the first
        week or after N successful takes. The teardown's own "explicitly not
        wanted" list includes "trial nag surfaces" — the discipline is the
        feature. Encode both caps as named constants and test them.
      * The nudge that matters most, and the one Wilson's report implies: the
        first take when NO text field has focus. Today the take succeeds, the
        paste has nowhere to go, and the user concludes Yap is broken. "Click
        into a text field, then hold fn" turns that into a lesson.
      * Copy in Yappy's voice across the three tones (rude|friendly|rose,
        live.ts:29), through the existing \`pill/tone.ts\` so there is one voice
        system. Never a sales line, never a price — Y2 owns commerce.
      * Renders as Y5-C's \`growthNudge\` phase with Y5-I's geometry, dismissible
        with one click, and never while a take is live.

    Tests \`desktop/src/pill/nudge.test.ts\`: one per session; never twice for a
    condition; silent after the caps; the no-focus nudge fires on the first
    no-target take; no nudge mentions money; every nudge has copy in all three
    tones; none fires during a take.

    Depends on Y5-C, Y5-I, Y8-B.

    What NOT to do:
      - Do NOT nudge more than once per session.
      - Do NOT put upgrade copy in a nudge.
      - Do NOT keep nudging a user who is clearly fluent.
  `,
  acceptance: `
    test -f desktop/src/pill/nudge.ts
    test -f desktop/src/pill/nudge.test.ts
    grep -q 'at_most_one_nudge_per_session' desktop/src/pill/nudge.test.ts
    grep -q 'never_fires_twice_for_one_condition' desktop/src/pill/nudge.test.ts
    grep -q 'no_nudge_mentions_money' desktop/src/pill/nudge.test.ts
    grep -q 'the_no_focus_nudge_fires_on_the_first_no_target_take' desktop/src/pill/nudge.test.ts
    grep -q 'every_nudge_has_copy_in_all_three_tones' desktop/src/pill/nudge.test.ts
    cd ${APP} && npm ci
    npm test         ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build    ; test $? -eq 0
  `,
})

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
  title: 'Rebase the six parked branches onto main so each is evaluated against the shipped min_embed, not against what main was',
  preflight: `
    test 0 -eq "$(git branch -r --list 'origin/feat/yv1*' | wc -l)"
    gh pr list --repo wilsonguenther-dev/wilson-voice --state open --json number --jq 'length' | grep -qx 0
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
  id: 'Y11-B', prompt: 'Y11', branch: 'loop/y11-b-overlap-honesty-in-the-shipped-string', gated: null,
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
  id: 'Y11-E', prompt: 'Y11', branch: 'loop/y11-e-false-mechanism-sentence-in-the-shipped-asset', gated: null,
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
  title: 'A real-voice eval corpus to replace the synthetic one — the numbers are only floors until it exists',
  preflight: `
    test -f desktop/src-tauri/tests/fixtures/real_voice_manifest.json
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test meeting_eval
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
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/fixtures/real_voice_manifest.json
    test -f desktop/src-tauri/tests/fixtures/real_voice_manifest.sha256
    grep -q 'synthetic' desktop/src-tauri/tests/meeting_eval.rs
    grep -q 'refuses_to_blend_corpora_into_one_number' desktop/src-tauri/tests/meeting_eval.rs
    grep -q 'absent_corpus_skips_with_a_named_reason' desktop/src-tauri/tests/meeting_eval.rs
    test 0 -eq "$(find desktop/src-tauri/tests/fixtures -name '*.wav' -newer desktop/src-tauri/tests/fixtures/README.md | wc -l)"
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test meeting_eval        ; test $? -eq 0
    cargo test --features custom-protocol --test diarization_metrics ; test $? -eq 0
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
      : `BUILD PASS: two builder lanes, no reviewer and no merge agent. Each build LANDS whatever is already gate-green, then builds its own item. The review pass runs afterwards with args {mode:'review'}.`)
)

const BUILD_RECON = `RECON for the Yap overhaul loop. You set up the TWO lane work dirs the items share, you warm the
Rust build cache once per lane, you make sure the ${LOOP_LABEL} label exists, and you MEASURE
whether GitHub Actions can run at all. Do not build anything. Do not open a PR. Each item's own
pre-flight is that item's job, never yours.

# 1 — CREATE BOTH LANE WORKTREES (not clones — they share the object store, which is the disk win)
    lane A  ${WORKDIR}      port ${PREVIEW_PORT}   CARGO_TARGET_DIR ${CARGO_TARGET_A}
    lane B  ${WORKDIR_B}    port ${PREVIEW_PORT_B} CARGO_TARGET_DIR ${CARGO_TARGET_B}
    mkdir -p /Users/wilsonguenther/code/wilson-voice-loop
    cd ${LOCAL_REPO} && git fetch origin
    git worktree add ${WORKDIR} -b yap/recon-a origin/main
    git worktree add ${WORKDIR_B} -b yap/recon-b origin/main
If either already exists from an earlier attempt or an earlier PART, do NOT blindly delete it: check
'git worktree list', and REUSE it if it is clean. Say for each which you did. If the branch name is
already taken, reuse the existing worktree rather than inventing a third one.
Confirm and paste, for EACH: git log --oneline -1 && git status --porcelain

# 2 — INSTALL ONCE PER LANE (the app lives in ${APP}/, never at the repo root)
    cd ${WORKDIR}/${APP} && npm ci --cache ${NPM_CACHE}
    cd ${WORKDIR_B}/${APP} && npm ci --cache ${NPM_CACHE}
If a lane's node_modules already exists (from an earlier part) and 'npm ls --depth 0' is healthy,
say so and SKIP that install — the worktrees are deliberately left standing between parts.

# 3 — WARM THE RUST CACHE ONCE PER LANE. THIS IS THE EXPENSIVE STEP AND IT IS PAID HERE, ONCE.
src-tauri statically links transcribe-cpp's ggml and yap-polish vendors llama-cpp-sys-2: a COLD
build of this workspace is MINUTES. The target dirs live OUTSIDE the worktrees on purpose, so this
cache survives every item, every part and both passes. For EACH lane, with that lane's target dir:
    cd ${WORKDIR}/${APP} && export CARGO_TARGET_DIR=${CARGO_TARGET_A}
    cargo build -p yap-polish --release ; echo "sidecarA=$?"
    TRIPLE="$(rustc -vV | awk '/^host:/ {print $2}')" ; mkdir -p src-tauri/binaries && cp ${CARGO_TARGET_A}/release/yap-polish "src-tauri/binaries/yap-polish-$TRIPLE" ; echo "stageA=$?"
    cd ${WORKDIR}/${APP}/src-tauri && cargo build --release --features custom-protocol ; echo "appA=$?"
and the same for lane B with ${WORKDIR_B} and ${CARGO_TARGET_B}.
STAGING THE SIDECAR IS NOT OPTIONAL: bundle.externalBin makes that binary a precondition of EVERY
cargo build of the app, so a missing one fails the build for ENVIRONMENT reasons and a later agent
will waste an hour reading it as a code defect.

# 4 — BASELINE GATE, ONCE, IN LANE A ONLY (both lanes sit on the same origin/main sha)
${GATE_CMDS(WORKDIR, CARGO_TARGET_A)}
Plus the informational one, so the run knows its baseline:
    cd ${WORKDIR}/${APP} && cargo fmt --all -- --check ; echo "fmt=$?"
MEASURED ON main 2026-09-12: typecheck=0 test=0 build=0 sidecar=0 polishtest=0 clippy=0 cargotest=0
and fmt=1. If any of the eight is non-zero for you, report it as the BASELINE state; do not fix it,
and do not let a later agent be blamed for it. Do NOT repeat the baseline in lane B — confirm the
two shas match instead.

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
Then, per chain, with that chain's CARGO_TARGET_DIR exported: build and STAGE the sidecar
(cargo build -p yap-polish --release, then copy it to ${APP}/src-tauri/binaries/yap-polish-<triple>)
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
const laneItems = [ITEMS.filter((it) => laneOf(it) === 0), ITEMS.filter((it) => laneOf(it) === 1)]
const indexOfItem = (item) => ITEMS.indexOf(item)

log(
  `lane A (${WORKDIR}, port ${PREVIEW_PORT}): ${laneItems[0].map((it) => it.id).join(', ') || '(none)'} || ` +
    `lane B (${WORKDIR_B}, port ${PREVIEW_PORT_B}): ${laneItems[1].map((it) => it.id).join(', ') || '(none)'}`
)

/** ONE BUILDER LANE. It walks its own items and never awaits the other lane, ever. */
async function buildLane(lane) {
  for (const item of laneItems[lane]) {
    if (halted) break
    const at = indexOfItem(item)
    const skipped = panelSkip(item)
    if (skipped) {
      slots[at] = skipped
      continue
    }
    // Lane A is the SINGLE writer of the global phase cursor. Lane B never touches it: two writers
    // race, and the loser's phase label is what Wilson would be reading.
    if (lane === 0) phase(item.id)
    slots[at] = await runBuildSafe(item, lane)
  }
  log(`lane ${LANE_TAGS[lane]}: finished its ${laneItems[lane].length} item(s)`)
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
    gh pr list -R ${REPO} --state open --label ${LOOP_LABEL} --limit 200 --json number,title,headRefName,headRefOid,mergeable,createdAt,files
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

RETURN exactly: {queue: [{pr, item, title, headRefName, klass, ci, mergeable, files, additions}],
                 closed: [{pr, survivor, why}], counts: {open, queued, closed, security, standard, unlabelled}}
${LOGLINE('review triage', '_review', 'harness:recon', '--open <the open loop-build PR count you just observed>')}
${GUARD(BOTH_REVIEW_DIRS)}`,
    { model: 'opus', effort: 'high', phase: 'Recon', label: 'review-triage', schema: TRIAGE_SCHEMA }
  )

  REVIEW_QUEUE = extractArray(triage, 'queue')
  const closedByTriage = extractArray(triage, 'closed')
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

  const pending = reviewOutcomes.filter((r) => r && r.status === 'ci-pending')
  log(`both review chains finished: ${reviewOutcomes.length} PR(s) processed, ${pending.length} left ci-pending for the sweeper`)

  const sweep = await agentR(
    `LAND SWEEPER for ${REPO}. The review chains never wait for CI — that wait is what serialised the
previous pass — so every PR that could not be settled when its fix+land agent finished was recorded
ci-pending and left open. You settle them, and nothing else.

CI-PENDING PRs FROM THIS PASS:
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
    cd ${REVIEW_DIRS[0]}/${APP} && export CARGO_TARGET_DIR=${REVIEW_TARGETS[0]} && npm run desktop:dev
Confirm: the main window renders, the float/pill window appears, the dictation hotkey fires, and the
sidecar answers. Screenshot anything visual. If you could not launch it (no display, permission
prompt, sidecar missing), SAY SO plainly and mark the claim unverified — never imply you ran it.
Kill it before you return and confirm nothing listens on ${REVIEW_PORTS[0]}.

# 5 — THE DMG, ONLY IF ASKED
${(typeof args !== 'undefined' && args && args.dmg) ? `args.dmg IS SET, so build the bundle once from main's head and report its path and size:
    cd ${REVIEW_DIRS[0]}/${APP} && export CARGO_TARGET_DIR=${REVIEW_TARGETS[0]} && npm run desktop:build ; echo "dmg=$?"
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

  return { triage, queued: REVIEW_QUEUE.length, closedByTriage, sweep, mainVerify }
}

let census = null
if (MODE === 'review') {
  census = await reviewPass()
} else {
  await Promise.all([buildLane(0), buildLane(1)])
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
  `DRAIN + SELF-AUDIT for ${REPO}, then TEAR DOWN the work dirs.

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
   leave them open. A build-mode LAND step merges the green ones; the review pass settles the rest.
   THE PRE-EXISTING feat/yv1xx PRs ARE YOURS TO SETTLE, AND THIS IS THE RUN THAT SETTLES THEM.
   They are weeks old, they carry no ${LOOP_LABEL} label, and nothing has closed them in four
   sessions. For EACH: rebase onto main and merge it if it still makes sense and the gate is green,
   or CLOSE it with the evidence that supersedes it (name the sha or PR on main that did the work,
   or state plainly that the work was abandoned). "Still open, stale" is not an outcome. A stale PR
   nobody closes is the backlog that grows with every run.

${MERGE_BAR}
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
   labelled needs-human, ci-red, needs-rebase, plus the ones triage closed as superseded/duplicate.
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
5. DISK HYGIENE — this bit hard last time (26 clones / 78 GB on the sibling project). THERE ARE TWO
   LANES, TWO CARGO TARGET DIRS AND TWO PORTS:
     lsof -nP -iTCP:${PREVIEW_PORT} -sTCP:LISTEN     # lane A's vite/preview, if one was left
     lsof -nP -iTCP:${PREVIEW_PORT_B} -sTCP:LISTEN   # lane B's
   Kill whatever is listening on EITHER port, kill any leftover 'tauri dev' or 'yap-polish'
   process either lane started, and confirm both ports are clear afterwards.
   Also report the current sizes: du -sh ${CARGO_TARGET_A} ${CARGO_TARGET_B} and df -h /System/Volumes/Data
${
  KEEP_WORKTREE
    ? `   DO NOT REMOVE ${WORKDIR}, ${WORKDIR_B}, ${CARGO_TARGET_A} OR ${CARGO_TARGET_B}. This script is
   ONE PART of a multi-part run and the parts that follow reuse both worktrees, both node_modules
   and both warm cargo caches — a cold rebuild is ~9.5 minutes per lane per part (measured). Leave them exactly
   where they are; the LAST part's drain tears them down. Do not delete the yap/recon-a or
   yap/recon-b branches either. You still kill both port listeners, you still bundle and sweep dead
   remote branches, and you still report the target-dir sizes. Return worktreeRemoved: false and say
   plainly that everything was KEPT ON PURPOSE for the next part; that is the correct outcome here,
   not a miss.`
    : `     cd ${LOCAL_REPO} && git worktree remove --force ${WORKDIR} && git worktree remove --force ${WORKDIR_B} && git worktree prune
     git worktree list                                # paste it: BOTH lanes must be GONE
     git branch -D yap/recon-a ; git branch -D yap/recon-b     # the throwaway recon branches
     rm -rf ${CARGO_TARGET_A} ${CARGO_TARGET_B}       # cache, not checkouts: ~7.5 GB each (measured)
     rmdir /Users/wilsonguenther/code/wilson-voice-loop 2>/dev/null || ls -a /Users/wilsonguenther/code/wilson-voice-loop
   This is the LAST part of the run, so the teardown is yours and nobody else's.
   Confirm with ls -d that neither worktree and neither target dir exists, and report the bytes
   reclaimed (df -h /System/Volumes/Data before and after). Never leave a clone, a worktree or a
   target dir behind. Do NOT touch ${LOCAL_REPO}'s own checkout or its desktop/target. Removing all
   four is not optional and it is not a nicety: it is the last line of defence against the 78 GB of
   abandoned loop clones this fleet has already paid for once.`
}

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
  lanes: { A: laneItems[0].map((it) => it.id), B: laneItems[1].map((it) => it.id) },
  results,
  errored,
  counts: {
    total: ITEMS.length,
    built: results.filter((r) => r && r.status === 'built').length,
    alreadyDone: results.filter((r) => r && r.status === 'already-done').length,
    failedAcceptance: results.filter((r) => r && r.status === 'failed-acceptance').length,
    awaitingPanel: results.filter((r) => r && r.status === 'skipped: awaiting Senior Panel').length,
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
