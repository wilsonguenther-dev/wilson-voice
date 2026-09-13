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
  name: "yap-overhaul-all-part-01",
  description:
    "Part 01 of the Yap (wilson-voice) overhaul, two builder lanes: land whatever is already gate-green, pre-flight on unmodified main (already-done items are skipped), build, open a labelled PR. The adversarial review, the independent second-Opus gate and the merge bar run afterwards in the same script with args {mode:'review'}. 63 items (Y0-A..Y7-E), 1 awaiting the Senior Panel. The gate is local (Actions is disabled by the account spending limit); the DMG is not in it.",
  phases: [
    { title: "Recon", detail: "two lane worktrees, one npm ci + one warm cargo build each, the loop-build label, ci-mode measured" },
    { title: "Y0-A", detail: "Clippy becomes a blocking CI gate instead of `|| true` decoration (rustfmt stays informational — ledger)" },
    { title: "Y0-D", detail: "One documented state root override and a --smoke mode, so two lanes and Wilson's own install stop sharing one history and one hotkey" },
    { title: "Y0-E", detail: "The instrument that can see \"looks broken\" ships BEFORE the nine UI items it judges — and it fails on today's tree" },
    { title: "Y0-B", detail: "One command proves a fresh clone builds, tests and stages both sidecars — no host-found dependencies" },
    { title: "Y0-C", detail: "A test asserts the SHIPPED defaults, so a feature that is off by default can never be called tested again" },
    { title: "PERM-A", detail: "Ask macOS the actual question: AVCaptureDevice authorizationStatus + requestAccess, replacing the device probe that cannot see a denial" },
    { title: "PERM-B", detail: "A denied microphone gets its own screen with a working System Settings deep link, not a green check" },
    { title: "PERM-C", detail: "Every hotkey press re-checks the grant, and the pill shows a denied state instead of recording silence" },
    { title: "Y1-A", detail: "A tap macOS disabled is re-armed and reported — the other half of \"the hotkey is dead\"" },
    { title: "PERM-D", detail: "First run refuses to reach calibration without a real grant, and a silent take is diagnosed instead of pasted as nothing" },
    { title: "SEC-A", detail: "Stop ad-hoc signing local builds — the reason permissions \"reset\" on Wilson's own machine" },
    { title: "Y1-B", detail: "Register the sleep/wake observer two later items already claim exists" },
    { title: "PERM-E", detail: "One permission health surface, watched for revocation, covering mic + Accessibility + Input Monitoring + audio capture" },
    { title: "Y4-A", detail: "Formatting is on for a fresh install: the shipped default reaches the formatting stage" },
    { title: "Y4-I", detail: "Measure the polish stage against the real weights before anything is built on its envelope" },
    { title: "SEC-C", detail: "The polish model gets an install path, so the LLM stage can exist on a real machine" },
    { title: "Y4-C", detail: "Paragraphing: long speech becomes paragraphs by rule, not one wall of text" },
    { title: "Y4-D", detail: "Lists, nesting and spoken punctuation proven at the level the product ships, with the gaps filled" },
    { title: "Y4-E", detail: "Long-form gets polished: the 400-word cliff becomes a chunked pass that keeps the deadline" },
    { title: "Y4-F", detail: "App-aware formatting: the six modes change the output, proven per app, and auto mode picks correctly" },
    { title: "Y4-G", detail: "The user can see what formatting did and undo it in one key — the trust mechanism" },
    { title: "Y4-H", detail: "The formatting settings stop being engineer words, and every one of them persists" },
    { title: "Y3-A", detail: "Capture stops growing two unbounded Vecs — a long take spills to disk with a measured memory ceiling" },
    { title: "Y3-B", detail: "Long takes decode in windows with seam dedupe, reusing the meeting chunker instead of one 120s-capped call" },
    { title: "Y3-C", detail: "The pill reports real progress on a long take instead of an honest-looking lie" },
    { title: "Y3-D", detail: "Cancel works mid-decode, not just mid-recording — and a cancel never loses the audio" },
    { title: "DB-B", detail: "A crash or quit mid-long-take loses nothing — the take resumes or is offered back on next launch" },
    { title: "Y3-F", detail: "A declared maximum session length with a warning before it, instead of an undeclared cliff" },
    { title: "Y3-G", detail: "A measured latency and energy budget for long takes, published as a test that fails on regression" },
    { title: "LIC-A", detail: "Payment to working dictation, on a Supabase issuer this repo owns — the leg no item owned" },
    { title: "Y2-A", detail: "The float window subscribes to license status — the wiring that does not exist" },
    { title: "Y2-B", detail: "A quiet trial numeral on the pill in both pill styles, in the 30px side dock too" },
    { title: "Y2-C", detail: "A refused hotkey press produces a pill state that explains itself, instead of a throttled notification" },
    { title: "Y2-F", detail: "A paying customer whose key stops verifying is never shown a price" },
    { title: "Y2-D", detail: "One click from the pill to purchase, reusing the existing Payment Link — no new money surface" },
    { title: "Y2-E", detail: "The menu-bar item says the same thing as the pill and the settings card, from one source" },
    { title: "DB-A", detail: "Usage metering and a \"limit reached\" surface — the numbers are Wilson's call" },
    { title: "SEC-B", detail: "The trial state machine gets the adversarial tests its own doc comment promises" },
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
    { title: "UPD-A", detail: "The updater points at an endpoint that can serve a private repo — today it points at a dead URL" },
    { title: "UPD-B", detail: "Publish latest.json + .app.tar.gz + .sig to the host the updater now points at, and keep the previous build" },
    { title: "Y6-A", detail: "Onboarding ends with one successful pasted dictation, or it tells you exactly what is missing" },
    { title: "PRIV-A", detail: "Crash reporting is local, complete and provably offline — no Sentry, no PostHog, ever" },
    { title: "Y6-B", detail: "The menu bar becomes a usable surface: state, the last transcript, hide-for-an-hour, quit" },
    { title: "Y6-C", detail: "The paste lands in the app you dictated into, or it does not paste — including secure-input fields" },
    { title: "DB-C", detail: "History, FTS search and export hold up at real volume, and Clear History still destroys the words" },
    { title: "PRIV-B", detail: "Clear History erases the audio and the partial words, not only the SQLite rows" },
    { title: "Y6-D", detail: "Cold launch, sleep/wake, display change and a second copy of Yap all behave" },
    { title: "Y6-E", detail: "README, ARCHITECTURE, ROADMAP and PRODUCT stop describing an app that no longer exists" },
    { title: "Y7-A", detail: "A headless smoke that runs the real built binary end to end, not a unit test of its parts" },
    { title: "Y7-B", detail: "A windowed smoke that launches Yap, walks all seven views and captures them at two sizes" },
    { title: "Y7-C", detail: "Every test this loop added is proven non-vacuous by a mutation that makes it fail" },
    { title: "Y7-D", detail: "The pure frontend modules get real coverage, so the pill and the states are testable without a window" },
    { title: "Y7-E", detail: "The shipped DMG is smoke-tested the way a first-time user meets it" },
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
const LOG = '/Users/wilsonguenther/Obsidian/Wilson-Brain/Projects/Loop-Logs/2026-09-12-yap-part01.md'
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
const KEEP_WORKTREE = true
/** Stamped by build.mjs like LOG: which part this is. The status board is keyed on it. */
const PART = 'part-01'
/**
 * Stamped by build.mjs: item id -> lane index, round-robin over the SOURCE ITEM FILES so that a
 * whole prompt group (whose items often depend on one another) stays sequential on one lane.
 */
const LANE_BY_ID = {"Y0-A":0,"Y0-D":0,"Y0-E":0,"Y0-B":0,"Y0-C":0,"PERM-A":1,"PERM-B":1,"PERM-C":1,"Y1-A":1,"PERM-D":1,"SEC-A":1,"Y1-B":1,"PERM-E":1,"Y4-A":0,"Y4-I":0,"SEC-C":0,"Y4-C":0,"Y4-D":0,"Y4-E":0,"Y4-F":0,"Y4-G":0,"Y4-H":0,"Y3-A":1,"Y3-B":1,"Y3-C":1,"Y3-D":1,"DB-B":1,"Y3-F":1,"Y3-G":1,"LIC-A":0,"Y2-A":0,"Y2-B":0,"Y2-C":0,"Y2-F":0,"Y2-D":0,"Y2-E":0,"DB-A":0,"SEC-B":0,"Y5-G":1,"Y5-A":1,"Y5-B":1,"Y5-C":1,"Y5-D":1,"Y5-E":1,"Y5-F":1,"Y5-J":1,"Y5-I":1,"Y5-K":1,"UPD-A":0,"UPD-B":0,"Y6-A":0,"PRIV-A":0,"Y6-B":0,"Y6-C":0,"DB-C":0,"PRIV-B":0,"Y6-D":0,"Y6-E":0,"Y7-A":1,"Y7-B":1,"Y7-C":1,"Y7-D":1,"Y7-E":1}
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
 */
const RUNTIME_PROOF = `
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

${RUNTIME_PROOF}
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
${RUNTIME_PROOF}
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
${RUNTIME_PROOF}
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
${RUNTIME_PROOF}
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

// ── 05-y1-audio-permission.mjs ────────────────────────────────────────────
// Y1 — AUDIO PERMISSION. Wilson, 2026-09-12, verbatim: "audio permission is not
// requested or handled at all."
//
// He is right, and the audit found WHY he experiences it that way even though an
// onboarding button exists. Yap has never asked macOS the actual question.
//
//   git grep -n "AVCaptureDevice\|authorizationStatus\|requestAccess" origin/main -- desktop
//     -> ONE hit, and it is a comment in syscapture.rs:2117 about the CoreAudio
//        process tap ("There is no `requestAccess`, no `authorizationStatus`").
//        For the MICROPHONE there is no call anywhere in the tree.
//
// What Yap does instead, everywhere it claims to know:
//   src/mic_auth.rs:12   microphone_ready()  = default_input_device().is_some()
//                                              && default_input_config().is_ok()
//   src/permissions.rs:66 microphone_probe() = the same two calls, plus prose
//   src/permissions.rs:118 feeds that into PermissionReport.microphone
//   src/Onboarding.tsx:294-303 renders it as "Granted ✓"
//
// Neither call consults TCC. A device exists and a config resolves whether or
// not this bundle is authorized — so the checklist shows a green dot, the button
// says "Granted ✓", the take records, and the transcript is empty. From the
// user's chair that is indistinguishable from "it never asked."
//
// Three further facts that make it worse, all measured:
//   * src/lib.rs:1135, on the ONE dictation entry point, verbatim: "Do NOT call
//     mic_auth::request_microphone_access here — that is Permissions-only." So
//     the hotkey never re-checks. A permission revoked after onboarding is
//     invisible forever.
//   * src/Onboarding.tsx:320 offers "Continue anyway" with no grant.
//   * desktop/src-tauri/tauri.conf.json bundle.macOS.signingIdentity = "-"
//     (ad-hoc). Every local build gets a NEW code signature, and macOS keys TCC
//     grants to the signature — so on Wilson's own machine the grant really does
//     evaporate on every rebuild. ROADMAP.md already names this: "ad-hoc re-sign
//     invalidates trust". It is a permission bug wearing a build-config costume.
//
// The pill has no permission state at all:
//   git grep -n "permission\|denied" origin/main -- desktop/src/pill  -> 0
//
// SHARED PREAMBLE + STANDARD GATE: see 00-y0-harness-and-gates.mjs.
// Depends on Y0-A (a real clippy gate) for everything after PERM-A.

ITEMS.push({
  id: 'PERM-A', prompt: 'Y1', branch: 'loop/perm-a-real-tcc-authorization-status', gated: null,
  title: 'Ask macOS the actual question: AVCaptureDevice authorizationStatus + requestAccess, replacing the device probe that cannot see a denial',
  preflight: `
    grep -q 'AVCaptureDevice' desktop/src-tauri/src/mic_auth.rs
    grep -q 'enum MicAuth' desktop/src-tauri/src/mic_auth.rs
    test 0 -eq "$(grep -rn 'default_input_config().is_ok()' desktop/src-tauri/src/mic_auth.rs desktop/src-tauri/src/permissions.rs | wc -l)"
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test mic_auth
  `,
  spec: `
    Add the real TCC read, and make it the ONLY source of truth for microphone
    authorization in the app.

    In \`desktop/src-tauri/src/mic_auth.rs\`:
      * \`#[repr(i32)] pub enum MicAuth { NotDetermined = 0, Restricted = 1,
        Denied = 2, Authorized = 3 }\` — the exact AVAuthorizationStatus values,
        so the FFI is a transmute-free \`match\`.
      * \`pub fn authorization_status() -> MicAuth\` — objc msgSend to
        \`+[AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio]\`.
        Link AVFoundation: \`#[link(name = "AVFoundation", kind = "framework")]\`.
        Use the \`objc2\`/\`objc\` crate already in the graph if one is (check
        Cargo.lock before adding a dependency); otherwise add \`objc2\` and
        \`objc2-av-foundation\` with exact pinned versions and say so in the PR.
      * \`pub fn request_access(timeout: Duration) -> MicAuth\` — msgSend to
        \`+[AVCaptureDevice requestAccessForMediaType:completionHandler:]\`, which
        is ASYNCHRONOUS: block on a channel the completion block signals, with a
        timeout, and return the status re-read afterwards. macOS shows the system
        dialog only when the status is NotDetermined; when it is Denied the call
        returns immediately with false and the correct next step is the Settings
        deep link (PERM-B).
      * \`#[cfg(not(target_os = "macos"))]\` stub returning \`Authorized\`, so the
        crate still compiles off-Mac like \`permissions.rs:57-62\` already does.

    Then REWIRE, deleting the lie rather than layering on it:
      * \`microphone_ready()\` (mic_auth.rs:12) becomes
        \`authorization_status() == MicAuth::Authorized\` AND a device exists.
        A device probe is a HARDWARE question and must be reported separately —
        keep it as \`pub fn input_device_present() -> bool\`.
      * \`permissions::microphone_probe()\` (permissions.rs:66) returns the TCC
        status, and its prose per status. Its current message — "If Yap is
        missing from System Settings → Microphone, click Dictate once to trigger
        the prompt" — is advice built on the old wrong model; replace it.
      * \`PermissionReport\` (permissions.rs:14-26) gains
        \`pub microphone_status: String\` ("not_determined"|"denied"|"restricted"|
        "authorized") alongside the bool, and \`all_critical_ok\` requires
        Authorized. The bool stays for the existing call sites; the string is
        what the UI branches on.
      * \`#[tauri::command] fn request_microphone()\` (lib.rs:2416-2418) returns
        the status string, not a bool. Add
        \`#[tauri::command] fn microphone_status() -> String\`.

    Tests (\`desktop/src-tauri/tests/mic_auth_status.rs\`), all runnable on a CI
    runner with no mic and no grant:
      * \`mic_auth_discriminants_match_avfoundation\` — the four enum values are
        0/1/2/3. A silent renumber is a permanently wrong permission screen.
      * \`authorization_status_is_the_only_microphone_authority\` — a call-site
        sweep: \`default_input_config\` appears ZERO times in any function whose
        name or doc mentions permission/authorization/granted. Assert with
        pattern AND scope over src/mic_auth.rs and src/permissions.rs.
      * \`report_requires_authorized_for_all_critical_ok\` — construct a
        PermissionReport per status; only Authorized sets all_critical_ok.

    What NOT to do:
      - Do NOT keep the device probe as a FALLBACK when the objc call fails.
        "Couldn't ask macOS, so assume yes" reproduces the exact bug.
      - Do NOT call \`request_access\` from \`authorization_status\`. Reading must
        never prompt; the UI reads constantly.
      - Do NOT use \`AVAudioSession\` — that is iOS. macOS is AVCaptureDevice.
    ── PANEL 2026-09-12, BINDING, three corrections ────────────────────────
    (1) NEVER BLOCK. The first draft said request_access should "block on a
        channel the completion block signals, with a timeout". PERM-C then calls
        it from start_recording, which is reached ONLY on the AppKit main thread
        (lib.rs:4728-4759 — the PTT tap callback wraps everything in
        \`h.run_on_main_thread(...)\`; the tray handler and the sync Tauri command
        path are main-thread too). Blocking there stops the AppKit run loop for
        the length of a human decision on the TCC dialog: no redraws, the pill's
        new \`blocked\` phase and the \`mic_permission_required\` event cannot
        paint (webview IPC is main-thread), and macOS marks the process
        unresponsive. AVCaptureDevice's completion handler is documented as
        arriving on an arbitrary dispatch queue, so if it lands on the main
        queue the wait DEADLOCKS until the timeout and the first press always
        fails. (One panel seat placed this block on the CGEvent tap thread
        instead; that is wrong on the mechanism — the tap hops to main and
        returns — and right on the remedy.)
        So: \`authorization_status()\` is a pure, non-blocking TCC read and is
        the only thing start_recording may call. On NotDetermined, fire
        \`requestAccessForMediaType:completionHandler:\` and return IMMEDIATELY;
        the completion block emits the status event that clears the pill's
        waiting state. Make \`request_microphone\` a
        \`#[tauri::command(async)]\` so the UI path is off the main thread as well.
    (2) \`#[repr(isize)]\`, not \`#[repr(i32)]\`. AVAuthorizationStatus is
        NS_ENUM(NSInteger) — 64-bit on both arm64 and x86_64. The i32 form
        happens to survive on arm64 because 0..3 fits the low word, which makes
        it a latent bug rather than a caught one, and objc2's \`msg_send!\` does
        not verify return encodings (verification lives in \`define_class\` and
        the opt-in \`AnyClass::verify_sel\`). Keep the 0/1/2/3 discriminant test.
    (3) A BUNDLE IS REQUIRED. requestAccess reads
        NSMicrophoneUsageDescription from the MAIN BUNDLE's Info.plist and TCC
        KILLS the process when it is absent. Info.plist is merged only into the
        .app by the bundler, so \`npm run tauri dev\`, \`cargo test\` and Y7-A's
        smoke all run a bare Mach-O with no bundle. Before calling requestAccess,
        read NSBundle.mainBundle's infoDictionary for the key and, when it is
        missing, log ONCE and return NotDetermined instead of calling into TCC.
        Add \`desktop/src-tauri/Info.dev.plist\` with the three usage strings so
        \`tauri dev\` behaves like the shipped app. Any test that touches TCC is
        \`#[ignore]\`d with the reason in its name.

  `,
  acceptance: `
    grep -q 'AVCaptureDevice' desktop/src-tauri/src/mic_auth.rs
    grep -q 'authorizationStatusForMediaType' desktop/src-tauri/src/mic_auth.rs
    grep -q 'requestAccessForMediaType' desktop/src-tauri/src/mic_auth.rs
    grep -qE 'NotDetermined *= *0' desktop/src-tauri/src/mic_auth.rs
    grep -q 'input_device_present' desktop/src-tauri/src/mic_auth.rs
    grep -q 'microphone_status' desktop/src-tauri/src/permissions.rs
    grep -q 'fn microphone_status' desktop/src-tauri/src/lib.rs
    # the old lie is gone from both permission modules   (was 2 sites)
    test 0 -eq "$(grep -c 'default_input_config().is_ok()' desktop/src-tauri/src/mic_auth.rs)"
    test -f desktop/src-tauri/tests/mic_auth_status.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test mic_auth_status ; test $? -eq 0    grep -q 'repr(isize)' desktop/src-tauri/src/mic_auth.rs
    test -f desktop/src-tauri/Info.dev.plist
    grep -q 'NSMicrophoneUsageDescription' desktop/src-tauri/Info.dev.plist
    # the blocking wait must not exist anywhere on the start_recording call graph
    test 0 -eq "$(grep -cE 'thread::sleep|recv_timeout' desktop/src-tauri/src/mic_auth.rs)"

  `,
})

ITEMS.push({
  id: 'PERM-B', prompt: 'Y1', branch: 'loop/perm-b-denied-state-ui-and-settings-deeplink', gated: null,
  title: 'A denied microphone gets its own screen with a working System Settings deep link, not a green check',
  preflight: `
    grep -q 'micStatus\\|microphoneStatus' desktop/src/Onboarding.tsx
    test 0 -eq "$(grep -c 'Continue anyway' desktop/src/Onboarding.tsx)"
    grep -rq 'Privacy_Microphone' desktop/src
    cd ${APP} && npm ci && npm test -- permission
  `,
  spec: `
    Four distinct states, four distinct screens. Today there is one boolean and
    one button label ("Request Microphone" / "Granted ✓", Onboarding.tsx:300-302).

      not_determined -> primary button "Allow microphone access", which calls
                        \`request_microphone\` and shows the macOS dialog.
      authorized     -> a settled row. No button. No spinner.
      denied         -> THE SCREEN THAT DOES NOT EXIST. Say plainly that macOS is
                        blocking Yap, that the dialog will not come back, and
                        that the only way through is System Settings. One primary
                        button: "Open System Settings", then a live re-check when
                        the window regains focus.
      restricted     -> managed by an MDM profile; the user cannot fix it. Say so
                        and do not offer a button that will not work.

    The deep link already exists and is correct — \`permissions.rs:185-206\`
    exposes a settings-pane opener whose Microphone arm is
    \`x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone\`
    with the Ventura+ \`com.apple.settings.PrivacySecurity.extension\` fallback.
    Wire the UI to it; do not write a second one. Confirm the Tauri command name
    with \`git grep -n "open_privacy_pane\\|open_settings_pane" desktop/src-tauri/src/lib.rs\`
    and expose it if it is not yet a command.

    Re-check on focus, not on a timer: \`Onboarding.tsx:98-122\` polls TCC every
    tick while on the permissions step. Keep that poll (it is already bounded to
    one step), and ADD a \`window.addEventListener("focus", ...)\` re-read so
    returning from System Settings updates the row instantly instead of after
    the next interval.

    Delete "Continue anyway" (Onboarding.tsx:320). A person who continues past a
    denied microphone lands in an app whose only feature cannot run — and then
    reports that dictation is broken. Replace it with "Continue without
    dictation", which is honest, and which routes to the done step with a
    persistent banner (PERM-C) rather than to the calibration step, because
    calibration cannot succeed.

    Copy rules: sentence case, no exclamation marks, name the app as Yap, never
    "the app". State what happens next, not what went wrong
    (feedback_think_ux_first).

    Frontend tests, pure, in \`desktop/src/permission.test.ts\` over a new pure
    module \`desktop/src/permission.ts\` holding \`permissionCopy(status)\` and
    \`permissionAction(status)\`: four statuses in, four distinct copy+action
    pairs out, and a case that asserts \`restricted\` yields NO settings action.

    What NOT to do:
      - Do NOT show the denied screen for \`not_determined\`. A first-run user who
        has not been asked yet is not a user who said no.
      - Do NOT put the deep-link URL in the frontend. It must stay a
        compile-time constant in Rust (the same discipline license.rs applies to
        PAYMENT_LINK_URL and for the same reason).
  `,
  acceptance: `
    test -f desktop/src/permission.ts
    test -f desktop/src/permission.test.ts
    grep -qE '"restricted"' desktop/src/permission.ts
    test 0 -eq "$(grep -c 'Continue anyway' desktop/src/Onboarding.tsx)"      # 0  (was 1)
    grep -q 'Continue without dictation' desktop/src/Onboarding.tsx
    grep -q 'addEventListener("focus"' desktop/src/Onboarding.tsx
    # the URL never crosses into the frontend
    test 0 -eq "$(git grep -c 'x-apple.systempreferences' -- desktop/src | wc -l)"
    cd ${APP} && npm ci
    npm test -- permission   ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build            ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'PERM-C', prompt: 'Y1', branch: 'loop/perm-c-recheck-on-every-hotkey-and-pill-denied-state', gated: null,
  title: 'Every hotkey press re-checks the grant, and the pill shows a denied state instead of recording silence',
  preflight: `
    grep -q 'authorization_status' desktop/src-tauri/src/lib.rs
    test 0 -eq "$(grep -c 'Do NOT call mic_auth::request_microphone_access here' desktop/src-tauri/src/lib.rs)"
    grep -q 'permission' desktop/src/pill/live.ts
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test mic_gate
  `,
  spec: `
    \`start_recording\` (src/lib.rs:1128) is documented at lib.rs:1100-1106 as
    "the ONE gate. Every way to begin a new dictation (hotkey, hands-free, tray,
    pill, the Home button, onboarding calibration) funnels into
    \`start_recording\`". That is exactly the right shape and exactly where the
    mic check belongs. It is not there: lib.rs:1135-1136 says
    "Do NOT call mic_auth::request_microphone_access here — that is
    Permissions-only. Opening the real capture stream is enough for TCC."
    Opening the stream is NOT enough — a denied stream delivers silence.

    Add a SECOND gate beside \`license_allows_new_dictation\` (lib.rs:1111),
    modelled on it exactly, because that function is already the proven pattern
    for "refuse a new take, say why once, never touch existing data":

      fn microphone_allows_new_dictation(app, state) -> bool
        * \`mic_auth::authorization_status()\` — a cheap TCC read, not a prompt.
        * Authorized -> true.
        * NotDetermined -> emit \`mic_permission_required\` with status, call
          \`request_access\` ONCE (so the very first hotkey press does the natural
          thing and shows the system dialog), and proceed if it comes back
          Authorized.
        * Denied / Restricted -> emit \`mic_permission_required\`, throttle the
          notification exactly the way \`should_announce_gate\` does
          (license.rs) so leaning on the hotkey is not a notification storm,
          and return false.
        * Order matters: microphone BEFORE license. "Yap cannot hear you" is the
          truer message than "buy a license", and a user with no mic grant must
          never be shown a purchase prompt.

    Add \`tests/mic_gate.rs\`, mirroring \`tests/license_gate.rs\` (which reads
    lib.rs and fails if the license check appears anywhere else):
      * \`microphone_check_lives_only_in_start_recording\` — one call site.
      * \`microphone_is_checked_before_license\` — assert source order in
        start_recording, so a refactor cannot invert them.
      * \`denied_microphone_never_starts_a_recorder\` — with a stubbed status,
        \`record::start_recording\` is not reached.

    THE PILL. \`desktop/src/pill/live.ts:252\` is
    \`LivePhase = "idle" | "listening" | "thinking" | "done" | "sleepy"\` — no
    error, no permission. \`ClassicPill.tsx\` (the DEFAULT pill: lib.rs:407
    \`pill_style: "classic"\`) tracks only \`{recording, busy, message}\` and a
    \`done\` flag. Add \`"blocked"\` to LivePhase and render it in BOTH
    ClassicPill.tsx and YappyPill.tsx: the capsule goes to a muted treatment
    with a struck-through mic glyph, and clicking it opens the permission screen
    in the main window. Listen for \`mic_permission_required\` in
    \`float-main.tsx\` and route it to the phase.

    Extend \`desktop/src/pill/live.test.ts\` (already 271 lines of pure state
    machine tests — the file the CI comment at ci.yml calls out as the reason
    vitest is a gate) with: blocked outranks listening; blocked survives a
    \`recording:false\` event; blocked clears on an \`authorized\` status event.

    What NOT to do:
      - Do NOT call \`request_access\` on every press. Once per NotDetermined
        session. macOS will not re-prompt after a denial and a loop of
        no-op requests is how the hotkey feels dead.
      - Do NOT silently drop the take. A refused press must produce a visible
        pill state — an invisible refusal is the bug being fixed.
    PANEL 2026-09-12 — two binding changes.
    (a) On NotDetermined this item does NOT wait. It calls PERM-A's
        non-blocking request, moves the pill to a \`waiting for permission\`
        phase and RETURNS; the completion event either clears it (and the user
        presses again, or the take auto-arms off the event) or paints
        \`blocked\`. Blocking start_recording blocks the AppKit main thread —
        see PERM-A (1).
    (b) THIS ITEM OWNS THE WHOLE \`LivePhase\` UNION. Six items across BOTH
        lanes mutate desktop/src/pill/live.ts (PERM-C, Y2-C, Y3-C, Y5-C, Y7-D,
        Y8-D), each adding a variant from its own branch off main, and the lanes
        have no barrier — so a later builder that finds its predecessor's PR
        unlanded invents the variant itself and guarantees a conflict on the one
        surface carrying Wilson's defects (a) and (c). PERM-C is the earliest of
        the six, so it lands the COMPLETE union in one commit — every phase
        this loop will need (blocked, waiting, gated, transcribing, polishing,
        pasting, error, model-loading, empty) — with each not-yet-rendered
        variant rendering as a named placeholder and a \`// OWNED BY <item>\`
        comment. Later items only add rendering and copy; none of them edits the
        union again.

  `,
  acceptance: `
    grep -q 'fn microphone_allows_new_dictation' desktop/src-tauri/src/lib.rs
    grep -q 'mic_permission_required' desktop/src-tauri/src/lib.rs
    test 0 -eq "$(grep -c 'Do NOT call mic_auth::request_microphone_access here' desktop/src-tauri/src/lib.rs)"
    grep -q '"blocked"' desktop/src/pill/live.ts
    grep -q 'mic_permission_required' desktop/src/float-main.tsx
    grep -q 'blocked' desktop/src/pill/ClassicPill.tsx
    grep -q 'blocked' desktop/src/pill/YappyPill.tsx
    test -f desktop/src-tauri/tests/mic_gate.rs
    grep -q 'microphone_is_checked_before_license' desktop/src-tauri/tests/mic_gate.rs
    cd ${APP} && npm ci
    npm test                                              ; test $? -eq 0
    cd src-tauri && cargo test --features custom-protocol --test mic_gate ; test $? -eq 0    grep -q 'OWNED BY' desktop/src/pill/live.ts
    test 8 -le "$(grep -ro '\\bcase \"' desktop/src/pill/live.ts | wc -l)"

  `,
})

ITEMS.push({
  id: 'Y1-A', prompt: 'Y1', branch: 'loop/y1-a-cgevent-tap-disabled-by-timeout-is-re-armed', gated: null,
  title: 'A tap macOS disabled is re-armed and reported — the other half of "the hotkey is dead"',
  preflight: `
    grep -q 'kCGEventTapDisabledByTimeout\\|0xFFFFFFFE' desktop/src-tauri/src/ptt_macos.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test tap_health
  `,
  spec: `
    PANEL 2026-09-12, two seats independently. MEASURED at 4e8c9adf:
      ptt_macos.rs:299      mask = flagsChanged | keyDown only
      ptt_macos.rs:303-317  CGEventTapCreate, listen-only, CGEventTapEnable
                            called EXACTLY ONCE at startup
      ptt_macos.rs:332-376  tap_callback branches only on FLAGS_CHANGED (12)
                            and KEY_DOWN (10); everything else returns the event
      ptt_macos.rs:174      the CGEventTapEnable extern is already declared
      grep -rn 'TapDisabled|CGEventTapEnable' over all item files -> ZERO hits
    macOS disables a tap whose callback is slow and notifies the callback with
    \`kCGEventTapDisabledByTimeout\` (0xFFFFFFFE) or \`ByUserInput\`
    (0xFFFFFFFF) — delivered regardless of the event mask — and the tap stays
    dead until CGEventTapEnable is called again. This tree never calls it again.
    So one slow callback, one long decode, or one sleep/wake cycle can silently
    kill the push-to-talk hotkey for the rest of the process's life, which is
    INDISTINGUISHABLE from the permission bug the rest of this file is chasing.
    Fixing TCC without fixing this leaves half of Wilson's observation (b) open.

    Do:
      * Handle both disable types in tap_callback: classify which, log it once
        at warn with the type name, call \`CGEventTapEnable(tap, true)\` (retain
        the CFMachPort so the callback can), and increment a counter.
      * Raise a health state the pill (PERM-C's surface) and PERM-E's report can
        show: "the hotkey stopped listening — re-armed". Re-arm is silent the
        first time and visible if it happens twice in one session.
      * Keep the classification in a PURE function over the event type so it is
        unit-testable with no window server.

    What NOT to do:
      - Do NOT respawn the tap thread as the fix. Re-enable the existing tap.
      - Do NOT swallow ByUserInput as normal: it means a user-input flood took
        the tap out, and it still needs re-arming.
  `,
  acceptance: `
    grep -qE '0xFFFFFFFE|kCGEventTapDisabledByTimeout' desktop/src-tauri/src/ptt_macos.rs
    grep -qE '0xFFFFFFFF|kCGEventTapDisabledByUserInput' desktop/src-tauri/src/ptt_macos.rs
    test 2 -le "$(grep -c 'CGEventTapEnable' desktop/src-tauri/src/ptt_macos.rs)"
    test -f desktop/src-tauri/tests/tap_health.rs
    grep -q 'a_timeout_disable_is_classified_and_re_armed' desktop/src-tauri/tests/tap_health.rs
    grep -q 'a_user_input_disable_is_classified_and_re_armed' desktop/src-tauri/tests/tap_health.rs
    grep -q 're_arm_attempts_are_counted_not_just_logged' desktop/src-tauri/tests/tap_health.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test tap_health ; test $? -eq 0
    sed -i.bak 's/0xFFFFFFFE/0x0EADBEEF/' src/ptt_macos.rs
    cargo test --features custom-protocol --test tap_health ; test $? -ne 0
    mv src/ptt_macos.rs.bak src/ptt_macos.rs
    git diff --exit-code src/ptt_macos.rs ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'PERM-D', prompt: 'Y1', branch: 'loop/perm-d-first-run-preflight-before-any-take', gated: null,
  title: 'First run refuses to reach calibration without a real grant, and a silent take is diagnosed instead of pasted as nothing',
  preflight: `
    grep -q 'silent_take\\|all_zero_samples' desktop/src-tauri/src/lib.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test silent_capture
  `,
  spec: `
    Two halves of the same failure: a take that recorded nothing.

    (1) FIRST-RUN ORDER. \`Onboarding.tsx:41\` is
    \`STEP_ORDER = ["welcome","permissions","calibration","done"]\` and
    calibration records a real take. With no grant that take can only produce
    silence, and the app's own response to silence is the YV16 no-speech gate,
    which correctly refuses to paste — so the user's first experience of Yap is
    a recording that does nothing and says nothing about permissions. Gate the
    calibration step's record button on \`microphone_status === "authorized"\`,
    wearing the same waiting-ribbon pattern the step already uses for a
    downloading model (Onboarding.tsx:~340, YV54), with the reason named.

    (2) DIAGNOSE SILENCE. \`record.rs\` already computes level and voiced
    seconds (\`record.rs:881\` builds an RMS series; \`microphone_ready\` prose
    mentions \`voiced_seconds\`). Add, at the end of a take, before the
    hallucination gate at lib.rs:1546:
      * if the take's peak amplitude is EXACTLY zero across the whole buffer,
        that is not quiet speech — it is a muted or unauthorized input. Emit
        \`take_failed\` with code \`silent_capture\`, re-read
        \`mic_auth::authorization_status()\`, and surface the permission screen if
        it is not Authorized and a "check your input device / it may be muted"
        toast if it is.
      * never paste, never store a transcript row for such a take, and
        preserve the clip under the existing recovery-dir lifecycle
        (\`recovery_dir()\`, lib.rs:1145, YV63) rather than unlinking it.

    Add \`tests/silent_capture.rs\`:
      * \`all_zero_buffer_is_classified_silent_not_quiet\` — a zero buffer and a
        -60 dBFS buffer classify differently. The second must NOT be silent: a
        whisper is a real take.
      * \`silent_capture_never_reaches_the_paste_path\`.
      * \`silent_capture_rechecks_authorization\`.

    Also: \`desktop/src-tauri/Info.plist\` already carries
    NSMicrophoneUsageDescription, NSAudioCaptureUsageDescription and
    NSAppleEventsUsageDescription — verified present at 4e8c9adf, do not touch
    them. Add a test asserting all three survive, because a missing usage string
    is an instant TCC failure with no dialog at all and no other test sees it.

    What NOT to do:
      - Do NOT treat "very quiet" as silent. That would suppress real takes in
        a quiet room and is a worse bug than the one being fixed.
      - Do NOT delete the clip on silent_capture.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/silent_capture.rs
    grep -q 'all_zero_buffer_is_classified_silent_not_quiet' desktop/src-tauri/tests/silent_capture.rs
    grep -q 'silent_capture' desktop/src-tauri/src/lib.rs
    grep -qE 'authorized' desktop/src/Onboarding.tsx
    grep -q 'NSMicrophoneUsageDescription' desktop/src-tauri/tests/silent_capture.rs
    cd ${APP} && npm ci && npm run build ; test $? -eq 0
    cd src-tauri
    cargo test --features custom-protocol --test silent_capture ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'SEC-A', prompt: 'Y1', branch: 'loop/sec-a-stable-signing-identity-so-grants-survive-rebuilds', gated: null,
  title: 'Stop ad-hoc signing local builds — the reason permissions "reset" on Wilson\'s own machine',
  preflight: `
    test 0 -eq "$(grep -c '"signingIdentity": "-"' desktop/src-tauri/tauri.conf.json)"
    test -x scripts/sign-local.sh
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test entitlements
  `,
  spec: `
    \`desktop/src-tauri/tauri.conf.json\` bundle.macOS.signingIdentity is \`"-"\`
    — ad-hoc. macOS keys TCC grants (Microphone, Accessibility, Input
    Monitoring) to the code signature, so every \`npm run tauri build\` produces
    an app macOS treats as a stranger and every grant has to be re-given.
    ROADMAP.md, "Permissions (hardest part)", already states it: "ad-hoc re-sign
    invalidates trust → user toggles off/on after each rebuild until Developer ID
    signing." That is a large part of what "permission is not handled at all"
    feels like from the inside.

    The certificates exist. project_yap_build_state records the documented
    manual dance: \`xattr -cr\`, then
    \`codesign --force --deep --entitlements src-tauri/Entitlements.plist
     --sign "Apple Development: Wilson Guenther (U8BP8Z86T2)"\`, because "the
    tauri codesign flakes on resource-fork detritus". The release path signs with
    \`Developer ID Application: Wilson Guenther (VHYV8C2JNU)\` via
    APPLE_SIGNING_IDENTITY in .github/workflows/release.yml.

    Do:
      * \`signingIdentity\` becomes an env-driven value, read from
        \`APPLE_SIGNING_IDENTITY\` (Tauri supports the env var; confirm against
        the Tauri 2 config docs before choosing between removing the key and
        setting it to null). Ad-hoc must not be the committed default.
      * Create \`scripts/sign-local.sh\`: xattr -cr the bundle, codesign with the
        FIRST available identity in this order — \`Developer ID Application\`,
        then \`Apple Development\` — using Entitlements.plist, then verify with
        \`codesign -dv --verbose=4\` and \`codesign -d --entitlements :-\`. It
        FAILS if no identity is present, printing the exact
        \`security find-identity -v -p codesigning\` command; it never falls back
        to ad-hoc. Replace the hand-typed dance in scripts/rebuild_app.sh with a
        call to it.
      * \`tests/entitlements.rs\`: parse Entitlements.plist and assert
        \`com.apple.security.app-sandbox\` is present AND its value is FALSE —
        the VALUE, not the key (feedback_never_sandbox_utility_apps says exactly
        this), that \`com.apple.security.device.audio-input\` is true, and that
        the two dylib-injection entitlements the plist's own comment forbids
        (\`cs.disable-library-validation\`,
        \`cs.allow-dyld-environment-variables\`) are ABSENT.
      * Document in docs/RELEASE.md: which identity signs what, and that a
        grant lost after a rebuild means the signature changed.

    What NOT to do:
      - Do NOT enable the sandbox. Ever. It silently kills the CGEvent tap, the
        synthesized ⌘V and AXIsProcessTrusted, and the failure looks like a
        permissions bug even when the grant is present.
      - Do NOT hardcode a certificate SHA or a team id into a committed file.
        Read the identity at sign time.
      - Do NOT print a certificate name or serial into any log this repo commits.
    PANEL 2026-09-12 — three seats converged that this item, whose whole point
    is "grants survive a rebuild", cannot currently tell a stably-signed build
    from the ad-hoc one it replaces: every check it lands is a grep of JSON or
    shell text, with no build, no codesign and no verification.
      * DEFINE THE UNSET CASE. No APPLE_SIGNING_IDENTITY -> the build FAILS
        with the \`security find-identity -v -p codesigning\` hint. It NEVER
        falls through to ad-hoc and never to unsigned — unsigned is worse than
        ad-hoc for TCC persistence and would pass a "no ad-hoc" grep. An
        env-driven identity plus a keychain certificate is state "found on the
        machine", which Y0-B's runtime-dependency rule forbids, so the named
        failure is what makes it legitimate.
      * PROVE IT. scripts/sign-local.sh runs inside the acceptance against a
        built .app and the acceptance greps its own \`codesign -dv --verbose=4\`
        output for an Authority line and for the ABSENCE of "Signature=adhoc",
        plus \`codesign -d --entitlements :-\` showing app-sandbox false.
      * TWO PROFILES, and Y7-E must agree with them: a RELEASE profile
        (Developer ID + notarized + stapled — the only identity available on
        this machine is a Developer ID) and a LOCAL profile (Apple Development,
        unnotarized, stable designated requirement, documented as such in
        docs/RELEASE.md). \`spctl --assess\` reporting "Notarized Developer ID"
        is a RELEASE assertion only; a locally signed DMG cannot satisfy it, and
        Y7-E asserting it unconditionally makes the two items disagree about
        what a good build is.

  `,
  acceptance: `
    test 0 -eq "$(grep -c '"signingIdentity": "-"' desktop/src-tauri/tauri.conf.json)"   # 0 (was 1)
    grep -q 'APPLE_SIGNING_IDENTITY' desktop/src-tauri/tauri.conf.json docs/RELEASE.md
    test -x scripts/sign-local.sh
    grep -q 'find-identity' scripts/sign-local.sh
    test 0 -eq "$(grep -c 'sign "-"' scripts/sign-local.sh)"
    grep -q 'sign-local.sh' scripts/rebuild_app.sh
    test -f desktop/src-tauri/tests/entitlements.rs
    grep -q 'app-sandbox' desktop/src-tauri/tests/entitlements.rs
    grep -q 'disable-library-validation' desktop/src-tauri/tests/entitlements.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test entitlements ; test $? -eq 0    grep -q 'find-identity' scripts/sign-local.sh
    grep -q 'codesign -dv' scripts/sign-local.sh
    grep -q 'adhoc' scripts/sign-local.sh
    grep -q 'Apple Development' docs/RELEASE.md

  `,
})

ITEMS.push({
  id: 'Y1-B', prompt: 'Y1', branch: 'loop/y1-b-register-the-sleep-wake-observer-that-does-not-exist', gated: null,
  title: 'Register the sleep/wake observer two later items already claim exists',
  preflight: `
    grep -qE 'NSWorkspaceWillSleepNotification|IORegisterForSystemPower' desktop/src-tauri/src/power.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test matrix_row16_sleep_wake
  `,
  spec: `
    PANEL 2026-09-12, two seats independently, and it deletes a FALSE CAPABILITY
    CLAIM from two other items. PERM-E's spec says "power.rs already observes
    sleep/wake — reuse it, do not add a second observer" and Y6-D repeats it.
    MEASURED at 4e8c9adf: power.rs registers NOTHING. It is
    IOPMAssertionCreateWithName / IOPMAssertionRelease only (power.rs:63-135).
    The repo has already written this down itself —
    meeting_matrix.rs:398-408 carries
    \`Coverage::PolicyOnly { absent_call_site: "NSWorkspaceWillSleepNotification" }\`
    with the comment "power.rs mentions NSWorkspaceWillSleepNotification in a
    comment and registers nothing", and
    \`git grep -n "WillSleep|DidWake|IORegisterForSystemPower|addObserver" -- desktop\`
    returns comments only, zero registrations.

    Following PERM-E as first written therefore produces a watcher that never
    re-reads after wake — the single most common way a TCC grant or an audio
    device changes under a laptop user — plus a test that asserts a subscription
    to a publisher with no input.

    Do:
      * Register the call site in power.rs and NOWHERE else: a block-based
        observer on \`NSWorkspace.sharedWorkspace.notificationCenter\` for
        \`NSWorkspaceWillSleepNotification\` and
        \`NSWorkspaceDidWakeNotification\` (objc2-app-kit 0.3.2 and block2 0.6.2
        are already in desktop/Cargo.lock — no new dependency), or
        IORegisterForSystemPower if the block API fights the Tauri run loop.
      * Fan ONE event out to three consumers: the dictation path (Y6-D's
        mid-take behaviour), meeting_matrix's SleepEvent, and PERM-E's
        revocation re-check. One publisher, three subscribers.
      * Flip meeting_matrix row 16 off \`Coverage::PolicyOnly\` and assert the row
        and the code agree, so they can never drift apart again.
      * Unregister on shutdown. An observer that outlives the app is a crash.
  `,
  acceptance: `
    grep -qE 'NSWorkspaceWillSleepNotification|IORegisterForSystemPower' desktop/src-tauri/src/power.rs
    grep -q 'NSWorkspaceDidWakeNotification' desktop/src-tauri/src/power.rs
    test 0 -eq "$(grep -c 'absent_call_site: "NSWorkspaceWillSleepNotification"' desktop/src-tauri/src/meeting_matrix.rs)"
    test -f desktop/src-tauri/tests/matrix_row16_sleep_wake.rs
    grep -q 'one_observer_fans_out_to_dictation_meetings_and_permissions' desktop/src-tauri/tests/matrix_row16_sleep_wake.rs
    grep -q 'row16_is_no_longer_policy_only' desktop/src-tauri/tests/matrix_row16_sleep_wake.rs
    grep -q 'the_observer_is_unregistered_on_shutdown' desktop/src-tauri/tests/matrix_row16_sleep_wake.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test matrix_row16_sleep_wake ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'PERM-E', prompt: 'Y1', branch: 'loop/perm-e-permission-health-row-and-revocation-watch', gated: null,
  title: 'One permission health surface, watched for revocation, covering mic + Accessibility + Input Monitoring + audio capture',
  preflight: `
    grep -q 'permission_changed' desktop/src-tauri/src/permissions.rs
    grep -rq 'PermissionHealth' desktop/src
    cd ${APP} && npm ci && npm test -- permission
  `,
  spec: `
    Yap needs four grants and reports them in three unrelated places today: the
    onboarding checklist (Onboarding.tsx:286-325), \`PermissionReport.summary\`
    (permissions.rs:23), and the meeting setup flow (the audio-capture deep link
    at permissions.rs:202, YV102). A user whose Accessibility grant was revoked
    by an OS update finds out when paste silently fails.

    Build ONE surface and one watcher:
      * \`permissions::report()\` gains Input Monitoring
        (\`Privacy_ListenEvent\`, already in the deep-link table at
        permissions.rs:196 — needed for the modifier-only fn / fn⌃ PTT hold that
        \`ptt_macos.rs\` implements) and system audio capture
        (\`Privacy_AudioCapture\`, for the notetaker tap), each with its own
        status string and its own deep link.
      * A single watcher thread re-reads all four on: app launch, window focus,
        wake from sleep (\`power.rs\` already observes sleep/wake — reuse it, do
        not add a second observer), and every hotkey refusal. It emits
        \`permission_changed\` ONLY on a transition, never on every read.
      * A \`PermissionHealth\` row in the main window that is INVISIBLE when all
        four are fine and a single calm line when one is not, with the one
        button that fixes it. Not a dashboard, not four persistent cards
        (feedback_no_generic_ui).
      * The pill's \`blocked\` phase (PERM-C) covers mic. Accessibility failure has
        its own shape: the take succeeds and the PASTE fails. Route that to the
        existing paste error path (\`paste_tx.rs\`) so the message names
        Accessibility instead of reading as a transcription failure.

    Minimum polling discipline: no interval under 30 s anywhere in the watcher,
    and it must park entirely while all four are Authorized. Yap's energy pass
    (YV81) removed busy timers on purpose; do not reintroduce one here.

    Tests: pure \`permissionHealth(report)\` in desktop/src/permission.ts ->
    \`{ visible, line, action } \`, with the all-authorized case asserting
    \`visible === false\`. Rust side: \`tests/permission_transitions.rs\` asserts
    \`permission_changed\` fires once per transition and zero times on repeat
    reads of an unchanged report.

    What NOT to do:
      - Do NOT add a second sleep/wake observer. power.rs owns that.
      - Do NOT show a permission banner while all four are granted. A
        permanent nag is how a good app becomes nagware.
    ── PANEL 2026-09-12, BINDING ───────────────────────────────────────────
    (1) DELETE the claim "power.rs already observes sleep/wake — reuse it, do
        not add a second observer". MEASURED: power.rs registers NOTHING
        (power.rs:63-135 is IOPMAssertionCreateWithName/Release only) and the
        repo says so itself at meeting_matrix.rs:398-408,
        \`Coverage::PolicyOnly { absent_call_site: "NSWorkspaceWillSleepNotification" }\`.
        Y1-B now writes that call site and runs BEFORE this item. Consume Y1-B's
        publisher; do not register a second observer.
    (2) FOUR GRANTS, TRI-STATE, and two of them cannot be read the way the
        first draft assumed. Each grant reports Authorized / Denied / UNKNOWN,
        and Unknown is a first-class, NON-NAGGING state:
          * Microphone — AVCaptureDevice.authorizationStatusForMediaType (PERM-A).
          * Accessibility — AXIsProcessTrustedWithOptions (permissions.rs:50-54).
          * Input Monitoring — \`IOHIDCheckAccess(kIOHIDRequestTypeListenEvent)\`
            from IOKit, named here because it appears NOWHERE in the tree today
            (\`git grep IOHIDCheckAccess -- desktop\` -> 0). permissions.rs:195-204
            holds deep LINKS only, which is what the first draft mistook for a
            status. A tap that is already working under an Accessibility grant
            must never be reported blocked.
          * System audio capture — UNKNOWN BY CONSTRUCTION. There is no public
            API, and syscapture.rs:2112-2119 already quotes the reason: "There's
            no public API to request audio recording permission or to check if
            the app has that permission ... no requestAccess, no
            authorizationStatus", and TCC does not re-ask after a denial. Infer
            it only from the YV102 pre-warm's observed outcome. NEVER probe and
            NEVER report a probe result as a grant — that is the same lie PERM-A
            exists to delete.
    (3) DELETE \`ffmpeg_ok\`. permissions.rs:119 hardcodes it true with the
        comment "no longer required — cpal in-process", and Onboarding.tsx:22-30
        still models it. A vestigial always-true row in the surface that is
        supposed to be the single source of permission truth is worse than no
        row. Removing it also means \`all_critical_ok\` stops being computed from
        a fiction.
    (4) The four rows here ARE the onboarding checklist's rows — one component,
        one source — so PERM-B and Y6-A render this, not their own list.

  `,
  acceptance: `
    grep -q 'Privacy_ListenEvent' desktop/src-tauri/src/permissions.rs
    grep -q 'permission_changed' desktop/src-tauri/src/permissions.rs
    grep -q 'permissionHealth' desktop/src/permission.ts
    grep -rq 'PermissionHealth' desktop/src
    test -f desktop/src-tauri/tests/permission_transitions.rs
    # no sub-30s polling introduced anywhere in the watcher
    test 0 -eq "$(grep -rnE 'from_secs\\((?:[0-9]|1[0-9]|2[0-9])\\)' desktop/src-tauri/src/permissions.rs | wc -l)"
    cd ${APP} && npm ci
    npm test -- permission ; test $? -eq 0
    cd src-tauri && cargo test --features custom-protocol --test permission_transitions ; test $? -eq 0    grep -q 'IOHIDCheckAccess' desktop/src-tauri/src/permissions.rs
    test 0 -eq "$(grep -c 'ffmpeg_ok' desktop/src-tauri/src/permissions.rs)"
    test 0 -eq "$(grep -rc 'ffmpeg_ok' desktop/src/Onboarding.tsx)"
    grep -q 'Unknown' desktop/src-tauri/src/permissions.rs
    grep -q 'an_unknown_grant_is_invisible_and_never_nags' desktop/src-tauri/tests/permission_health.rs
    grep -q 'audio_capture_is_unknown_by_construction_never_probed' desktop/src-tauri/tests/permission_health.rs

  `,
})

// ── 10-y4-formatting.mjs ──────────────────────────────────────────────────
// Y4 — FORMATTING. Wilson, 2026-09-12, verbatim: "formatting does not work
// whatsoever."
//
// He is exactly right, and the reason is not a broken algorithm. It is TWO
// DEFAULTS. The formatting pipeline is well built, heavily tested, and
// UNREACHABLE in the shipping configuration:
//
//   src-tauri/src/lib.rs:417    cleanup_level: "light".into()        <- shipped default
//   src-tauri/src/dictation.rs:636
//       fn runs_format(self) -> bool {
//           matches!(self, CleanupLevel::Medium | CleanupLevel::High) }
//   src-tauri/src/dictation.rs:644
//       fn runs_llm(self) -> bool { self == CleanupLevel::High }
//   src-tauri/src/lib.rs:418    polish_model: String::new()          <- "" = OFF
//   src-tauri/src/lib.rs:264-267 doc: with no id "`polish::polish_llm` never
//                                spawns the sidecar and the cleanup pipeline is
//                                exactly what it is today."
//
// CleanupLevel::Light (dictation.rs:607-616) runs the dictionary and backtrack
// ONLY. So on every fresh install: no list detection, no spoken punctuation
// marks (`apply_spoken_marks`), no email shape (`format_email_shape`, R13), no
// trailing-period tone rule (R3/R14), no LLM polish. Saying "new line" types the
// words "new line". That is "does not work whatsoever", literally.
//
// AND THE TESTS ALL PASS, because every fixture pins its own level:
//   tests/fixtures/formatting/group-a-sentence-shape.jsonl line 4:
//     {"id":"a04-r1-quotation-pair", ... "level":"medium", ...}
// The corpus proves a configuration the product does not ship. This is the
// canonical case from the "verification that verifies nothing" rule and it
// is why Y0-C (a shipped-defaults test) is a prerequisite.
//
// SECOND, INDEPENDENT DEFECT — long-form gets no polish even at High:
//   src-tauri/src/polish.rs:73   const MAX_POLISH_WORDS: usize = 400;
//   its doc, :71-72: "Long-form is rules-only: chunking is a later item".
//   Y3-B builds that chunking; Y4-E consumes it.
//
// THIRD — the polish model is never installed. Onboarding's STEP_ORDER
// (src/Onboarding.tsx:41) is welcome | permissions | calibration | done; it
// downloads an ASR model and never mentions a polish model, so `polish_model`
// stays "" forever even if the level is raised. SEC-C fixes the install path.
//
// SHARED PREAMBLE + STANDARD GATE: see 00-y0-harness-and-gates.mjs.
// Depends on Y0-C.

ITEMS.push({
  id: 'Y4-A', prompt: 'Y4', branch: 'loop/y4-a-formatting-on-by-default', gated: null,
  title: 'Formatting is on for a fresh install: the shipped default reaches the formatting stage',
  preflight: `
    grep -qE 'cleanup_level: "(medium|high)"' desktop/src-tauri/src/lib.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test shipped_defaults
    cargo test --features custom-protocol --test formatting_fixtures
  `,
  spec: `
    THE ONE-LINE FIX AND ITS FULL COST. Change lib.rs:417
    \`cleanup_level: "light"\` to \`"medium"\` so \`runs_format()\`
    (dictation.rs:636) is true out of the box, and then earn it:

      1. Run the WHOLE formatting corpus at the new default level and fix what
         breaks. The corpus is
         tests/fixtures/formatting/{group-a-sentence-shape,group-b-lists,group-c-llm-shape}.jsonl
         driven by tests/formatting_fixtures.rs. Rows pinning "medium" already
         pass; the work is rows that assumed "light".
      2. Add a fixture row at the SHIPPED level for every rule the formatting
         stage owns, because Y0-C's
         \`formatting_fixtures_declare_the_shipped_level\` now requires at least
         one and one is not enough. Cover, each with its own row: list
         detection; \`apply_spoken_marks\` punctuation-by-name ("period",
         "comma", "question mark", "quotation mark" pairing —
         dictation.rs:1203, :2845, :2892 show the table and its tests); the
         line/paragraph commands ("new line", "new paragraph"); email shape
         (R13, \`format_email_shape\`); the tone-dialled trailing period (R3/R14).
      3. \`polish_model\` STAYS "" in this item. LLM polish is SEC-C/C. This item
         is the RULES stage only, and separating them is what makes a regression
         attributable.
      4. Update Y0-C's \`shipped_defaults.rs\` table to the new value and remove
         any \`#[ignore]\` on \`defaults_reach_every_cleanup_stage\`.
      5. MIGRATE, UNCONDITIONALLY. PANEL 2026-09-12 overrides the first draft
         of this step, which said "migrate ONLY a value that is absent or was
         never explicitly set ... if you cannot, do not migrate". That condition
         is UNIMPLEMENTABLE and therefore resolves to "do nothing":
         \`save_settings\` persists the WHOLE \`AppSettings\` struct
         (lib.rs:2281-2298) under a container-level \`#[serde(default)]\`, so
         \`cleanupLevel\` is present in every store the moment onboarding saves
         and carries no "the user chose this" bit anywhere. And
         \`apply_settings_migrations\` returns early for any store at the current
         version (lib.rs:2253-2256, \`CURRENT_SETTINGS_SCHEMA_VERSION = 1\`).
         MEASURED on this machine: ~/Library/Application Support/WilsonVoice/
         settings.json holds \`"cleanupLevel": "light"\` with
         \`"schemaVersion": 1\` — i.e. the reporter's own install. A fix that
         only reaches fresh installs does not fix Wilson's complaint.
         So: bump \`CURRENT_SETTINGS_SCHEMA_VERSION\` to 2, add a v1 -> v2 arm
         that rewrites \`cleanup_level\` "light" -> "medium" with no condition
         (the old value was a default nobody chose), show the user ONE quiet
         line saying formatting is now on and where to turn it off, and start
         recording provenance from here on (\`cleanup_level_set_by_user: bool\`,
         set only by the settings command that writes the picker) so the NEXT
         default change can be honest. A v2 store with
         \`cleanupLevelSetByUser: true\` is never rewritten.

    Also fix the Settings copy. The picker offers none|light|medium|high with no
    explanation of what any of them do; the level names are engineer words. Give
    each option a one-line description in the user's terms, generated from the
    same \`runs_*\` predicates so the copy cannot drift from the behaviour.

    What NOT to do:
      - Do NOT gate the migration on a provenance bit that does not exist yet.
      - Do NOT set the default to "high". High runs the LLM, which needs a model
        that is not installed (SEC-C), and a default that silently no-ops is the
        exact class of bug being deleted.
      - Do NOT make formatting unconditional and delete CleanupLevel. "none" is
        a real user need (verbatim mode) and a real test axis.
      - Do NOT edit fixture EXPECTATIONS to make a failing row pass. If the
        formatter is wrong, fix the formatter.
  `,
  acceptance: `
    grep -qE 'cleanup_level: "medium"' desktop/src-tauri/src/lib.rs      # was "light"
    grep -qE 'polish_model: String::new\\(\\)' desktop/src-tauri/src/lib.rs  # unchanged in this item
    # PANEL: the two lines that used to sit here (>0 medium rows; a 'new paragraph'
    # substring) are GREEN ON UNMODIFIED MAIN — 36 medium rows and one matching
    # row already exist. They proved nothing. The real bar is Y0-C's per-rule
    # coverage test plus the migration proof below.
    grep -q 'every_rule_has_a_fixture_row_at_the_shipped_level' desktop/src-tauri/tests/shipped_defaults.rs
    grep -q 'a_v1_store_with_light_loads_as_medium' desktop/src-tauri/tests/settings_migration.rs
    grep -q 'an_explicit_user_choice_is_never_rewritten' desktop/src-tauri/tests/settings_migration.rs
    grep -qE 'CURRENT_SETTINGS_SCHEMA_VERSION: u32 = 2' desktop/src-tauri/src/lib.rs
    test 0 -eq "$(grep -rn '#\\[ignore\\]' desktop/src-tauri/tests/shipped_defaults.rs | wc -l)"
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test shipped_defaults     ; test $? -eq 0
    cargo test --features custom-protocol --test formatting_fixtures  ; test $? -eq 0
    cargo test --features custom-protocol --test settings_kv          ; test $? -eq 0    cargo test --features custom-protocol --test settings_migration   ; test $? -eq 0

  `,
})

ITEMS.push({
  id: 'Y4-I', prompt: 'Y4', branch: 'loop/y4-i-measure-the-polish-sidecar-against-the-real-weights', gated: null,
  title: 'Measure the polish stage against the real weights before anything is built on its envelope',
  preflight: `
    test -f docs/BUDGETS.md
    grep -q 'polish_latency' docs/BUDGETS.md
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test polish_envelope
  `,
  spec: `
    PANEL 2026-09-12. Every number the Y4 LLM lane is built on is a derived
    guess and nothing in the repo can falsify one: \`cargo test -p yap-polish
    --release\` passes 11 tests, all prompt- and protocol-level, and NOT ONE
    loads a model. MEASURED in the panel session on an M4 Pro, warm sidecar,
    deadline 600 s (i.e. pure model cost):
      qwen2.5-1.5B q4_k_m: 30w/423ms ok · 60w/527ms ok · 100w/864ms ok ·
        150w/1186ms ok · 200w/2880ms ERR=max_out · 300w/1526ms ok ·
        400w/5671ms ERR=max_out ; cold ready 7,323 ms
      qwen2.5-0.5B q4_k_m: 30w ok · 60w ok · then ERR=max_out at 100, 150, 200,
        300 and 400 words
    Constants for contrast: polish.rs:59 DEFAULT_POLISH_DEADLINE_MS = 1200,
    :63-64 bounds 100..5000, :73 MAX_POLISH_WORDS = 400, and
    yap-polish/src/main.rs:544-550 turns a max_out overrun into a HARD ERROR
    that discards the whole rewrite for KIND_POLISH. So the stage's real working
    envelope at the shipped deadline is roughly a paragraph, the 0.5B "fast
    tier" is worse than no model, and MAX_POLISH_DEADLINE_MS (5000) is below the
    measured 400-word cost.

    Do:
      * \`desktop/src-tauri/tests/polish_envelope.rs\`: drive the REAL sidecar
        against the REAL GGUF and record p50/p95 ms against input words, plus
        cold-ready ms. SKIP with a NAMED reason when the weights are absent
        (the Y11-F pattern) — never silently pass.
      * Write the curve into docs/BUDGETS.md under a \`polish_latency\` heading,
        with the machine it was measured on named.
      * Derive the per-chunk word budget for Y4-E FROM THE CURVE, not from 400,
        and state the derived number in BUDGETS.md so Y4-E can cite it.
      * Add the ceiling this loop was missing: peak resident bytes with the ASR
        engine and the polish child both loaded (Y3-G asserts it).

    Runs immediately after Y4-A and BEFORE SEC-C and Y4-E, both of which are
    currently specced against numbers nobody took.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/polish_envelope.rs
    grep -q 'p95' desktop/src-tauri/tests/polish_envelope.rs
    grep -q 'skipped_with_a_named_reason_when_the_weights_are_absent' desktop/src-tauri/tests/polish_envelope.rs
    grep -q 'polish_latency' docs/BUDGETS.md
    grep -qE '[0-9]+ ?ms' docs/BUDGETS.md
    grep -q 'per_chunk_word_budget' docs/BUDGETS.md
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test polish_envelope ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'SEC-C', prompt: 'Y4', branch: 'loop/sec-c-polish-model-is-actually-installable', gated: null,
  title: 'The polish model gets an install path, so the LLM stage can exist on a real machine',
  preflight: `
    grep -q 'polish' desktop/src/ModelSetup.tsx
    grep -q 'polish' desktop/src/Onboarding.tsx
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test polish_model_install
  `,
  spec: `
    \`models::polish_models()\` and a catalog exist (src-tauri/src/catalog.json,
    models.rs 762 lines, referenced by lib.rs:264). The sidecar exists and is
    bundled (\`bundle.externalBin: ["binaries/yap-polish", ...]\`,
    tauri.conf.json). \`SidecarPool\` handles the warm child, the readiness
    handshake and a restart budget (polish.rs:26-33, YV75 — and its doc records
    that a missing handshake made a cold child "indistinguishable from a wedged
    one", so this path has already been debugged once).

    What does not exist is any way for a user to get the WEIGHTS. Onboarding
    never mentions polish (STEP_ORDER, Onboarding.tsx:41) and
    \`polish_model: ""\` is the permanent default. So the entire LLM formatting
    stage is dead code on every installed copy.

    RUNTIME-DEPENDENCY RULE (the Yap rule, and the reason this is its own item):
    nothing may be "found on the machine". The model is either SHIPPED in the
    bundle or its install is MANAGED by the app. A GGUF is too large to ship, so
    it is managed:
      * Surface the polish model in \`ModelSetup.tsx\` next to the ASR model, with
        its size, what it changes, and that it is optional. Reuse the existing
        download machinery (\`models.rs\` already downloads, verifies and reports
        \`native_ready\`; the catalog already pins digests — check
        catalog.json for the polish entries and add them if absent, with a
        digest, never a bare URL).
      * A resumable, cancellable download with a digest check on completion. A
        half-downloaded GGUF that loads and produces garbage is worse than no
        model, and \`validate_polish\` (polish.rs) would then be the only thing
        between garbage and the user's document.
      * On success, set \`polish_model\` to the catalog id. That is the ONLY way
        it should ever become non-empty — never a hardcoded default.
      * If the model is absent, the stage no-ops exactly as it does today
        (lib.rs:264-267 documents this and \`polish_stage\`'s off-gate is already
        tested with zero model bytes). Assert that the no-op is unchanged.
      * Make the SIDECAR's absence diagnosable too: if the staged binary is
        missing (a broken build), say so once in the log with the expected path,
        rather than timing out every take against a deadline that can never be met.

    Tests \`tests/polish_model_install.rs\`, all with zero model bytes:
      * \`catalog_polish_entries_carry_a_digest\`
      * \`interrupted_download_is_not_marked_ready\`
      * \`digest_mismatch_is_rejected_and_deleted\`
      * \`absent_model_leaves_the_pipeline_byte_identical\` — the regression that
        matters most; compare against the rules-only output on the fixture corpus.
      * \`missing_sidecar_binary_is_reported_once_not_per_take\`

    What NOT to do:
      - Do NOT download a model during onboarding by default. It is optional and
        large; offering it is right, forcing it is not.
      - Do NOT set \`polish_model\` to a catalog id that is not verified present
        on disk. That is how a default becomes a silent 1200 ms timeout on
        every take.
      - Do NOT add a network call anywhere on the dictation path.
    PANEL 2026-09-12 — this item is aimed at the wrong half of the gap, and its
    headline acceptance passes on unmodified main.
      * The catalog is ALREADY done: src/catalog.json polish_models holds
        qwen2.5-1.5b-instruct-q4_k_m (1,117,320,736 B, sha256 pinned,
        recommended_rank 1) and qwen2.5-0.5b-instruct-q4_k_m (491,400,032 B,
        rank 2). \`grep -qE 'polish' catalog.json\` is green today. Drop the
        "add them if absent" instruction.
      * The REAL gap is narrower and unasserted: \`download_polish_model_with\`
        exists (models.rs:209) and there is NO Tauri command and ZERO frontend
        references to polishModel anywhere in desktop/src
        (\`grep -rn 'polishModel\\|polish_model' desktop/src/*.tsx\` -> 0;
        \`grep -c polish ModelSetup.tsx\` -> 0). The weights can be fetched by
        Rust and never by a user. Build: a registered command in the invoke
        handler, a ModelSetup row, and a test that \`polish_model\` becomes
        non-empty ONLY after a digest-verified file exists on disk.
      * ADD A FREE-SPACE PRECONDITION and a named refusal. A 1.12 GB download
        with no disk check and no low-RAM behaviour is how "optional polish"
        becomes "polish silently does nothing" on exactly the machines least
        able to tell.
      * DO NOT offer the 0.5B as a tier yet. MEASURED (see Y4-I): it returns
        err=max_out at 100, 150, 200, 300 and 400 words, i.e. it is worse than
        no model. Ship the 1.5B only; the fast tier is deferred until the
        measured curve says otherwise.
      * The readiness budget is DERIVED, not measured: polish.rs
        SIDECAR_READY_BUDGET = 10 s, and cold ready for the 1.5B measured 7,323
        ms on an M4 Pro. A base M-series Air paging in the same 1.12 GB will
        cross 10 s and be declared "stuck" while it is merely loading. Take the
        number from Y4-I's measurement, or make "model loading" a real pill
        state (Y5-C) so lateness is reported instead of guessed.

  `,
  acceptance: `
    grep -q 'polish' desktop/src/ModelSetup.tsx
    grep -qE 'polish' desktop/src-tauri/src/catalog.json
    test -f desktop/src-tauri/tests/polish_model_install.rs
    grep -q 'absent_model_leaves_the_pipeline_byte_identical' desktop/src-tauri/tests/polish_model_install.rs
    grep -q 'digest_mismatch_is_rejected_and_deleted' desktop/src-tauri/tests/polish_model_install.rs
    grep -q 'missing_sidecar_binary_is_reported_once_not_per_take' desktop/src-tauri/tests/polish_model_install.rs
    grep -qE 'polish_model: String::new\\(\\)' desktop/src-tauri/src/lib.rs   # still never defaulted on
    cd ${APP} && npm ci && npm run build ; test $? -eq 0
    cd src-tauri
    cargo test --features custom-protocol --test polish_model_install ; test $? -eq 0
    cargo test --features custom-protocol --test formatting_fixtures  ; test $? -eq 0    grep -rq 'polish_model' desktop/src-tauri/src/lib.rs
    grep -rq 'polish' desktop/src/ModelSetup.tsx
    grep -q 'refuses_when_there_is_not_enough_free_space' desktop/src-tauri/tests/polish_model_install.rs
    grep -q 'polish_model_is_set_only_after_a_digest_verified_file_exists' desktop/src-tauri/tests/polish_model_install.rs

  `,
})

ITEMS.push({
  id: 'Y4-C', prompt: 'Y4', branch: 'loop/y4-c-paragraphing-the-rule-that-does-not-exist', gated: null,
  title: 'Paragraphing: long speech becomes paragraphs by rule, not one wall of text',
  preflight: `
    grep -q 'fn paragraph_breaks\\|fn insert_paragraphs' desktop/src-tauri/src/dictation.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test paragraphing
  `,
  spec: `
    Wilson named paragraphing specifically. The rules stage does list detection
    and spoken marks (dictation.rs's \`format_dictation\` +
    \`apply_spoken_marks\`); it has no paragraph rule at all —
    \`git grep -n "paragraph" desktop/src-tauri/src/dictation.rs\` finds the
    spoken COMMAND ("new paragraph") and no automatic rule. A five-minute
    dictation therefore arrives as one unbroken block, which is the single most
    visible way formatting "does not work".

    Build it as a PURE rule with real inputs, not a guess:
      * \`insert_paragraphs(text, &[PauseSpan], mode, style) -> String\`.
      * The strongest signal available is SILENCE, and Yap already measures it:
        \`vad.rs\` (803 lines, Silero VAD) and the RMS series at record.rs:881.
        The ASR path also carries timestamps (\`max_timestamp_kind\`,
        asr_engine.rs:382). Thread the pause spans from capture through to the
        rules stage — today they are computed and thrown away. A pause over a
        threshold at a sentence boundary is a paragraph break; a pause mid-clause
        is not.
      * Secondary, when timing is unavailable (a recovered or imported take):
        sentence count and discourse markers ("so", "anyway", "okay so",
        "moving on"). These must be a FALLBACK with its own test, not the
        primary mechanism — a marker-only rule breaks paragraphs on speech
        habits and is the reason so many dictation apps feel arbitrary.
      * Mode-aware, via the existing \`DictationMode\` (6 modes, \`mode_for_app\`):
        Email gets paragraphs and a blank line between them; chat modes get
        NONE — a paragraph break in Slack sends the message. That is a
        correctness bug, not a taste call. Assert it.
      * Idempotent: running the rule twice must produce the same string, and it
        must never touch text the user already broke with "new paragraph".

    Tests \`tests/paragraphing.rs\` plus new fixture rows in
    group-a-sentence-shape.jsonl at the shipped level:
      * \`pause_at_a_sentence_boundary_breaks\`
      * \`pause_mid_clause_does_not_break\`
      * \`chat_mode_never_inserts_a_break\`
      * \`explicit_new_paragraph_is_preserved_and_not_doubled\`
      * \`idempotent_on_second_application\`
      * \`marker_fallback_only_fires_without_timing\`
      * \`a_short_take_is_unchanged\` — the no-regression floor.

    Depends on Y4-A (the stage must actually run).

    What NOT to do:
      - Do NOT paragraph on a fixed word count. "Every 60 words" is the
        arbitrary behaviour users notice and hate.
      - Do NOT put this in the LLM stage. It must work with no model installed.
      - Do NOT break paragraphs inside a detected list.
  `,
  acceptance: `
    grep -q 'fn insert_paragraphs' desktop/src-tauri/src/dictation.rs
    test -f desktop/src-tauri/tests/paragraphing.rs
    grep -q 'chat_mode_never_inserts_a_break' desktop/src-tauri/tests/paragraphing.rs
    grep -q 'pause_mid_clause_does_not_break' desktop/src-tauri/tests/paragraphing.rs
    grep -q 'idempotent_on_second_application' desktop/src-tauri/tests/paragraphing.rs
    grep -q 'marker_fallback_only_fires_without_timing' desktop/src-tauri/tests/paragraphing.rs
    grep -q 'paragraph' desktop/src-tauri/tests/fixtures/formatting/group-a-sentence-shape.jsonl
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test paragraphing        ; test $? -eq 0
    cargo test --features custom-protocol --test formatting_fixtures ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y4-D', prompt: 'Y4', branch: 'loop/y4-d-lists-and-punctuation-at-the-shipped-level', gated: null,
  title: 'Lists, nesting and spoken punctuation proven at the level the product ships, with the gaps filled',
  preflight: `
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test formatting_fixtures
    test 0 -lt "$(grep -h '"level":"medium"' tests/fixtures/formatting/group-b-lists.jsonl | wc -l)"
  `,
  spec: `
    \`group-b-lists.jsonl\` exists and the list detector exists
    (\`runs_format\` -> \`format_dictation\` "(list detection)",
    dictation.rs:636/:652). What is unproven is behaviour at the shipped level
    and the cases a real dictation hits.

    Audit the fixture file first and write down, in the PR body, which of these
    is already covered and which is new. Then cover every gap:
      * "one, two, three" as PROSE vs "number one ... number two" as a LIST.
        Over-eager list detection is the failure users report as "it keeps
        making bullets out of my sentences".
      * Ordered vs unordered; a list that starts mid-paragraph; a list of ONE
        item (must not become a list).
      * Nesting one level ("sub-bullet" / "indent"), and the explicit decision
        NOT to support deeper nesting if that is the call — write the decision
        into the doc comment either way.
      * Spoken marks in a list item without terminating the item.
      * The full spoken-punctuation table: period, comma, question mark,
        exclamation point, colon, semicolon, dash, quotation mark (which
        ALTERNATES open/close — group-a line 4 already pins this pairing
        behaviour, extend it to three pairs in one utterance), apostrophe,
        parentheses, at symbol, percent, degree. \`formatting_fixtures.rs:217\`
        lists the mark vocabulary the corpus knows; make sure the fixture rows
        exercise it rather than the constant merely naming it.
      * The literal-escape case: how does a user type the WORD "period"? There
        must be an answer and it must be tested. If the answer is "say 'literal
        period'", implement it; if it is "we accept the ambiguity", write that
        in the doc comment so the next engineer does not re-litigate it.
      * Capitalization after a spoken period; no capitalization after an
        abbreviation ("e.g.", "Dr.").
      * R5 lead casing from caret context must still win. polish.rs:35-41
        explains at length that \`dictation::join_with_context\` runs AFTER
        \`run_cleanup\` so the context's casing decision lands last, asserted by
        \`polish_never_overrides_the_r5_lead_case\`. Keep that test green and add
        the rules-stage equivalent.

    Every new case is a FIXTURE ROW at \`"level":"medium"\`, not a bespoke unit
    test, so the corpus stays the single source of truth
    (tests/fixtures/README.md documents the corpus conventions — follow them).

    Depends on Y4-A.

    What NOT to do:
      - Do NOT add a case by relaxing an existing expectation.
      - Do NOT make list detection LLM-dependent. It is a rules-stage feature
        and must work with no model.
  `,
  acceptance: `
    # PANEL: a medium-row COUNT is already 36 on main — vacuous. The bar is
    # per-rule coverage at the shipped level, enumerated from the rule table.
    grep -q 'every_rule_has_a_fixture_row_at_the_shipped_level' desktop/src-tauri/tests/shipped_defaults.rs
    grep -q 'every_rule_id_is_covered_and_names_the_missing_ones' desktop/src-tauri/tests/formatting_fixtures.rs
    grep -q 'literal' desktop/src-tauri/tests/fixtures/formatting/group-a-sentence-shape.jsonl
    grep -q 'sub-bullet\\|indent' desktop/src-tauri/tests/fixtures/formatting/group-b-lists.jsonl
    grep -q 'single_item' desktop/src-tauri/tests/fixtures/formatting/group-b-lists.jsonl
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test formatting_fixtures ; test $? -eq 0
    cargo test --features custom-protocol polish_never_overrides_the_r5_lead_case ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y4-E', prompt: 'Y4', branch: 'loop/y4-e-polish-long-form-by-chunking', gated: null,
  title: 'Long-form gets polished: the 400-word cliff becomes a chunked pass that keeps the deadline',
  preflight: `
    test 0 -eq "$(grep -c 'MAX_POLISH_WORDS: usize = 400' desktop/src-tauri/src/polish.rs)"
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test polish_long_form
  `,
  spec: `
    MEASURED: \`polish.rs:73\` \`const MAX_POLISH_WORDS: usize = 400;\` with the
    doc at :71-72: "Long-form is rules-only: chunking is a later item, and a
    single pass over 400+ words cannot hold the deadline (§2.3)." So the longest
    and most valuable dictations — the ones Wilson actually does — get the least
    formatting. The deadline is \`DEFAULT_POLISH_DEADLINE_MS = 1200\`
    (polish.rs:59), bounded 100..5000 (:64-65).

    Do:
      * Chunk the polish pass at PARAGRAPH boundaries (Y4-C now produces them)
        with a small overlap of context, each chunk under the per-chunk word
        budget, each with its own slice of the deadline. The whole-take deadline
        scales with chunk count; do NOT give one long take 1200 ms total.
      * \`validate_polish\` (polish.rs, V1-V7) runs PER CHUNK. A rejected chunk
        keeps that chunk's rules text and the rest still benefits. Today a
        rejection loses the whole rewrite; per-chunk rejection is strictly
        better and preserves the never-lose-text property, which is the module's
        stated reason to exist (polish.rs:4-8).
      * The content-word retention floor (\`RETENTION_FLOOR = 0.80\`) and the
        length band (\`MAX_LENGTH_RATIO 2.5\` / \`MIN_LENGTH_RATIO 0.45\`) are
        computed per chunk AND across the whole take. A chunk-wise pass that
        each time stays inside the band can still drift the document; assert
        the aggregate.
      * Seam hygiene: a chunk boundary must not duplicate a sentence, must not
        capitalize mid-sentence, and must not drop the join whitespace. This is
        the same class of bug Y3-B's ASR seams have; write the test, do not
        assume.
      * \`SidecarPool\`'s restart budget (YV75) must not be exhausted by a
        12-chunk take. Assert the budget is per-take-scaled or per-chunk-reset,
        and say which.
      * Keep the off/too-short gates exactly as they are: under
        \`MIN_POLISH_WORDS = 4\` still costs no round trip.

    Tests \`tests/polish_long_form.rs\`, driven through the injectable
    \`PolishClient\` seam (polish.rs:31-33 — the tests already inject a client
    that sleeps past the deadline, one that panics, one that returns garbage;
    reuse all three at length):
      * \`a_two_thousand_word_take_is_polished_not_skipped\`
      * \`one_rejected_chunk_keeps_only_that_chunk_raw\`
      * \`aggregate_retention_floor_holds_across_chunks\`
      * \`seam_does_not_duplicate_or_drop_a_sentence\`
      * \`deadline_scales_with_chunk_count\`
      * \`a_panicking_client_on_chunk_seven_still_returns_the_whole_document\`
      * \`short_take_path_is_byte_identical\`

    Depends on SEC-C (a model to run), Y4-C (paragraph boundaries), Y3-B
    (chunking precedent to follow — reuse its geometry helpers where they fit).

    What NOT to do:
      - Do NOT raise the deadline instead of chunking.
      - Do NOT abandon per-chunk validation to save time. The validator is the
        only thing standing between an untrusted local model and the user's
        document; polish.rs:3-5 is explicit that sidecar output is UNTRUSTED.
      - Do NOT re-apply R5 lead casing inside polish. polish.rs:35-41 explains
        why that is deliberately absent.
    ── PANEL 2026-09-12, BINDING: build this against MEASURED numbers ──────
    One seat measured the stage this item chunks. On an M4 Pro with an
    effectively unlimited deadline, the 1.5B returns a usable rewrite only up to
    ~150 words and hard-errors (err=max_out, which for KIND_POLISH DISCARDS the
    whole rewrite — yap-polish/src/main.rs:544-550) at 200 and 400 words; the
    0.5B errors at every length from 100 words up. At the shipped
    DEFAULT_POLISH_DEADLINE_MS = 1200 every input over ~150 words fails, and
    MAX_POLISH_DEADLINE_MS = 5000 is itself BELOW the measured 400-word cost
    (5,671 ms). So this item's seven tests — all of which drive the injectable
    PolishClient seam with fakes, including \`deadline_scales_with_chunk_count\`
    — can be green while the real model never once meets the deadline.
      * Y4-I runs FIRST and writes the curve into docs/BUDGETS.md. Take the
        per-chunk word budget FROM THAT CURVE, not from MAX_POLISH_WORDS = 400,
        and cite the number in a comment.
      * Raise MAX_POLISH_DEADLINE_MS above the measured per-chunk cost, or the
        deadline discipline is decorative. (This is not "raise the deadline
        instead of chunking" — it is making the ceiling bigger than the floor.)
      * A max_out overrun stops being a discard: keep the RULES text for that
        chunk, keep the successful chunks, and record the reason. A 2,000-word
        take chunked to a 150-word envelope is 13-14 sequential round trips
        (~17 s after the hold ends on the fastest Apple laptop silicon), so the
        per-chunk fallback is the difference between late and lost.
      * At least ONE test must drive the real sidecar, skipping with a NAMED
        reason when the weights are absent. Fakes cannot see this failure.

  `,
  acceptance: `
    test 0 -eq "$(grep -c 'MAX_POLISH_WORDS: usize = 400' desktop/src-tauri/src/polish.rs)"   # 0 (was 1)
    grep -q 'POLISH_CHUNK' desktop/src-tauri/src/polish.rs
    test -f desktop/src-tauri/tests/polish_long_form.rs
    grep -q 'a_two_thousand_word_take_is_polished_not_skipped' desktop/src-tauri/tests/polish_long_form.rs
    grep -q 'one_rejected_chunk_keeps_only_that_chunk_raw' desktop/src-tauri/tests/polish_long_form.rs
    grep -q 'aggregate_retention_floor_holds_across_chunks' desktop/src-tauri/tests/polish_long_form.rs
    grep -q 'a_panicking_client_on_chunk_seven_still_returns_the_whole_document' desktop/src-tauri/tests/polish_long_form.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test polish_long_form    ; test $? -eq 0
    cargo test --features custom-protocol --test formatting_fixtures ; test $? -eq 0    grep -q 'per_chunk_word_budget' docs/BUDGETS.md
    grep -q 'a_max_out_chunk_keeps_its_rules_text_and_says_so' desktop/src-tauri/tests/polish_long_form.rs
    grep -q 'skipped_with_a_named_reason_when_the_weights_are_absent' desktop/src-tauri/tests/polish_long_form.rs

  `,
})

ITEMS.push({
  id: 'Y4-F', prompt: 'Y4', branch: 'loop/y4-f-app-aware-formatting-that-is-actually-applied', gated: null,
  title: 'App-aware formatting: the six modes change the output, proven per app, and auto mode picks correctly',
  preflight: `
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test app_aware_formatting
  `,
  spec: `
    \`mode_for_app\` and 6 \`DictationMode\`s exist, the shipped default is
    \`dictation_mode: "auto"\` (lib.rs:415), \`focus.rs\` (342 lines) resolves the
    frontmost app, and \`polish_styles\` is a per-mode tone map (lib.rs:279).
    reference_wispr_parity_research §2.4 scores Yap ✅ on app-category detection.
    What is NOT proven is that the mode changes the OUTPUT at the shipped
    configuration, and Wilson's complaint says it does not.

    Do:
      * A table test over the six modes with ONE input utterance, asserting the
        outputs are pairwise DIFFERENT where the rules say they should be and
        IDENTICAL where they should not. That single assertion is what catches
        "mode is detected, threaded, and then ignored" — which is the shape of
        a defect nothing currently tests for.
      * Email: greeting/sign-off shape (R13, \`format_email_shape\`), paragraphs,
        full punctuation, trailing period per the tone dial.
      * Chat / work chat: no paragraph breaks (a newline sends), no sign-off,
        trailing period per R3/R14's casual setting, emoji left alone.
      * Code / terminal context: NO spoken-mark expansion of characters that are
        syntax, no auto-capitalization, no trailing period. If there is no such
        mode today, this item adds it — Wilson lives in Claude Code and Cursor
        (reference_wispr_parity_research P1 #12 calls IDE context "the highest
        personal-ROI item on the list"), and a dictation that capitalizes his
        identifiers is worse than no dictation.
      * Document / notes: paragraphs, lists, full punctuation.
      * \`auto\` resolution: a fixture table of real bundle identifiers
        (com.apple.mail, com.tinyspeck.slackmacgap, com.google.Chrome,
        com.microsoft.VSCode, com.apple.Terminal, com.apple.TextEdit) mapping to
        modes, with an explicit DEFAULT for an unknown bundle id, and a test
        that the default is the CONSERVATIVE mode — an unknown app must not get
        the most aggressive formatting.
      * Browser ambiguity is real (Gmail in Chrome is email; Google Docs in
        Chrome is a document). \`AXSelectedText\`/URL resolution is P1 #11 in the
        parity backlog and is OUT of scope here. State the limitation in the doc
        comment and make the browser's default mode the conservative one so the
        wrong guess is cheap.

    Tests \`tests/app_aware_formatting.rs\` + fixture rows carrying a \`mode\`
    field (the corpus already has \`"mode":"document"\` — group-a line 4 — so the
    axis exists; populate it).

    Depends on Y4-A, Y4-C.

    What NOT to do:
      - Do NOT read the browser URL in this item.
      - Do NOT make an unknown app default to Email. A sign-off appearing in a
        random text field is the kind of thing a user uninstalls over.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/app_aware_formatting.rs
    grep -q 'unknown_bundle_id_gets_the_conservative_mode' desktop/src-tauri/tests/app_aware_formatting.rs
    grep -q 'chat_mode_output_differs_from_email_mode_output' desktop/src-tauri/tests/app_aware_formatting.rs
    grep -q 'com.microsoft.VSCode' desktop/src-tauri/tests/app_aware_formatting.rs
    grep -q '"mode":"chat"' desktop/src-tauri/tests/fixtures/formatting/group-a-sentence-shape.jsonl
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test app_aware_formatting ; test $? -eq 0
    cargo test --features custom-protocol --test formatting_fixtures  ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y4-G', prompt: 'Y4', branch: 'loop/y4-g-formatting-is-visible-and-reversible', gated: null,
  title: 'The user can see what formatting did and undo it in one key — the trust mechanism',
  preflight: `
    grep -rq 'DiffView\\|formatting_diff' desktop/src
    cd ${APP} && npm ci && npm test -- diff
  `,
  spec: `
    Formatting that silently rewrites what you said is only acceptable if you
    can see it and revert it. Yap stores \`raw_text\` already (YV10/51) and has
    \`undo_ai_edit_text\` plus a \`UNDO_AI_EDIT\` global binding
    (shortcuts.rs:98). There is no diff UI:
    \`git grep -n "DiffView\\|diff" desktop/src\` -> nothing.
    reference_wispr_parity_research §2.6 lists "Diff/'see what changed' viewer +
    undo + feedback" as ✅ Wispr / 🟡 Yap, and P1 #8 says it is "mostly UI"
    because raw_text is already stored — and that Wispr "found this necessary
    for trust".

    Do:
      * A pure diff module \`desktop/src/diff.ts\` (word-level, no dependency —
        a small Myers or LCS implementation; do NOT add a library for this, the
        CSP forbids CDN loads and the bundle should not grow for a 60-line
        algorithm) returning a token list of {equal|added|removed}.
      * A "see what changed" panel in History and in the post-take surface,
        showing raw -> formatted, with the stages that ran named (dictionary /
        backtrack / rules / polish) so a surprising change is attributable to a
        stage. The stage list is already knowable from \`CleanupLevel\` and the
        polish outcome; thread it through as metadata on the take.
      * Undo restores BYTE-IDENTICAL raw text through the clipboard path. The
        parity backlog's acceptance for this is exactly that
        ("undo restores byte-identical raw text through the clipboard path") and
        it must be asserted, not assumed — the paste path is receipt-sequenced
        (YV39) and a naive re-paste can interleave.
      * Local-only thumbs up/down per take, stored in SQLite, used for nothing
        yet. It costs nothing now and is the only way a future rules change can
        be evaluated against real dissatisfaction. Never transmitted.

    Tests: \`desktop/src/diff.test.ts\` (pure, incl. identical strings -> all
    equal, and a pathological case that must not be quadratic-blow-up slow on a
    2,000-word take), and \`tests/undo_restores_raw.rs\` asserting byte-identical
    restoration including trailing whitespace and the signature block, which
    \`snippets::append_signature\` copies BYTE FOR BYTE after polish (lib.rs:283)
    and which undo must therefore also handle exactly.

    Depends on Y4-A.

    What NOT to do:
      - Do NOT add a diff library.
      - Do NOT send feedback anywhere. Local, forever. No Sentry, no PostHog.
      - Do NOT make undo re-run the pipeline. It replays a stored string.
    PANEL 2026-09-12 — two additions, and one word deleted.
    (a) THE SILENT-SKIP SIGNAL. Both polish failure modes are invisible by
        construction: a missed deadline returns err="deadline" and the parent
        falls back to rules text (polish.rs:299-300), and a max_out overrun
        discards the rewrite outright. As measured (Y4-I) those are the NORMAL
        outcomes for any take over ~150 words. So a user who installs a 1.12 GB
        model and sets High gets rules-only output on every real dictation with
        no indication the model was consulted — which is exactly how the current
        defect stayed invisible for a release cycle. Every take therefore
        records WHICH stages ran, and when the LLM stage was enabled but
        produced nothing, the REASON (deadline / max_out / no model / no
        sidecar). Show it in this item's diff view and once, quietly, on the
        pill. One counter per reason in the local log.
    (b) "the post-take surface" is deleted from the spec: THERE IS NO SUCH
        SURFACE. After \`done\` the pill is a small non-activating NSPanel with
        no text region (float_pill.rs:343-371) and no window appears. This item
        is scoped to History plus the existing ⌃⌘Z undo. An in-the-moment diff
        panel needs real geometry (size per dock, dismiss rules, how it avoids
        stealing focus from a non-activating panel, what happens if the user
        keeps typing) and is DEFERRED to docs/loop/DEFERRED.md rather than left
        as prose a builder has to invent.

  `,
  acceptance: `
    test -f desktop/src/diff.ts
    test -f desktop/src/diff.test.ts
    grep -rq 'formatting_diff\\|DiffView' desktop/src
    test -f desktop/src-tauri/tests/undo_restores_raw.rs
    grep -q 'undo_restores_byte_identical_raw_including_signature' desktop/src-tauri/tests/undo_restores_raw.rs
    # no new frontend dependency was added for the diff
    node -e "const d=require('./desktop/package.json').dependencies;process.exit(Object.keys(d).some(k=>/diff|jsdiff|myers/.test(k))?1:0)"
    cd ${APP} && npm ci
    npm test -- diff ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build    ; test $? -eq 0
    cd src-tauri && cargo test --features custom-protocol --test undo_restores_raw ; test $? -eq 0    grep -q 'a_skipped_llm_stage_records_its_reason' desktop/src-tauri/tests/formatting_diff.rs
    grep -rq 'stages_that_ran' desktop/src-tauri/src

  `,
})

ITEMS.push({
  id: 'Y4-H', prompt: 'Y4', branch: 'loop/y4-h-formatting-settings-a-person-can-use', gated: null,
  title: 'The formatting settings stop being engineer words, and every one of them persists',
  preflight: `
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test settings_kv
    cargo test --features custom-protocol --test formatting_settings_persist
  `,
  spec: `
    Two problems, one screen.

    (1) THE NAMES. The formatting controls are \`cleanup_level\`
    (none|light|medium|high), \`polish_model\`, \`polish_deadline_ms\`,
    \`polish_styles\` (a per-mode map of very casual|casual|default|formal),
    \`snippet_scope\`, \`signature_mode\` (off|cue|auto). Every one of them is
    named for its implementation. A user cannot tell what "high" does, that
    "high" needs a downloaded model, or that \`polish_deadline_ms\` exists at all.
    Restructure the screen around what the user wants — how much should Yap
    clean up? should it use the local model? how should it sound per app? —
    with each option's one-line description derived from the same \`runs_*\`
    predicates Y4-A exposed, so copy cannot drift from behaviour. Controls
    first, prose last (feedback_think_ux_first).

    (2) PERSISTENCE. The 2026-07-24 senior audit's top user-facing bug was
    "half of Settings never persist (incl. PTT remap)" (project_yap_open_bugs),
    addressed as M6 in yap8. \`tests/settings_kv.rs\` exists. What does NOT exist
    is a test that every field of \`AppSettings\` round-trips. Write the
    exhaustive one: for each field, set a non-default value, restart the store,
    read it back, assert equality — and make it FAIL when a new field is added
    without being included, by asserting the field count against a constant
    that a new field forces you to update. A serde-driven round-trip over a
    fully-populated struct is the cheapest form of this; take that shape.
      * Include \`polish_styles\` (a BTreeMap — maps are the classic
        never-persisted field) and \`calibration_sample\` (an Option).
      * Assert \`schema_version\` migration: an old settings blob missing the new
        fields loads with the documented defaults and does not reset the fields
        it does carry.

    Tests \`tests/formatting_settings_persist.rs\` + keep settings_kv.rs green.

    Depends on Y4-A, SEC-C.

    What NOT to do:
      - Do NOT expose \`polish_deadline_ms\` as a raw millisecond number field. If
        it stays user-visible it is a named choice (Fast / Balanced / Careful)
        mapped to the existing bounded range (100..5000, polish.rs:64-65).
      - Do NOT drop any setting while renaming its label. Rename the LABEL, keep
        the KEY, or migrate it with the schema version.
      - Do NOT overinvest in the settings surface beyond making it correct and
        legible (feedback_dont_overinvest_settings).
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/formatting_settings_persist.rs
    grep -q 'every_app_settings_field_round_trips' desktop/src-tauri/tests/formatting_settings_persist.rs
    grep -q 'polish_styles' desktop/src-tauri/tests/formatting_settings_persist.rs
    grep -q 'field_count_forces_this_test_to_be_updated' desktop/src-tauri/tests/formatting_settings_persist.rs
    grep -q 'old_blob_migrates_without_resetting_carried_fields' desktop/src-tauri/tests/formatting_settings_persist.rs
    cd ${APP} && npm ci && npm run build ; test $? -eq 0
    cd src-tauri
    cargo test --features custom-protocol --test formatting_settings_persist ; test $? -eq 0
    cargo test --features custom-protocol --test settings_kv                 ; test $? -eq 0
  `,
})

// ── 15-y3-long-dictations.mjs ─────────────────────────────────────────────
// Y3 — LONG DICTATIONS. Wilson, 2026-09-12, verbatim: "no handling for long
// dictations."
//
// AUDIT — the dictation path is one-shot from end to end. There is no chunking,
// no streaming, no memory bound, no progress, and a hard wall at 120 seconds of
// DECODE that silently kills the whole take:
//
//   src-tauri/src/transcription.rs:54
//     pub const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(120);
//   its own doc, :51-53: "Way above any real clip (a 60 s take is ~1 s on
//   Metal) — this exists only so a wedged native call surfaces as an error".
//   That reasoning holds for a 60 s take and fails for a 15 minute one.
//
//   src-tauri/src/lib.rs:1350   manager.transcribe(samples, language, bias_prompt)
//     ONE call, the ENTIRE take's samples, no windowing.
//
//   src-tauri/src/record.rs:2368  self.mono.extend_from_slice(interleaved)
//   src-tauri/src/record.rs:2377  self.raw.extend_from_slice(&self.mono)
//     TWO unbounded Vec<f32> for the whole take. At 48 kHz stereo a 15-minute
//     dictation is ~345 MB in `raw` alone before the 16 kHz downsample, and
//     both live until finalize_take (record.rs:2445).
//
//   src-tauri/src/polish.rs:73
//     const MAX_POLISH_WORDS: usize = 400;
//   its own doc, :71-72: "Long-form is rules-only: chunking is a later item,
//   and a single pass over 400+ words cannot hold the deadline". That later
//   item is THIS one. A long dictation gets no formatting at all, which is the
//   other half of Wilson's formatting complaint.
//
// THE MACHINERY ALREADY EXISTS — for MEETINGS, not for dictation:
//   meeting_asr.rs + transcription.rs chunk geometry, MAX_SANCTIONED_CHUNK_DECODE_SECONDS
//   (transcription.rs:101), seam dedupe via max_timestamp_kind (asr_engine.rs:382),
//   rtring.rs (YV91 lock-free SPSC ring, whose doc at rtring.rs:6-10 says in as
//   many words: "A five-second dictation never exposes that. Three continuous
//   hours on a meeting ... does"), the capture journal + recovery dir (YV63),
//   and asr_engine.rs:130 which "cancels a meeting chunk whenever a dictation
//   wants the engine".
//   Y3 is largely a REUSE job, not an invention job. Anything Y3 builds that
//   meeting_asr.rs already does is a defect in Y3.
//
// Wispr's comparable number, for calibration: max session length 20 min, raised
// from 5 (reference_wispr_parity_research §2.1 [OFFICIAL]).
//
// SHARED PREAMBLE + STANDARD GATE: see 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'Y3-A', prompt: 'Y3', branch: 'loop/y3-a-bounded-capture-spill-to-disk', gated: null,
  title: 'Capture stops growing two unbounded Vecs — a long take spills to disk with a measured memory ceiling',
  preflight: `
    test 0 -eq "$(grep -c 'self.raw.extend_from_slice(&self.mono)' desktop/src-tauri/src/record.rs)"
    grep -q 'SpillWriter\\|spill_to_disk' desktop/src-tauri/src/record.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test dictation_capture_memory
  `,
  spec: `
    MEASURED: \`record.rs:2280-2284\` holds \`out\`, \`raw\` and \`mono\` as
    \`Vec<f32>\`; \`:2368\` appends every interleaved callback to \`mono\` and
    \`:2377\` appends \`mono\` to \`raw\`. Nothing bounds either. \`finalize_take\`
    (:2445) then takes the whole thing by value.

    There is already a proven pattern in this repo for exactly this problem and
    it must be reused rather than re-derived: \`tests/meeting_capture_memory.rs\`
    asserts a memory ceiling for the meeting path, and \`rtring.rs\` is the
    real-time-safe hand-off built once "so 22-B reuses it verbatim rather than
    growing a second copy" (rtring.rs:16-18). Dictation is now the third
    consumer.

    Do:
      * The capture consumer writes finalized PCM to the take's WAV incrementally
        (a \`SpillWriter\` over the existing \`recordings\` dir path, which
        \`start_recording\` already passes in at lib.rs:1142) and keeps only a
        bounded working window in RAM.
      * Ceiling: a named constant \`MAX_RESIDENT_CAPTURE_BYTES\`, set so that a
        take of ANY length holds under ~32 MB resident for the audio buffers.
        Pick the window from the DSP's actual lookahead needs (high_pass,
        normalize_rms, denoise all operate blockwise — read record.rs:2483-2600
        and size the window to the largest true dependency, then say the number
        in the doc comment).
      * \`finalize_take\` reads back from the spill instead of taking a giant Vec.
        \`read_wav_16k_mono\` (record.rs:826) already exists for the read side.
      * RNNoise denoise (\`denoise: true\` is the shipped default, lib.rs:419)
        must still run, blockwise, with identical output on a short clip. That is
        the regression that would be easy to miss: assert byte-identical output
        against today's path for the existing fixture
        \`tests/fixtures/quick-brown-fox-16k.wav\`.
      * The capture journal (YV63) and recovery dir keep working unchanged — a
        take that dies mid-spill must still be recoverable, and a spilled WAV is
        strictly easier to recover than a lost Vec.

    Tests \`tests/dictation_capture_memory.rs\`:
      * \`resident_bytes_are_bounded_across_a_synthetic_twenty_minute_take\` —
        feed 20 minutes of generated frames through the consumer and assert peak
        resident buffer bytes < MAX_RESIDENT_CAPTURE_BYTES. Model it on
        meeting_capture_memory.rs so the two agree on how "resident" is counted.
      * \`short_take_output_is_byte_identical_to_the_pre_spill_path\` — against
        the committed fixture WAV.
      * \`spill_survives_a_mid_take_abort_and_lands_in_the_recovery_dir\`.
      * \`the_audio_callback_still_never_allocates\` — reuse the assertion shape
        from tests/meeting_capture_rt_safety.rs; do not write a second one.

    What NOT to do:
      - Do NOT move DSP into the audio callback to avoid buffering. rtring.rs's
        entire premise is that the callback copies and returns.
      - Do NOT change the WAV format or sample rate on disk. Other code reads it.
      - Do NOT bound by TRUNCATING audio. Dropping the end of a long dictation
        is the bug, not the fix.
  `,
  acceptance: `
    test 0 -eq "$(grep -c 'self.raw.extend_from_slice(&self.mono)' desktop/src-tauri/src/record.rs)"   # 0 (was 1)
    grep -q 'MAX_RESIDENT_CAPTURE_BYTES' desktop/src-tauri/src/record.rs
    test -f desktop/src-tauri/tests/dictation_capture_memory.rs
    grep -q 'resident_bytes_are_bounded_across_a_synthetic_twenty_minute_take' desktop/src-tauri/tests/dictation_capture_memory.rs
    grep -q 'short_take_output_is_byte_identical_to_the_pre_spill_path' desktop/src-tauri/tests/dictation_capture_memory.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test dictation_capture_memory ; test $? -eq 0
    cargo test --features custom-protocol --test meeting_capture_memory   ; test $? -eq 0
    cargo test --features custom-protocol --test capture_journal_recovery ; test $? -eq 0
    cargo test --features custom-protocol --test meeting_capture_rt_safety; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y3-B', prompt: 'Y3', branch: 'loop/y3-b-chunked-decode-for-the-dictation-path', gated: null,
  title: 'Long takes decode in windows with seam dedupe, reusing the meeting chunker instead of one 120s-capped call',
  preflight: `
    grep -q 'chunk' desktop/src-tauri/src/lib.rs
    grep -q 'DICTATION_CHUNK' desktop/src-tauri/src/transcription.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test dictation_chunked
  `,
  spec: `
    MEASURED: \`lib.rs:1350\` is \`manager.transcribe(samples, language,
    bias_prompt)\` — the whole take, one call — under
    \`TRANSCRIBE_TIMEOUT = 120s\` (transcription.rs:54). Whisper decode is
    roughly real-time-divided-by-N on Metal; the 120 s wall is generous for a
    minute of speech and fatal somewhere in the multi-minute range, and when it
    trips the user loses the ENTIRE take. There is no partial result to keep.

    Reuse, do not rebuild. Read these first and extract the shared piece:
      transcription.rs:86-110  the chunk-decode budget and
                               MAX_SANCTIONED_CHUNK_DECODE_SECONDS (:101) with
                               its own warning that "the two constants
                               disagreeing is precisely how a legal 37 s chunk
                               could ..." — read that comment in full.
      asr_engine.rs:382-394    max_timestamp_kind / max_audio_ms — "is there an
                               alignment to dedupe seams with at all?" and "is a
                               35 s window ..." This is the seam machinery.
      asr_engine.rs:221-236    Shift onto another timeline (a chunk's own start).
      meeting_asr.rs           the working chunker.

    Do:
      * Factor the chunk geometry + seam dedupe out of the meeting path into a
        module both callers use. If meeting_asr.rs's chunker is already
        general, call it; if it is entangled with meeting state, extract the
        pure part. Either way the outcome is ONE chunker with one set of tests.
      * Dictation decodes in windows with overlap, deduping at seams by
        timestamp when \`max_timestamp_kind\` supports it and by longest-common-
        suffix/prefix when it does not — and when NEITHER is available, it must
        fail loudly rather than silently double a phrase at every seam.
      * \`TRANSCRIBE_TIMEOUT\` becomes per-CHUNK, not per-take, so a long take has
        no wall at all. Keep a separate, generous whole-take deadline so a truly
        wedged engine still surfaces; derive it from chunk count so it scales.
      * Partial results are kept: if chunk 9 of 12 fails, the take returns the
        first 8 chunks' text, marks the take degraded, and preserves the clip in
        the recovery dir for a retry (the retry path exists — lib.rs:6230, YV66).
        Losing 11 minutes because minute 12 failed is the behaviour being deleted.
      * Short takes must take a byte-identical path to today. The single-window
        case has to produce the same bytes, or every formatting fixture and the
        YV66 hallucination-gate corpus shift under us for no reason.

    Tests \`tests/dictation_chunked.rs\`:
      * \`single_window_take_is_byte_identical_to_the_unchunked_path\` (fixture WAV).
      * \`seams_do_not_duplicate_or_drop_words\` — synthesize a known sentence
        stream across a seam and assert exact text.
      * \`a_failed_middle_chunk_keeps_the_earlier_chunks\`
      * \`per_chunk_deadline_not_per_take\` — assert a 12-chunk take is not
        subject to a 120 s total.
      * \`chunker_has_exactly_one_implementation\` — a call-site sweep asserting
        meeting and dictation resolve to the same function.
      * Keep \`tests/matrix_new_asr_chunk_timeout.rs\` green; it already covers
        the meeting side of a chunk timeout and must not regress.

    Depends on Y3-A (a spilled WAV is what the chunker reads).

    What NOT to do:
      - Do NOT fix this by raising TRANSCRIBE_TIMEOUT. A bigger wall is still a
        wall, and a 15-minute single-window decode also blows the memory ceiling
        Y3-A just established.
      - Do NOT let dictation chunks contend with a meeting for the engine.
        asr_engine.rs:130 already makes dictation PREEMPT meetings; preserve that
        priority and keep tests/meeting_dictation_preempts_transcription.rs green.
    PANEL 2026-09-12 — the cliff that justifies this item is not where the audit
    put it, so the scope narrows. MEASURED: a 601-second WAV (241x the
    quick-brown-fox fixture, 16 kHz mono) through the shipped headless path —
    \`./target/debug/wilson-voice --transcribe-file <601s.wav>\` — returned exit
    0 in 30.5 s with ~1,900 words. That is ~20x real time INCLUDING model load,
    consistent with the repo's own note (transcription.rs:51-54, "a 60 s take is
    ~1 s on Metal"), which puts the 120 s TRANSCRIBE_TIMEOUT near 45 minutes of
    audio, not four. What actually broke at ten minutes is what the plan ranked
    lower: 1,900 words arriving as one unbroken block (no paragraph rule, no
    polish past 400 words), with no progress and no cancel.
      * KEEP the narrow, high-value half: TRANSCRIBE_TIMEOUT becomes PER-CHUNK,
        and a trip keeps the chunks already decoded instead of losing the take.
      * Put the measurement in the pre-flight: a timed long-WAV decode row with
        the observed real-time factor, written to docs/BUDGETS.md, so Y3-F's
        declared maximum session length is derived from a number somebody took.
      * Refactoring the WORKING meeting chunker into a shared module is the one
        change in this lane that can regress a shipped subsystem. Do it only as
        far as the per-chunk timeout and the seam dedupe require, and say in the
        PR body what the meeting path's behaviour was before and after.
      * Y3-A (the 345 MB ceiling), Y4-C (paragraphing, rules-only) and
        Y3-C/Y3-D (progress and cancel) are what a ten-minute take needs; this
        item must not block them.

  `,
  acceptance: `
    grep -q 'DICTATION_CHUNK' desktop/src-tauri/src/transcription.rs
    test -f desktop/src-tauri/tests/dictation_chunked.rs
    grep -q 'single_window_take_is_byte_identical_to_the_unchunked_path' desktop/src-tauri/tests/dictation_chunked.rs
    grep -q 'seams_do_not_duplicate_or_drop_words' desktop/src-tauri/tests/dictation_chunked.rs
    grep -q 'a_failed_middle_chunk_keeps_the_earlier_chunks' desktop/src-tauri/tests/dictation_chunked.rs
    grep -q 'chunker_has_exactly_one_implementation' desktop/src-tauri/tests/dictation_chunked.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test dictation_chunked                          ; test $? -eq 0
    cargo test --features custom-protocol --test matrix_new_asr_chunk_timeout               ; test $? -eq 0
    cargo test --features custom-protocol --test meeting_dictation_preempts_transcription   ; test $? -eq 0
    cargo test --features custom-protocol --test formatting_fixtures                        ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y3-C', prompt: 'Y3', branch: 'loop/y3-c-long-take-progress-in-the-pill', gated: null,
  title: 'The pill reports real progress on a long take instead of an honest-looking lie',
  preflight: `
    grep -q '"transcribing"' desktop/src/pill/live.ts
    grep -q 'transcribe_progress' desktop/src-tauri/src/lib.rs
    cd ${APP} && npm ci && npm test -- pill/live
  `,
  spec: `
    Two separate honesty problems, both already named by Wilson.

    (1) THE ESCALATION IS A TIMER. project_yap_open_bugs BUG 2, resolved as
    YV17 but only partly: "the pill's live escalation was TIMER-based, not real
    word/sentence tracking ... it 'just moves along' regardless of what's
    actually said. Wilson wants it to react to ACTUAL words/sentences." The
    machinery still estimates: \`live.ts:44\`
    \`wordsFromVoiced = (voicedSec) => voicedSec * WORDS_PER_SEC\`, and
    \`liveTierForWords\` (:46) drives quick|notes|desk|essay|saga off that
    estimate. On a 10-minute take the estimate's error is enormous and the
    persona it promises is unearned. Y3-B's chunker gives the first real fix
    available: each completed chunk yields REAL text and a REAL word count
    mid-take. Feed it.

    (2) THERE IS NO PROGRESS AT ALL AFTER THE HOLD ENDS. ClassicPill.tsx tracks
    \`{recording, busy, message}\`; \`busy\` is a boolean. A 15-minute take
    decoding across 12 chunks shows the same undifferentiated busy state for
    minutes. project_yap_pill_vision names this exactly: "fill the dead time
    after talking stops and before text appears (transcribe/think gap)".

    Do:
      * Rust emits \`transcribe_progress { chunk, of, words_so_far }\` per
        completed chunk from Y3-B's loop. No timer, no interpolation — only real
        completions. Emit nothing for a single-window take (a short take has no
        progress to report and a flicker of 1/1 is worse than silence).
      * \`LivePhase\` gains \`"transcribing"\` as a first-class phase distinct from
        \`thinking\` (which is the polish/LLM gap), and carries
        \`{ chunk, of, wordsSoFar }\`.
      * Both pills render it: a determinate fill, the numeral in Departure Mono,
        and — this is the character beat — Yappy's live commentary escalates on
        \`wordsSoFar\` from real chunks instead of \`wordsFromVoiced\`. Keep
        \`wordsFromVoiced\` for the LISTENING phase only, where nothing better
        exists, and mark it in its doc comment as an estimate that the
        transcribing phase must not use.
      * Cap the claim: for a take still recording, the pill must never state a
        word count as fact. "Listening" states may be playful; numeric states
        may not be wrong.

    Tests in \`desktop/src/pill/live.test.ts\`:
      * \`transcribing_uses_real_chunk_words_not_the_voiced_estimate\`
      * \`single_window_take_emits_no_progress_phase\`
      * \`progress_is_monotonic_and_never_exceeds_of\`
      * \`listening_tier_still_uses_the_estimate\` (the deliberate exception).
    Rust: \`tests/transcribe_progress_events.rs\` — one event per completed
    chunk, zero for a single window, and none after the take ends.

    Depends on Y3-B.

    PR body owes a capture of the transcribing phase at 3/12 in both pill styles
    and at all three dock positions.

    What NOT to do:
      - Do NOT show an indeterminate spinner for a chunked take. The whole point
        is that the count is now knowable.
      - Do NOT interpolate progress between chunk completions to make the bar
        smooth. A smooth bar that is lying is the defect being removed.
  `,
  acceptance: `
    grep -q '"transcribing"' desktop/src/pill/live.ts
    grep -q 'transcribe_progress' desktop/src-tauri/src/lib.rs
    grep -q 'transcribing_uses_real_chunk_words_not_the_voiced_estimate' desktop/src/pill/live.test.ts
    grep -q 'single_window_take_emits_no_progress_phase' desktop/src/pill/live.test.ts
    grep -q 'progress_is_monotonic_and_never_exceeds_of' desktop/src/pill/live.test.ts
    test -f desktop/src-tauri/tests/transcribe_progress_events.rs
    cd ${APP} && npm ci
    npm test -- pill ; test $? -eq 0
    cd src-tauri && cargo test --features custom-protocol --test transcribe_progress_events ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y3-D', prompt: 'Y3', branch: 'loop/y3-d-cancel-a-long-take-at-any-stage', gated: null,
  title: 'Cancel works mid-decode, not just mid-recording — and a cancel never loses the audio',
  preflight: `
    grep -q 'CANCEL' desktop/src-tauri/src/shortcuts.rs
    grep -q 'cancel_transcription' desktop/src-tauri/src/lib.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test cancel_long_take
  `,
  spec: `
    \`cancel_recording\` exists (referenced at lib.rs:478: "cleared by
    \`cancel_recording\`") and stops a RECORDING. Once the hold ends and decode
    starts, there is nothing to press: a 15-minute take that the user
    immediately regrets occupies the engine to completion. And there is no
    cancel HOTKEY at all — \`shortcuts.rs:143 ALL\` is
    PASTE_LAST, UNDO_AI_EDIT, DICTATION_TOGGLE_LEGACY, MEETING_TOGGLE.
    Wispr ships a dedicated rebindable cancel key
    (reference_wispr_parity_research §2.1 [BUNDLE]
    \`settings_hotkey_dialog_cancel_*\`, listed as ❌ for Yap).

    Do:
      * \`shortcuts::CANCEL\` — a new Binding in the existing table (the table
        exists so bindings are declared once; shortcuts.rs:3-8 explains why).
        Default: Escape while a take is active, rebindable through the YV15
        capture control. It must not register a global Escape that eats Escape
        from every app: bind it only while a take is in flight, or scope it to
        the pill's own window plus a take-active global. State which and why.
      * \`cancel_transcription\`: cooperative cancellation checked at every chunk
        boundary in Y3-B's loop, plus the existing engine-cancel path
        (transcription.rs already has one — \`PREEMPTED_FOR_DICTATION\` :68 and
        \`ABANDONED_FOR_EXIT\` :84 prove chunk decode is already cancellable;
        reuse that mechanism, add a third reason \`CANCELLED_BY_USER\`).
      * A cancel NEVER deletes the clip. It parks it in the recovery dir with
        the same 7-day purge lifecycle (YV52/YV63) so "actually, transcribe that"
        is possible from History. A cancel that destroys 15 minutes of audio is
        a worse bug than the one being fixed.
      * A cancel pastes NOTHING and writes no transcript row. Assert both.
      * The pill shows a cancelled state that settles to idle — not an error.

    Tests \`tests/cancel_long_take.rs\`:
      * \`cancel_mid_recording_writes_no_row_and_pastes_nothing\`
      * \`cancel_mid_decode_stops_at_the_next_chunk_boundary\`
      * \`cancelled_clip_lands_in_the_recovery_dir\`
      * \`cancel_reason_is_distinguishable_from_preempted_and_abandoned\` — three
        distinct constants; a string compared against a literal in another
        module is the bug transcription.rs:74-77 already warns about.
      * \`escape_is_not_a_global_shortcut_while_idle\`

    Depends on Y3-B.

    What NOT to do:
      - Do NOT implement cancel by killing the engine process. The engine is
        in-process and shared with meetings; killing it takes a meeting down.
      - Do NOT reuse PREEMPTED_FOR_DICTATION as the cancel reason.
  `,
  acceptance: `
    grep -q 'pub const CANCEL' desktop/src-tauri/src/shortcuts.rs
    grep -q 'CANCELLED_BY_USER' desktop/src-tauri/src/transcription.rs
    grep -q 'fn cancel_transcription' desktop/src-tauri/src/lib.rs
    test -f desktop/src-tauri/tests/cancel_long_take.rs
    grep -q 'cancelled_clip_lands_in_the_recovery_dir' desktop/src-tauri/tests/cancel_long_take.rs
    grep -q 'cancel_reason_is_distinguishable_from_preempted_and_abandoned' desktop/src-tauri/tests/cancel_long_take.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test cancel_long_take        ; test $? -eq 0
    cargo test --features custom-protocol --test tray_hotkey_no_collision; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'DB-B', prompt: 'Y3', branch: 'loop/db-b-crash-recovery-for-a-long-take', gated: null,
  title: 'A crash or quit mid-long-take loses nothing — the take resumes or is offered back on next launch',
  preflight: `
    grep -q 'resume_take\\|recover_dictation' desktop/src-tauri/src/lib.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test long_take_recovery
  `,
  spec: `
    The pieces exist and are not connected for the dictation path:
      * YV63 capture journal + recovery dir (\`recovery_dir()\`, lib.rs:1145),
        with \`tests/capture_journal_recovery.rs\` green.
      * The meeting path re-decodes abandoned chunks on next launch
        (transcription.rs:70-84: "the relaunch resumed AFTER a chunk that had
        never been decoded: a permanent [gap]" — a bug they already fixed there).
      * \`tests/matrix_new_quit_mid_processing.rs\` covers quit-mid-processing for
        meetings.
    For a long dictation there is no equivalent: the spilled WAV (Y3-A) and any
    completed chunk text (Y3-B) are on disk, and nothing reads them back.

    Do:
      * Persist per-take chunk results as they complete (a small sidecar JSON
        next to the spilled WAV in the recovery dir, or a \`take_chunks\` table —
        prefer the DB, it is already WAL and already the durable store, and
        db.rs:1191 already reasons about which tables hold user text).
      * On launch, \`recover_dictation()\` finds takes with audio + partial text
        and NO transcript row, and offers them in History as recoverable with
        one action: "Finish transcribing". It must not auto-paste anything —
        pasting into whatever app happens to be focused minutes or days later is
        the wrong behaviour and would violate YV21's paste-target guard.
      * Honour the existing 7-day purge so the recovery dir cannot grow forever.
      * A recovered take runs the SAME cleanup and hallucination gates as a live
        take. lib.rs:6230 already records this rule for the retry path ("the
        retry path runs the SAME gate as the live path") — extend it, do not
        fork it.

    Tests \`tests/long_take_recovery.rs\`:
      * \`kill_mid_decode_leaves_audio_plus_partial_text_and_no_transcript_row\`
      * \`next_launch_offers_the_take_and_never_pastes_it\`
      * \`finishing_a_recovered_take_runs_the_same_gates_as_a_live_take\`
      * \`recovery_dir_purge_still_bounds_growth\`
      * \`a_completed_take_leaves_nothing_recoverable\` (no phantom offers).

    Depends on Y3-A, Y3-B.

    What NOT to do:
      - Do NOT auto-resume decode at launch without the user asking. A cold
        launch that pins the GPU for four minutes is its own bug.
      - Do NOT store partial TEXT anywhere outside the app's data dir. Transcript
        text must never reach a log or a support bundle unredacted — there are
        already tests for that (tests/support_bundle_redaction.rs); keep them green.
  `,
  acceptance: `
    grep -q 'fn recover_dictation' desktop/src-tauri/src/lib.rs
    test -f desktop/src-tauri/tests/long_take_recovery.rs
    grep -q 'next_launch_offers_the_take_and_never_pastes_it' desktop/src-tauri/tests/long_take_recovery.rs
    grep -q 'finishing_a_recovered_take_runs_the_same_gates_as_a_live_take' desktop/src-tauri/tests/long_take_recovery.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test long_take_recovery        ; test $? -eq 0
    cargo test --features custom-protocol --test capture_journal_recovery  ; test $? -eq 0
    cargo test --features custom-protocol --test support_bundle_redaction  ; test $? -eq 0
    cargo test --features custom-protocol --test matrix_new_quit_mid_processing ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y3-F', prompt: 'Y3', branch: 'loop/y3-f-max-session-length-and-the-honest-ceiling', gated: null,
  title: 'A declared maximum session length with a warning before it, instead of an undeclared cliff',
  preflight: `
    grep -q 'MAX_SESSION_SECONDS' desktop/src-tauri/src/record.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test max_session
  `,
  spec: `
    There is no maximum take length in the code — \`git grep "MAX_TAKE\\|
    max_take\\|MAX_RECORD\\|session_len" desktop\` returns nothing functional.
    That is not generosity; it means the failure mode at some unknown length is
    a timeout or an OOM rather than a message. Wispr declares 20 minutes,
    raised from 5 (reference_wispr_parity_research §2.1 [OFFICIAL]).

    Do:
      * \`record::MAX_SESSION_SECONDS\` — a declared ceiling. Choose it from what
        Y3-A/B actually hold and SAY the measurement in the doc comment; do not
        copy Wispr's 20 minutes without evidence. If the measured answer is
        larger than 20 minutes, declare the larger number — the point is that it
        is declared and tested, not that it matches a competitor.
      * At 80% of the ceiling the pill says so, once, calmly (reuse the
        transcribing/listening phase copy path from Y3-C — no new toast system).
      * At the ceiling the take STOPS cleanly and transcribes what it has. It
        does not error, does not truncate mid-word if a VAD boundary is within
        a couple of seconds (vad.rs:803 lines of machinery already exists — use
        it to pick the cut), and does not discard anything.
      * A meeting is NOT subject to this ceiling. \`tests/matrix_row17_meeting_cap.rs\`
        already covers the meeting cap; assert the two ceilings are separate
        constants so tightening one never silently tightens the other.

    Tests \`tests/max_session.rs\`:
      * \`take_stops_at_the_ceiling_and_transcribes_what_it_has\`
      * \`warning_fires_once_at_eighty_percent\`
      * \`cut_prefers_a_vad_boundary_within_the_grace_window\`
      * \`meeting_cap_and_dictation_cap_are_separate_constants\`

    Depends on Y3-A, Y3-C.

    What NOT to do:
      - Do NOT pick 5 minutes to be safe. Wilson dictates long; a low ceiling is
        the same complaint in a new costume.
      - Do NOT stop the take without transcribing it.
  `,
  acceptance: `
    grep -q 'MAX_SESSION_SECONDS' desktop/src-tauri/src/record.rs
    test -f desktop/src-tauri/tests/max_session.rs
    grep -q 'take_stops_at_the_ceiling_and_transcribes_what_it_has' desktop/src-tauri/tests/max_session.rs
    grep -q 'meeting_cap_and_dictation_cap_are_separate_constants' desktop/src-tauri/tests/max_session.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test max_session              ; test $? -eq 0
    cargo test --features custom-protocol --test matrix_row17_meeting_cap ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y3-G', prompt: 'Y3', branch: 'loop/y3-g-long-take-latency-and-energy-budget', gated: null,
  title: 'A measured latency and energy budget for long takes, published as a test that fails on regression',
  preflight: `
    test -f desktop/src-tauri/tests/long_take_budget.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test long_take_budget
  `,
  spec: `
    \`latency.rs\` (146 lines) already instruments the press->capture_start span
    (YV35, anchored on physical key-down — lib.rs:1138). YV81 was an explicit
    energy pass ("no busy timers, idle animations park, polish sidecar unloads
    when unused"). Y3 adds chunked decode, a spill writer and a progress event
    stream — three new opportunities to undo all of that.

    Also note the marketing stake, which is real and already written down:
    reference_wispr_parity_research §2.11 records Wispr at ~800 MB RAM / ~8% CPU
    idle and says of Yap "Measure it and publish the number — it is free
    marketing." A regression test is how that number stays true.

    Create \`tests/long_take_budget.rs\` with named budgets as constants and a
    comment giving the machine and the date each was measured on:
      * \`press_to_capture_start\` unchanged by the spill writer (compare against
        latency.rs's existing measurement path, not a new stopwatch).
      * \`chunk_decode_wall_per_audio_second\` — a ceiling, measured on the
        committed fixture, so a chunker regression that halves throughput fails.
      * \`progress_events_per_minute_of_audio\` — a CEILING. An event storm from
        the progress stream is an energy regression that nothing else would catch.
      * \`resident_bytes_after_a_long_take_return_to_baseline\` — the spill
        buffers and the chunk text must be dropped, not leaked, when the take
        finalizes.
      * \`no_new_polling_timer_was_introduced\` — sweep for
        \`Duration::from_millis\`/\`from_secs\` under a threshold in the modules Y3
        touched (record.rs, transcription.rs, lib.rs's take path), asserting
        against an explicit allowlist of the ones that already exist. This is
        the standing guard for YV81.

    Then write the numbers into docs/loop/PLAN.md's audit section as measured
    facts with their date, so the next loop inherits evidence instead of folklore.

    Depends on Y3-A..F.

    What NOT to do:
      - Do NOT assert a wall-clock latency budget that depends on the runner's
        GPU. Express the decode budget as a RATIO against the same fixture
        decoded single-window on the same machine in the same run.
      - Do NOT use a timing test as the only proof of a correctness fix.
    PANEL 2026-09-12 — the budget list is missing the app itself and the write
    target does not exist in a lane worktree.
      * Budgets go to docs/BUDGETS.md (TRACKED). docs/loop/PLAN.md is UNTRACKED
        in git — \`git status --porcelain --untracked=all\` lists it, and
        \`git ls-files docs/loop\` returns HARNESS.md only — so a lane worktree,
        which is a checkout of origin/main, does not contain it and the old
        \`grep -q 'measured' docs/loop/PLAN.md\` could never pass.
      * ADD A FOOTPRINT CEILING: peak resident bytes with the ASR engine AND
        the polish child both loaded, asserted as a ceiling, on the declared
        floor machine (Y6-E names it). SEC-C turns on a 1.12 GB resident GGUF
        and Y4-E runs a multi-chunk pass over it while ASR is loaded; Y3-A caps
        only the audio buffers (~32 MB). "Budgets on the weakest supported Mac"
        cannot be enforced until the weakest Mac is named.

  `,
  acceptance: `
    test -f desktop/src-tauri/tests/long_take_budget.rs
    grep -q 'progress_events_per_minute_of_audio' desktop/src-tauri/tests/long_take_budget.rs
    grep -q 'resident_bytes_after_a_long_take_return_to_baseline' desktop/src-tauri/tests/long_take_budget.rs
    grep -q 'no_new_polling_timer_was_introduced' desktop/src-tauri/tests/long_take_budget.rs
    # PANEL: docs/loop/PLAN.md is UNTRACKED in git, so it does not exist in any
    # lane worktree and this grep could never pass. Budgets go in a tracked doc.
    test -f docs/BUDGETS.md
    grep -qE '[0-9]+ ?ms' docs/BUDGETS.md
    grep -q 'peak_resident_bytes_with_asr_and_polish_loaded' desktop/src-tauri/tests/long_take_budget.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test long_take_budget ; test $? -eq 0
  `,
})

// ── 20-y2-trial-and-limits.mjs ────────────────────────────────────────────
// Y2 — TRIAL + LIMITS IN THE PILL. Wilson, 2026-09-12, verbatim: "the pill does
// not tell people when they reach their limits or when the 14-day trial ends
// (Wispr Flow does)."
//
// AUDIT: the licensing BACKEND is good and the MAIN WINDOW is good. The pill —
// the only Yap surface a user looks at while working — knows nothing.
//
//   src-tauri/src/license.rs         1784 lines. TRIAL_DAYS = 14 (license.rs:107),
//                                    clock-rollback floor, two-store trial start,
//                                    Ed25519 offline verify, revocation list.
//   src-tauri/src/lib.rs:1111        license_allows_new_dictation — the one gate,
//                                    emits `license_required`, throttled notify.
//   src/license/status.ts            chipFor / statusCopy / trialWarningText /
//                                    TRIAL_WARN_DAYS = 3, all pure + unit-tested.
//   src/App.tsx:1194-1210            main window listens for license_status and
//                                    license_required and raises a sheet.
//
//   git grep -n "license\|trial" origin/main -- desktop/src/pill \
//        desktop/src/float-main.tsx desktop/src-tauri/src/float_pill.rs
//     -> ZERO MATCHES. The pill window never receives the license event, has no
//        state for it, and renders nothing.
//
// So a user on day 13 gets no warning where they are looking, and on day 15 the
// hotkey stops working with a throttled system notification as the only signal.
// status.ts:80-88 deliberately fires the trial warning ONCE at three days
// ("a countdown that reappears every launch is how a good app becomes
// nagware") — a good rule for a modal toast, and the wrong rule for the pill,
// which is ambient and can carry a quiet persistent numeral the way Wispr's
// Flow Bar does.
//
// USAGE LIMITS: there is no metering of any kind.
//   git grep -n "quota\|usage_limit\|daily_limit\|words_limit\|minutes_used" \
//        origin/main -- desktop  -> 0 functional matches.
// Wispr's free desktop tier is 2,000 words/week and it ships a named
// notification `WeeklyWordsLimitReached`
// (reference_wispr_parity_research §2.8 / §5.2, both [BUNDLE]/[OFFICIAL]).
// Yap's shipped model is $29 lifetime with a 14-day full-feature trial and NO
// subscription (project_yap_build_state, closed decision). Whether Yap gains a
// metered free tier at all is a pricing decision -> DB-A is gated:'panel'.
// Y2-A..E ship the mechanism and the surfaces for the trial, which is decided.
//
// SHARED PREAMBLE + STANDARD GATE: see 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'LIC-A', prompt: 'Y2', branch: 'loop/lic-a-stripe-to-supabase-issuer-purchase-to-working-dictation', gated: null,
  title: 'Payment to working dictation, on a Supabase issuer this repo owns — the leg no item owned',
  preflight: `
    test -f supabase/functions/yap-license/index.ts
    test -f docs/YAP-LICENSING.md
    grep -q 'ISSUANCE' docs/RELEASE.md
    test 0 -eq "$(grep -c 'sslip.io' desktop/src-tauri/src/license.rs)"
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test activation_e2e
  `,
  spec: `
    OWNER DECISION 2026-09-13 (Wilson). The panel gated this on "automated or
    manual?". The answer is neither of the panel's two options as written:
    issuance is ALREADY automated, and it lives somewhere this repo does not
    own. Wilson's call is to MOVE IT TO SUPABASE — "connect this to Supabase".
    So this item is a MIGRATION with a proof, not a greenfield build.

    HOW LICENSING WORKS TODAY — measured, not assumed:
      * Checkout is a Stripe PAYMENT LINK, one compile-time constant:
        license.rs:137 PAYMENT_LINK_URL, opened by \`open_purchase_page\`
        (lib.rs:3557-3570) with no argument, so the webview never supplies a URL.
        The link is \`active: false\` on Stripe (license.rs:131-136) — purchasing
        is OFF until the delivery path is proven, which is this item.
      * Fulfilment is a Fastify service on the FORGE BOX, outside this repo:
        drivia-forge \`server/src/routes/yap.ts\` registers
        POST /v1/yap/stripe-webhook (:93), GET /v1/yap/license (:402),
        GET /v1/yap/revoked.json (:479), POST /v1/yap/resend (:493), wired in
        \`server/src/index.ts:406\`. The signer is \`server/src/yap-license.ts\`
        (\`signClaims\`), Ed25519, key at
        /etc/forge/yap/license-signing-ed25519.pem root:root 0400
        (yap-license.ts:43, :179). Delivery is Resend (yap-license.ts:623-690),
        and a mail failure deliberately never turns into a non-2xx for Stripe
        (:659-662). Issuance is idempotent on session id AND event id (:580-583).
      * The app verifies OFFLINE against the pinned public key
        (license.rs ISSUER_PUBLIC_KEY_SPKI_B64 / ISSUER_SKID) and only ever
        contacts one host, for the public revocation list: license.rs:117-119,
        \`https://forge.87-99-149-214.sslip.io/v1/yap/revoked.json\` — a URL with
        the box's IP ADDRESS in its hostname.
      * So the real defects are: the fulfilment path is invisible to this repo
        and untestable in this gate; revocation is pinned to a box IP; and
        nothing here has ever been walked end to end.

    WHAT TO BUILD — Stripe Checkout -> Supabase Edge Function issuer:
      1. \`supabase/functions/yap-license/index.ts\` — one Deno Edge Function
         with the four routes the Forge service has: stripe-webhook (verify the
         Stripe signature with the webhook secret, idempotent on
         \`event.id\` AND \`checkout.session.id\`), license (retrieve by purchase
         email), revoked.json (the public list, cache-control max-age 300), and
         resend. PORT the Forge logic; do not reinvent the wire format.
      2. The WIRE FORMAT IS FROZEN:
         \`base64url(claimsJson) "." base64url(ed25519 sig)\`, the signature over
         the ASCII BYTES of the first segment, claims
         \`{ v, plan, seats, email_hash, issued_at, kid, skid }\`. Every shipped
         copy of Yap pins the public key. KEEP THE SAME SIGNING KEY: re-keying
         invalidates nothing yet (no customer exists) but changing the FORMAT
         silently forks the verifier. Put the claims/signature logic in
         \`supabase/functions/_shared/claims.ts\` using Web Crypto so it runs
         unchanged under Deno and under vitest.
      3. THE SIGNING KEY MOVES INTO SUPABASE SECRETS
         (\`supabase secrets set YAP_SIGNING_KEY_PEM=...\`), never into this repo,
         never into an .env that is read by anything else. The PUBLIC half stays
         compiled into the app exactly as it is today. Nothing changes on the
         verification side.
      4. \`REVOCATION_URL\` and \`ISSUER_HOST\` in license.rs repoint at the
         Supabase function URL — a stable hostname, no IP. \`sslip.io\` must not
         appear in license.rs afterwards. Keep the "one host, one call, no
         telemetry" property: it is a shipped claim (PRIVACY.md, PRIV-A).
      5. Delivery stays RESEND (the Forge implementation is correct and its
         failure semantics are right), called from the Edge Function with
         \`RESEND_API_KEY\` in Supabase secrets. AND the key is retrievable
         in-app: add "I already paid — retrieve my license" to
         \`PurchasePrompt.tsx\`, which posts the checkout email to the license
         route and activates on success.
      6. Define what a FAILED activation and a FAILED retrieval say. Today
         neither path has copy. One sentence, one action, never a raw Rust
         string, and a retrieval failure must NEVER block offline verification.

    THE SUPABASE PROJECT IS A RUNTIME DEPENDENCY WILSON PROVISIONS.
      * A DEDICATED YAP PROJECT. Explicitly NOT the Drivia project
        (\`vlfrzdbqwsnrosmcygca\`), which is over its free-tier limits; a
        licensing outage caused by an unrelated product's usage is the worst
        possible coupling. The project ref, its function URL, the Stripe webhook
        signing secret, \`RESEND_API_KEY\` and \`YAP_SIGNING_KEY_PEM\` are
        OWNER-PROVISIONED VALUES. None of them may be committed.
      * THE GATE STAYS OFFLINE. Build and test against \`supabase start\` (the
        local stack) or against stubs: the webhook handler takes its Stripe
        client and its mailer as injected dependencies so the tests drive it
        with a fixture event and a fixture key pair and never open a socket. If
        \`supabase\` is not installed, the vitest suite must still pass — the
        local-stack walk is a DOCUMENTED MANUAL STEP, not a gate conjunct.
      * Write \`docs/YAP-LICENSING.md\`: what the flow is, and a
        RUNTIME DEPENDENCIES table (same convention Y6-E puts in
        ARCHITECTURE.md) with one row per provisioned value — what it is, who
        provisions it, where it lives, and what breaks if it is missing.
        Nothing may be listed as "assumed present on the machine".
      * \`docs/RELEASE.md\` gains an ISSUANCE heading: deploy the function,
        set the secrets, register the Stripe webhook endpoint, re-activate the
        payment link, and the one-command liveness check for the issuer host
        that belongs in the RELEASE CHECKLIST and never at runtime.

    THE PROOF, which is the item's real evidence: issue a staging key from the
    local stack, activate it in a clean \`YAP_DATA_DIR\` (Y0-D), assert the
    entitlement flips and a dictation completes.

    What NOT to do:
      - Do NOT put the signing key, or any path to a live one, in this repo.
      - Do NOT make dictation depend on reaching the issuer. Offline verify
        stays the mechanism; retrieval and revocation are conveniences.
      - Do NOT point anything at the Drivia Supabase project.
      - Do NOT change the claims wire format or the pinned public key.
      - Do NOT delete the Forge implementation from drivia-forge as part of this
        item. Two live issuers signing with one key is fine; one dead customer
        path is not. Decommission is a release step, after the proof.
  `,
  acceptance: `
    test -f supabase/functions/yap-license/index.ts
    test -f supabase/functions/_shared/claims.ts
    test -f docs/YAP-LICENSING.md
    grep -q 'Runtime Dependencies' docs/YAP-LICENSING.md
    grep -q 'ISSUANCE' docs/RELEASE.md
    grep -rq 'retrieve' desktop/src/license
    test 0 -eq "$(grep -c 'sslip.io' desktop/src-tauri/src/license.rs)"
    test 0 -eq "$(grep -rl 'vlfrzdbqwsnrosmcygca' supabase desktop | wc -l | tr -d ' ')"
    test 0 -eq "$(grep -rl 'BEGIN PRIVATE KEY' supabase desktop | wc -l | tr -d ' ')"
    test -f desktop/src-tauri/tests/activation_e2e.rs
    grep -q 'a_signed_key_flips_the_entitlement_and_dictation_resumes' desktop/src-tauri/tests/activation_e2e.rs
    grep -q 'a_failed_activation_has_copy_and_an_action' desktop/src-tauri/tests/activation_e2e.rs
    grep -q 'retrieval_failure_never_blocks_offline_verification' desktop/src-tauri/tests/activation_e2e.rs
    grep -rq 'the_webhook_is_idempotent_on_event_id_and_session_id' supabase desktop/src
    grep -rq 'the_signature_covers_the_ascii_bytes_of_the_claims_segment' supabase desktop/src
    cd ${APP} && npm ci
    npm test         ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build    ; test $? -eq 0
    cd src-tauri
    cargo test --features custom-protocol --test activation_e2e ; test $? -eq 0
    cd ../..
    sed -i '' 's/pub const SOLD_PLAN: &str = "lifetime";/pub const SOLD_PLAN: \\&str = "lifetime_MUTANT";/' desktop/src-tauri/src/license.rs
    ( cd ${APP}/src-tauri && cargo test --features custom-protocol --test activation_e2e ) ; test $? -ne 0
    git checkout -- desktop/src-tauri/src/license.rs
    git diff --exit-code -- desktop/src-tauri/src/license.rs
  `,
})

ITEMS.push({
  id: 'Y2-A', prompt: 'Y2', branch: 'loop/y2-a-license-status-reaches-the-pill-window', gated: null,
  title: 'The float window subscribes to license status — the wiring that does not exist',
  preflight: `
    grep -q 'license' desktop/src-tauri/src/float_pill.rs
    grep -q 'license_status\\|LicenseStatus' desktop/src/float-main.tsx
    cd ${APP} && npm ci && npm test -- pill
  `,
  spec: `
    Tauri events emitted with \`app.emit\` reach every window, but the float
    window has no listener and no state, so the payload lands nowhere.
    \`desktop/src/float-main.tsx\` is 53 lines and mounts a pill; it subscribes
    to nothing license-shaped.

    Do:
      * In float-main.tsx, on mount: \`invoke<LicenseStatus>("license_status")\`
        for the initial value (the same command App.tsx:1194 calls), then
        \`listen<LicenseStatus>("license", ...)\` and
        \`listen<LicenseStatus>("license_required", ...)\`. Hold it in one piece
        of state and pass it to whichever pill is mounted.
      * REUSE \`desktop/src/license/status.ts\` verbatim — \`chipFor\`,
        \`daysLeft\`, \`trialCountdown\`, \`statusCopy\`. Do not write a second
        copy of the trial arithmetic for the pill; that file exists precisely so
        the card, the prompt and the changelog "cannot drift apart" (its own
        doc comment) and the pill is now a fourth consumer.
      * Add a pure \`pillLicense(status)\` to a new
        \`desktop/src/pill/license.ts\` returning
        \`{ show: boolean, tone: "trial"|"urgent"|"ended", glyph: string,
           value: string|null, title: string }\`, with the display POLICY in one
        pure function so Y2-B/C/D render it and never re-decide it:
          - licensed                        -> show: false. Nothing. Ever.
          - trial, days_left > 7            -> show: false (ambient silence)
          - trial, 1..7 days                -> show: true, tone trial,  value "Nd"
          - trial, last day / 0             -> show: true, tone urgent, value "1d"
          - license_required                -> show: true, tone ended
        Seven days, not three: the pill is ambient and cheap to glance at, and
        the one-shot toast at TRIAL_WARN_DAYS = 3 stays exactly as it is. Say
        both numbers in the doc comment so the difference reads as deliberate.
      * The pill NEVER shows a price in this item. The money copy belongs to the
        purchase surface (Y2-D), and \`PRICE_LABEL\` must not be imported by any
        file under desktop/src/pill/.

    Tests in \`desktop/src/pill/license.test.ts\`, table-driven over the full
    fortnight (14 -> 0) plus licensed and license_required: assert the exact
    show/tone/value for each day, and assert \`show === false\` for every
    licensed status regardless of trial fields (a licensed user who once had a
    trial must see nothing).

    What NOT to do:
      - Do NOT poll \`license_status\` on an interval from the pill. It is
        event-driven; the backend already emits on every change.
      - Do NOT re-derive days-left from \`expires_at_ms\` and a second wall-clock read in the
        pill. The backend owns the clock, including the rollback floor
        (license.rs:473-520). A second clock is a second answer.
  `,
  acceptance: `
    test -f desktop/src/pill/license.ts
    test -f desktop/src/pill/license.test.ts
    grep -q 'pillLicense' desktop/src/pill/license.ts
    grep -q 'license_status' desktop/src/float-main.tsx
    grep -q 'license_required' desktop/src/float-main.tsx
    grep -q 'from "../license/status"' desktop/src/pill/license.ts
    # the pill never learns the price
    test 0 -eq "$(git grep -c 'PRICE_LABEL' -- desktop/src/pill | wc -l)"
    # no second clock in the pill
    test 0 -eq "$(grep -c 'now()' desktop/src/pill/license.ts)"
    cd ${APP} && npm ci
    npm test -- pill/license ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build            ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y2-B', prompt: 'Y2', branch: 'loop/y2-b-pill-trial-countdown-in-both-styles', gated: null,
  title: 'A quiet trial numeral on the pill in both pill styles, in the 30px side dock too',
  preflight: `
    grep -q 'pillLicense' desktop/src/pill/ClassicPill.tsx
    grep -q 'pillLicense' desktop/src/pill/YappyPill.tsx
    cd ${APP} && npm ci && npm test -- pill
  `,
  spec: `
    Render \`pillLicense(status)\` from Y2-A in both pills. \`ClassicPill.tsx\` is
    the DEFAULT (\`lib.rs:407 pill_style: "classic"\`) so it is not optional, and
    \`YappyPill.tsx\` is the character pill.

    ClassicPill: a small trailing chip on the capsule — the numeral in
    Departure Mono (the pixel face already bundled at
    src/assets/fonts/DepartureMono-Regular.woff2, and the face \`chipFor\` in
    status.ts:118 already says "the component sets it in Departure Mono"), the
    unit in the body face. \`urgent\` shifts hue and nothing else: no pulsing, no
    animation, no motion. A countdown that moves is a countdown that nags.

    YappyPill: Yappy holds it. Same numeral, same silence — a posture change at
    \`urgent\` (ears down, one blink slower) rather than a badge, because this
    pill's whole job is that state reads as character
    (feedback_companion_must_be_cute: pixel art, chunky, no angry eyebrows).

    THE SIDE DOCK IS THE HARD CASE AND IT IS IN SCOPE. Per
    reference_wispr_parity_research §4.2, Wispr's side-docked bar is a 30 px
    strip and their fixed-width label states "crush in a 30px column", which is
    why only the primary listening states rotate. A "3d" numeral fits a 30 px
    column; "3 days left" does not. So: the pill shows the VALUE only, and the
    full sentence lives in the \`title\` (tooltip) which \`pillLicense\` already
    returns. Verify at all three dock positions — \`pill_position\` is
    \`bottom|left|right\` (lib.rs:408 default "bottom").

    prefers-reduced-motion: ClassicPill.tsx:44-47 already paints one calm static
    frame under Reduce Motion. The trial chip must be present in that frame —
    it is information, not decoration, and must not be hidden with the animation.

    Tests: extend \`desktop/src/pill/license.test.ts\` for the geometry policy
    (\`value\` is never longer than 3 characters for any day 0..14) and add a
    vitest render assertion per pill that the chip is present for
    \`days_left: 5\`, absent for \`licensed\`, and present under a mocked
    \`matchMedia("(prefers-reduced-motion: reduce)") => matches: true\`.

    PR body owes: the pill captured at bottom, left and right docks, for
    days_left 10 (hidden), 5 (trial), 1 (urgent) and license_required.

    What NOT to do:
      - Do NOT animate, pulse, bounce or flash the countdown.
      - Do NOT put the word "upgrade" or a price on the capsule. That is Y2-D.
      - Do NOT let the chip widen the capsule enough to break the 30 px strip.
  `,
  acceptance: `
    grep -q 'pillLicense' desktop/src/pill/ClassicPill.tsx
    grep -q 'pillLicense' desktop/src/pill/YappyPill.tsx
    grep -q 'reduce' desktop/src/pill/ClassicPill.tsx
    grep -q 'value_is_never_wider_than_the_side_dock\\|valueFitsSideDock' desktop/src/pill/license.test.ts
    cd ${APP} && npm ci
    npm test -- pill ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build    ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y2-C', prompt: 'Y2', branch: 'loop/y2-c-trial-ended-pill-state-and-refused-press', gated: null,
  title: 'A refused hotkey press produces a pill state that explains itself, instead of a throttled notification',
  preflight: `
    grep -q '"gated"' desktop/src/pill/live.ts
    cd ${APP} && npm ci && npm test -- pill/live
  `,
  spec: `
    Today, past the trial: \`license_allows_new_dictation\` (lib.rs:1111-1126)
    emits \`license_required\`, calls \`notify()\` if \`should_announce_gate()\`
    permits, logs, returns false. The pill does not move. The user holds the key
    and nothing happens — which is the single worst possible reading of a
    paid-product boundary, because it is indistinguishable from a broken app.

    Add \`"gated"\` to \`LivePhase\` (live.ts:252, currently
    \`idle|listening|thinking|done|sleepy\`, plus \`"blocked"\` from PERM-C) and
    render it in both pills: the capsule takes the \`ended\` tone from
    \`pillLicense\`, shows the trial-ended glyph, and a click opens the purchase
    surface in the main window. It holds for ~2.5 s after a refused press and
    then settles back to the persistent \`ended\` chip, so leaning on the hotkey
    is answered every time without the state becoming permanent noise.

    Precedence, asserted in tests, because these will collide in real use:
      blocked (no mic)  >  gated (no license)  >  listening  >  thinking  >  done
    The microphone reason wins: telling a user to buy a license when Yap cannot
    hear them is the wrong sentence. This is the same order PERM-C enforces in
    \`start_recording\` and the pill must not disagree with the backend.

    Keep \`should_announce_gate\`'s throttle for the SYSTEM notification exactly
    as it is. The pill state is not throttled — it is the cheap in-place signal
    that makes the throttle safe.

    Copy, from the strings that already exist so nothing drifts:
    \`statusCopy(license_required)\` = "Dictation is paused" / "Everything you
    have already written is still here and still exportable." The pill shows the
    headline; the tooltip carries the body. Reuse, do not rewrite.

    Tests in \`desktop/src/pill/live.test.ts\` (271 lines already):
      * \`gated_holds_then_settles_to_the_ended_chip\`
      * \`blocked_outranks_gated\`
      * \`gated_never_suppresses_the_done_state_of_a_take_already_in_flight\` —
        the trial ending must not eat the result of a take that was allowed to
        start. license.rs's own doc (lib.rs:1100-1108) says the gate "never
        takes back the ones already spoken"; this is that promise in the UI.

    What NOT to do:
      - Do NOT make the gated state permanent-modal or focus-stealing. The pill
        is non-activating (\`macOSPrivateApi: true\`, NSPanel behaviour).
      - Do NOT disable the pill's other affordances. History, search, export and
        settings all keep working past the trial — that is the product's
        promise (KEEP_FOREVER_LINE in status.ts) and the pill must not imply
        otherwise.
  `,
  acceptance: `
    grep -q '"gated"' desktop/src/pill/live.ts
    grep -q 'blocked_outranks_gated' desktop/src/pill/live.test.ts
    grep -q 'gated_never_suppresses_the_done_state' desktop/src/pill/live.test.ts
    grep -q 'gated' desktop/src/pill/ClassicPill.tsx
    grep -q 'gated' desktop/src/pill/YappyPill.tsx
    grep -q 'statusCopy' desktop/src/pill/license.ts
    cd ${APP} && npm ci
    npm test -- pill ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build    ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y2-F', prompt: 'Y2', branch: 'loop/y2-f-a-stored-key-that-grants-nothing-is-not-a-lapsed-trial', gated: null,
  title: 'A paying customer whose key stops verifying is never shown a price',
  preflight: `
    grep -rq 'storedKeyProblem' desktop/src/pill
    cd ${APP} && npm ci && npm test -- license
  `,
  spec: `
    PANEL 2026-09-12, THREE seats independently — the most-converged gap in the
    trial lane. MEASURED at 4e8c9adf: the backend already distinguishes "a key
    is stored and granted nothing" and only the main window renders it.
      desktop/src/license/status.ts:47-48  license_problem_message,
                                           has_stored_license
      desktop/src/license/status.ts:213-217  storedKeyProblem(status)
      desktop/src/license/status.ts:70      DEFAULT_SEATS = 3
      desktop/src/license/status.test.ts:162  "This license was refunded or
                                           charged back."
      grep -rn 'storedKeyProblem|has_stored_license|license_problem_message'
        over all item files -> no functional hit
    Y2-A's pillLicense policy enumerates exactly three inputs — licensed, trial
    with days, license_required — so a revoked, unreadable, clock-broken or
    seat-capped key collapses into license_required, Y2-C paints the "ended"
    tone, and Y2-D offers the Payment Link to someone who has already paid.
    That is the most expensive sentence this app can say.

    Do:
      * pillLicense gains a SIXTH branch: \`has_stored_license && state !==
        "licensed"\` -> tone \`problem\`. Its copy comes from
        \`license_problem_message\`, never from the purchase copy, and it is
        distinct from the lapsed-trial tone at a glance in both pill styles.
      * Its click opens the License panel (re-activate / re-paste the key). NO
        purchase affordance is reachable from this tone — assert that.
      * The tray row says the same sentence, from the same source (Y2-E).
      * Table test over the whole matrix, including a revoked key, an unreadable
        store and a seat-capped key.

    Depends on Y2-A (the wiring) and pairs with Y2-D's "never show a price to a
    stored-key holder" rule, which this item is what makes checkable.
  `,
  acceptance: `
    grep -rq 'storedKeyProblem' desktop/src/pill
    grep -rq "problem" desktop/src/pill/license.ts
    test -f desktop/src/pill/license.test.ts
    grep -q 'a_revoked_key_never_shows_a_price' desktop/src/pill/license.test.ts
    grep -q 'a_seat_capped_key_reads_as_a_problem_not_a_lapsed_trial' desktop/src/pill/license.test.ts
    grep -q 'the_problem_tone_opens_the_license_panel' desktop/src/pill/license.test.ts
    cd ${APP} && npm ci
    npx tsc --noEmit ; test $? -eq 0
    npm test         ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y2-D', prompt: 'Y2', branch: 'loop/y2-d-one-upgrade-path-from-the-pill', gated: null,
  title: 'One click from the pill to purchase, reusing the existing Payment Link — no new money surface',
  preflight: `
    grep -q 'open_purchase_page\\|show_purchase' desktop/src-tauri/src/float_pill.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test purchase_from_pill
  `,
  spec: `
    The purchase machinery is built and proven: \`license::PAYMENT_LINK_URL\` is a
    compile-time constant, \`open_purchase_page\` is the only thing that can hand
    a URL to \`open(1)\` (status.ts:56-62 documents exactly this discipline and
    cites the Stripe objects), and \`src/license/PurchasePrompt.tsx\` is the
    sheet. Stripe was E2E-proven including the refund and dispute legs
    (project_yap_build_state, yap21).

    All this item does is connect the pill to it:
      * Clicking the pill in \`gated\` or \`urgent\` tone focuses the main window
        and raises the existing PurchasePrompt. It does NOT open a browser
        directly from the pill — the sheet is where the $29 / $19 founding copy,
        the seats line and KEEP_FOREVER_LINE live, and skipping it would put
        Wilson's user in Stripe with no context.
      * Add one Rust command \`reveal_purchase_prompt\` that unminimizes + focuses
        the main window and emits \`show_purchase\`. App.tsx listens and raises
        the sheet. The pill's job ends at "ask the main window".
      * The pill's own click target must not steal focus on hover or on the
        press that starts a dictation. \`ClassicPill.tsx\` already publishes a
        hitbox (\`watchPillHitbox\`, YV65) — the upgrade affordance shares it and
        is only live when \`pillLicense().show\` is true, so a licensed user's
        pill has no dead click region.

    Test \`tests/purchase_from_pill.rs\`:
      * \`reveal_purchase_prompt_never_opens_a_url\` — assert the command's body
        contains no call into the opener; the URL path stays behind
        \`open_purchase_page\`. This is a security property, not a style
        preference: one function is the only thing that may be handed to open(1).
      * \`pill_upgrade_is_inert_when_licensed\`.

    What NOT to do:
      - Do NOT add a second Payment Link, price string, or coupon code anywhere.
        FOUNDING_CODE and PRICE_LABEL live in status.ts and PAYMENT_LINK_URL
        lives in Rust; a third copy is a mispriced checkout waiting to happen.
      - Do NOT open a browser from the float window.
  `,
  acceptance: `
    grep -q 'fn reveal_purchase_prompt' desktop/src-tauri/src/lib.rs
    grep -rq 'show_purchase' desktop/src
    grep -q 'reveal_purchase_prompt' desktop/src/pill/license.ts
    test -f desktop/src-tauri/tests/purchase_from_pill.rs
    grep -q 'reveal_purchase_prompt_never_opens_a_url' desktop/src-tauri/tests/purchase_from_pill.rs
    # exactly one payment link and one price label in the tree
    test 1 -eq "$(git grep -c 'PAYMENT_LINK_URL: ' -- desktop/src-tauri/src/license.rs | cut -d: -f2)"
    test 1 -eq "$(git grep -l 'PRICE_LABEL =' -- desktop/src | wc -l | tr -d ' ')"
    cd ${APP} && npm ci && npm run build ; test $? -eq 0
    cd src-tauri && cargo test --features custom-protocol --test purchase_from_pill ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y2-E', prompt: 'Y2', branch: 'loop/y2-e-menu-bar-tray-carries-the-same-truth', gated: null,
  title: 'The menu-bar item says the same thing as the pill and the settings card, from one source',
  preflight: `
    grep -q 'pillLicense\\|license_tray_line' desktop/src-tauri/src/lib.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test tray_license
  `,
  spec: `
    Yap has a tray menu (YV26; \`sync_tray\` referenced at lib.rs:4468) and it
    does not mention the trial. Wispr's status menu carries the state
    (reference_wispr_parity_research §2.2, \`hub_status_menu_*\`). A user who
    hides the pill — \`show_floating_pill\` is a setting (lib.rs:406) — currently
    has NO ambient signal at all.

    Add to the tray, from the SAME decision function so the three surfaces can
    never disagree:
      * A disabled header item carrying the state: "Trial — 5 days left" /
        "Licensed" / "Trial ended — dictation paused".
      * An "Upgrade Yap — $29 once" item, present only when
        \`pillLicense().show\` is true, firing \`reveal_purchase_prompt\`.
      * The tray ICON takes the urgent treatment on the last day and past the
        trial, and only then. A permanently decorated tray icon is noise.

    The decision must be shared, not duplicated: add
    \`license::tray_line(&LicenseStatus) -> (String, bool /*urgent*/)\` in Rust
    and assert in \`tests/tray_license.rs\` that its day boundaries match
    \`pillLicense\`'s exactly — 7 days to appear, urgent at <= 1 — by reading the
    thresholds from named constants that both sides import. Name them once:
    \`license::PILL_SHOW_DAYS = 7\` and \`license::PILL_URGENT_DAYS = 1\`, exported
    to TS through a generated constants module or asserted equal by a test that
    parses both files. Prefer the test-parses-both-files approach; it needs no
    build step and it fails loudly.

    \`sync_tray\` is already guarded and already the only place the tray is
    rebuilt — keep it that way. \`tests/tray_hotkey_no_collision.rs\` exists;
    do not disturb it.

    What NOT to do:
      - Do NOT add a badge count or a number on the tray icon.
      - Do NOT hardcode 7 and 1 in two languages. The whole point of this item
        is one source of truth for three surfaces.
  `,
  acceptance: `
    grep -q 'PILL_SHOW_DAYS' desktop/src-tauri/src/license.rs
    grep -q 'PILL_URGENT_DAYS' desktop/src-tauri/src/license.rs
    grep -q 'fn tray_line' desktop/src-tauri/src/license.rs
    test -f desktop/src-tauri/tests/tray_license.rs
    grep -q 'pill_and_tray_share_the_same_day_thresholds' desktop/src-tauri/tests/tray_license.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test tray_license            ; test $? -eq 0
    cargo test --features custom-protocol --test tray_hotkey_no_collision ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'DB-A', prompt: 'Y2', branch: 'loop/db-a-usage-metering-and-limit-surface', gated: 'panel',
  title: 'Usage metering and a "limit reached" surface — the numbers are Wilson\'s call',
  preflight: `
    grep -q 'usage_window\\|words_this_week' desktop/src-tauri/src/db.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test usage_meter
  `,
  spec: `
    GATED: 'panel'. Wilson said the pill must tell people "when they reach their
    limits". Yap has no limits today, by decision — $29 lifetime, 14-day
    full-feature trial, "no subscription v1", and
    reference_wispr_parity_research §3 lists "trial nag surfaces" under
    "Explicitly not wanted". Wispr's limit is 2,000 words/week on its free
    desktop tier with a \`WeeklyWordsLimitReached\` notification
    (§2.8 [LOCAL], §5.2 [OFFICIAL]). Yap's own marketing line is the opposite:
    "No account, no cloud, no subscription tier gating your words."

    So the MECHANISM is buildable now and the POLICY is not. What the panel and
    Wilson must decide before this item's numbers are written:
      1. Does Yap gain a metered free tier after the trial at all, or does the
         trial simply end (today's behaviour)?
      2. If metered: the unit (words / minutes / takes), the window (day /
         week / rolling 7d), and the number.
      3. Does a limit throttle NEW dictation only, matching the trial's
         boundary exactly (lib.rs:1100-1108), or degrade quality? (Degrading
         quality is almost certainly wrong — say so and let it be rejected.)
      4. Does the pill show consumption before the limit (a Wispr-style
         "1,847 / 2,000 words" readout) or only on arrival?

    BUILD REGARDLESS, because it is useful with or without a limit and it is
    the honest version of Insights:
      * \`usage\` rollup in SQLite: words and voiced-seconds per local day,
        written on take finalize, in the SAME transaction as the transcript row
        so the two can never disagree. Reuse the existing rollup shape —
        \`db.rs\` already carries an insights/day-series path
        (\`tests/meeting_stats_rollup.rs\`, \`get_insights\`, lib.rs:2420) —
        rather than adding a parallel aggregate.
      * \`db::usage_window(unit, window) -> UsageWindow { used, window_start }\`,
        pure over the rollup, with the LIMIT VALUE passed in by the caller and
        NOT stored in this function. That is the seam that lets the policy land
        later as one constant.
      * \`pillLicense\` (Y2-A) grows a \`limit\` branch behind a single
        \`LIMIT_ENABLED\` constant that is FALSE in this item. Wire the state,
        the copy and the tests; ship it dark.
      * Copy drafted, not shipped, for Wilson's review before any send-equivalent
        moment: the limit-reached sentence must name what still works
        (history, search, export, settings — KEEP_FOREVER_LINE) before it names
        what stopped.

    Tests \`tests/usage_meter.rs\`: rollup is written in the transcript's
    transaction (kill the process between and assert neither exists); a local-day
    boundary rolls at local midnight, not UTC; a rolling 7-day window excludes
    day 8 exactly; \`LIMIT_ENABLED == false\` means no gate is ever consulted.

    What NOT to do:
      - Do NOT pick a number. Do NOT ship \`LIMIT_ENABLED = true\`.
      - Do NOT meter by wall-clock recording time. Voiced seconds and words are
        the units a user recognises; a paused hotkey is not consumption.
      - Do NOT send any usage figure anywhere. Local only, forever.
  `,
  acceptance: `
    grep -q 'fn usage_window' desktop/src-tauri/src/db.rs
    grep -q 'LIMIT_ENABLED' desktop/src-tauri/src/license.rs
    grep -qE 'LIMIT_ENABLED: *bool *= *false' desktop/src-tauri/src/license.rs
    grep -q 'limit' desktop/src/pill/license.ts
    test -f desktop/src-tauri/tests/usage_meter.rs
    grep -q 'rollup_is_written_in_the_transcript_transaction' desktop/src-tauri/tests/usage_meter.rs
    grep -q 'local_day_boundary_is_local_not_utc' desktop/src-tauri/tests/usage_meter.rs
    # nothing leaves the machine
    test 0 -eq "$(git grep -cE 'reqwest|http://|https://' -- desktop/src-tauri/src/db.rs | wc -l)"
    cd ${APP} && npm ci && npm test ; test $? -eq 0
    cd src-tauri && cargo test --features custom-protocol --test usage_meter ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'SEC-B', prompt: 'Y2', branch: 'loop/sec-b-trial-state-machine-hardening', gated: null,
  title: 'The trial state machine gets the adversarial tests its own doc comment promises',
  preflight: `
    test -f desktop/src-tauri/tests/trial_state_machine.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test trial_state_machine
  `,
  spec: `
    \`license.rs\` documents its own threat model carefully (license.rs:43-53:
    "A local trial clock on a machine the user controls is deterrence, not
    security"; :473-483 the two-store earliest-wins rule and the
    max-seen-wall-clock floor). The implementation looks right. What it lacks is
    a test file that drives the fortnight and the attacks end to end — and Y2's
    whole UI layer is about to depend on \`days_left\` being correct on every one
    of those days.

    Create \`desktop/src-tauri/tests/trial_state_machine.rs\` over
    \`evaluate_trial\` / \`decide_entitlement\` with the in-memory store
    (\`MemoryStore\`, license.rs:382 — it already exists for this purpose and
    counts writes, so the tests need no disk and no app):
      * \`first_run_starts_the_trial_and_reports_fourteen\`
      * \`each_day_reports_one_fewer_and_zero_is_the_last_day\` — all 15 values.
      * \`clock_rolled_back_cannot_rewind_the_trial\` — the wall-clock floor.
      * \`deleting_the_license_file_does_not_restart_the_trial\` — the DB store
        still holds the start (license.rs:361-363 names this exact attack).
      * \`deleting_the_db_row_does_not_restart_the_trial\` — the mirror case.
      * \`earlier_of_the_two_stores_wins\`
      * \`a_valid_license_beats_an_expired_trial\`
      * \`a_revoked_license_does_not_cancel_a_running_trial\` (license.rs:568-569
        states this; assert it).
      * \`expired_trial_stops_only_new_dictation\` — assert \`allows_new_dictation\`
        is false while nothing else in the entitlement changes. Pair it with the
        existing \`tests/license_gate.rs\` call-site sweep rather than repeating it.
      * \`trial_days_left_never_goes_negative\`
      * \`should_announce_gate_throttles\` — N presses produce one announcement.

    Then close the loop to the UI: a test that for every day 14..0 the Rust
    \`days_left\` and the TS \`daysLeft\`/\`trialCountdown\` agree. Do it by
    generating a small JSON fixture from the Rust test
    (\`desktop/src-tauri/tests/fixtures/trial_days.json\`) and reading it from a
    vitest case, so the two languages are pinned to one table instead of two
    hand-written ladders.

    What NOT to do:
      - Do NOT make the trial cryptographic. license.rs:43 already closed that:
        deterrence, not security. Rewriting it as DRM is out of scope and a
        product change nobody asked for.
      - Do NOT test by sleeping. Inject the clock.
    PANEL 2026-09-12 — the attack ladder is missing the direction that actually
    fires in the field. \`evaluate_trial\` sets
    \`effective_now = max(wall, monotonic, recorded_floor)\` (license.rs:490-496)
    and writes \`floor_ms = effective_now\` back to both stores (:521). A trial
    START in the future IS clamped (\`stored_start.unwrap_or(effective_now)
    .min(effective_now)\`, :505); the FLOOR is not. So ONE forward clock
    excursion — a restored Time Machine image, a bad NTP jump, a user who set
    the date forward once — permanently poisons the floor, the trial reads
    expired on day two, and the design deliberately removes every ordinary
    recovery (deleting the file or the row buys nothing, by earliest-wins). This
    loop then builds a countdown numeral, a hard \`gated\` pill state, a refused
    press and a purchase sheet on top of that latch, so a quiet backend bug
    becomes a surface telling a user who never had a fair trial to pay.
      * Clamp the floor the way the start is clamped: refuse to advance the
        recorded floor more than a few hours beyond the current wall clock, and
        RECORD the excursion instead of absorbing it.
      * Add \`forward_clock_excursion_does_not_expire_the_trial\` and
        \`a_poisoned_floor_can_be_cleared_by_a_signed_grace_claim\`.
      * Give support one non-DRM lever: the signature path already exists, so a
        signed grace/extension claim costs nothing and turns an unrecoverable
        lockout into an email. LIC-A uses the same claim.

  `,
  acceptance: `
    test -f desktop/src-tauri/tests/trial_state_machine.rs
    test -f desktop/src-tauri/tests/fixtures/trial_days.json
    grep -q 'clock_rolled_back_cannot_rewind_the_trial' desktop/src-tauri/tests/trial_state_machine.rs
    grep -q 'deleting_the_license_file_does_not_restart_the_trial' desktop/src-tauri/tests/trial_state_machine.rs
    grep -q 'a_revoked_license_does_not_cancel_a_running_trial' desktop/src-tauri/tests/trial_state_machine.rs
    grep -q 'trial_days' desktop/src/license/status.test.ts
    test 0 -eq "$(grep -c 'thread::sleep' desktop/src-tauri/tests/trial_state_machine.rs)"
    cd ${APP} && npm ci
    npm test -- license ; test $? -eq 0
    cd src-tauri && cargo test --features custom-protocol --test trial_state_machine ; test $? -eq 0
    cargo test --features custom-protocol --test license_gate                        ; test $? -eq 0    grep -q 'forward_clock_excursion_does_not_expire_the_trial' desktop/src-tauri/tests/trial_state_machine.rs
    grep -q 'a_poisoned_floor_can_be_cleared_by_a_signed_grace_claim' desktop/src-tauri/tests/trial_state_machine.rs

  `,
})

// ── 25-y5-ui-ux-polish.mjs ────────────────────────────────────────────────
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

// ── 30-y6-end-to-end-wiring.mjs ───────────────────────────────────────────
// Y6 — END TO END. Wilson: "we got to really think about this thing end to end."
// The seams between the features, which is where a product that works in pieces
// still feels broken.
//
// AUDIT at 4e8c9adf. SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.
// Gate, from docs/loop/HARNESS.md "The gate", run from the worktree's app dir:
//   npx tsc --noEmit · npm test · npm run build ·
//   cargo build -p yap-polish --release + stage src-tauri/binaries/yap-polish-<triple> ·
//   cargo test -p yap-polish --release ·
//   cargo clippy --all-targets --features custom-protocol (in src-tauri) ·
//   cargo test --features custom-protocol (in src-tauri).
// cargo fmt is INFORMATIONAL and exits 1 on unmodified main — never reformat to silence it.
//
// NEVER touch the bundle identifier (com.wilsonguenther.wilson-voice) or the data
// directory (WilsonVoice). Renaming the id resets every macOS TCC grant; renaming
// the data dir orphans the SQLite history.

ITEMS.push({
  id: 'UPD-A', prompt: 'Y6', branch: 'loop/upd-a-updater-endpoint-that-can-actually-serve', gated: null,
  title: 'The updater points at an endpoint that can serve a private repo — today it points at a dead URL',
  preflight: `
    test 0 -eq "$(grep -c 'releases/latest/download/latest.json' desktop/src-tauri/tauri.conf.json)"
    test -f desktop/src-tauri/tests/updater_endpoint.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test updater_endpoint
  `,
  spec: `
    MEASURED, desktop/src-tauri/tauri.conf.json plugins.updater.endpoints:
      "https://github.com/wilsonguenther-dev/wilson-voice/releases/latest/download/latest.json"
    and the repo's own history records why that cannot work: commit 2eabf33
    "site: serve the DMG from Forge — repo went private, GitHub release assets
    are no longer publicly downloadable", then 734aa8c "YV83 site + DMG served
    from Vercel, past Forge's 25MB edge cap". The DMG moved twice and the
    updater manifest URL did not move with it. docs/loop/HARNESS.md records the
    repo as PUBLIC again as of 2026-09-12 — which means the URL may resolve
    today and will silently die the next time the repo is flipped private
    (project_github_actions_public_window is an explicit, recurring procedure).
    An updater whose correctness depends on repo visibility is not an updater.

    Do:
      * Point \`plugins.updater.endpoints\` at the same host that serves the DMG,
        so the manifest and the asset can never disagree about where the build
        is. Read docs/DEPLOY-SITE.md and site/ for the current host before
        choosing, and state in the PR body which host you chose and why.
      * Keep the GitHub URL as a SECOND endpoint, after the primary. Tauri tries
        endpoints in order, so a private-repo window degrades to the primary
        instead of failing.
      * The signing pubkey stays exactly as it is
        (plugins.updater.pubkey, verified present at 4e8c9adf — a base64
        minisign key, not a placeholder). Never regenerate it in this item: a
        new keypair makes every installed copy unable to verify an update, and
        project_yap_build_state records one keypair regeneration already (YV82).
      * \`src/updater.ts\` is already correct in shape — check-only, no auto
        install, DEBUG not ERROR when there is no manifest (its own doc says
        so). Do not change its contract. Add ONE thing: when every endpoint
        fails, the manual "Check for updates" button must say which endpoint was
        tried, because a silent "you're up to date" on a dead endpoint is the
        failure this item exists to prevent.
      * \`tests/updater_endpoint.rs\`: the config parses; there are >= 2
        endpoints; the primary is not a github.com release-asset URL; the pubkey
        is non-empty and is not the string "PLACEHOLDER" or a bare newline; and
        \`createUpdaterArtifacts\` is still true.

    What NOT to do:
      - Do NOT regenerate the updater keypair.
      - Do NOT make the updater install on startup. User-triggered only
        (src/updater.ts's own contract: "USER-TRIGGERED ONLY").
      - Do NOT print any key material into a log, a test name or the PR body.
  `,
  acceptance: `
    # PANEL: the old line anchored the URL to end-of-line, so it passed only
    # while that endpoint happened to be last with no trailing comma. Parse it.
    node -e "const e=require('./desktop/src-tauri/tauri.conf.json').plugins.updater.endpoints; process.exit(e.length>=2 && !/github\\.com\\/.*\\/releases\\//.test(e[0]) ? 0 : 1)"
    node -e "const u=require('./desktop/src-tauri/tauri.conf.json').plugins.updater; process.exit(u.pubkey && u.pubkey.length>40 ? 0 : 1)"
    test -f desktop/src-tauri/tests/updater_endpoint.rs
    grep -q 'at_least_two_endpoints' desktop/src-tauri/tests/updater_endpoint.rs
    grep -q 'primary_endpoint_is_not_a_github_release_asset' desktop/src-tauri/tests/updater_endpoint.rs
    grep -q 'pubkey_is_present_and_not_a_placeholder' desktop/src-tauri/tests/updater_endpoint.rs
    grep -q 'createUpdaterArtifacts' desktop/src-tauri/tests/updater_endpoint.rs
    cd ${APP} && npm ci
    npx tsc --noEmit ; test $? -eq 0
    npm run build    ; test $? -eq 0
    cd src-tauri && cargo test --features custom-protocol --test updater_endpoint ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'UPD-B', prompt: 'Y6', branch: 'loop/upd-b-publish-the-updater-triple-and-keep-a-rollback', gated: null,
  title: 'Publish latest.json + .app.tar.gz + .sig to the host the updater now points at, and keep the previous build',
  preflight: `
    test -x scripts/release-local.sh
    grep -q 'latest.json' docs/DEPLOY-SITE.md
    grep -q 'app.tar.gz' docs/DEPLOY-SITE.md
  `,
  spec: `
    PANEL 2026-09-12, two seats independently. UPD-A repoints
    plugins.updater.endpoints at "the same host that serves the DMG" — and
    nothing publishes a manifest there. MEASURED at 4e8c9adf:
      docs/DEPLOY-SITE.md:40-46  the staging block copies *.html *.css *.woff2 +
        vercel.json and one \`gh release download --pattern '*.dmg'\`. No
        latest.json, no .app.tar.gz, no .sig.
      .github/workflows/release.yml:91-120  the ONLY producer of the updater
        manifest and the minisign .sig — and Actions is disabled account-wide
        (docs/loop/HARNESS.md, CI mode: the expected answer is \`local\`).
      tauri.conf.json:42 createUpdaterArtifacts true; :66-69 the endpoint and
        the pubkey.
    macOS's updater consumes the .app.tar.gz plus its .sig, not the DMG, so
    serving the DMG at that host is necessary and insufficient. Left as is,
    every installed copy reports "up to date" forever — src/updater.ts logs
    DEBUG, not ERROR, when there is no manifest — and a security fix reaches
    nobody. There is also no rollback: no known-good DMG retained on the host.

    Do:
      * \`scripts/release-local.sh\`: build, sign with SEC-A's identity,
        notarize, staple, emit Yap.app.tar.gz + .sig + latest.json with
        TAURI_SIGNING_PRIVATE_KEY, and stage all four next to the DMG.
      * Amend docs/DEPLOY-SITE.md's staging block to copy all four.
      * Verify the LIVE endpoint at the end of a release: fetch latest.json over
        the network and verify the .sig against the pubkey already in
        tauri.conf.json. Offline, skip with a NAMED reason — never pass quietly.
      * Keep the previous DMG + manifest on the host as the documented rollback,
        and write the downgrade steps into docs/RELEASE.md.
      * Until a manifest is actually published, the updater check ships DISABLED
        rather than pointed at a 404. Say which state shipped in the PR body.

    Runs immediately after UPD-A, whose config change is inert without it.
  `,
  acceptance: `
    test -x scripts/release-local.sh
    grep -q 'set -euo pipefail' scripts/release-local.sh
    grep -q 'TAURI_SIGNING_PRIVATE_KEY' scripts/release-local.sh
    grep -q 'app.tar.gz' scripts/release-local.sh
    grep -q 'latest.json' docs/DEPLOY-SITE.md
    grep -q 'app.tar.gz' docs/DEPLOY-SITE.md
    grep -q 'rollback' docs/RELEASE.md
    test 0 -eq "$(grep -c 'TAURI_SIGNING_PRIVATE_KEY=' scripts/release-local.sh)"
    bash -n scripts/release-local.sh ; test $? -eq 0
    ./scripts/release-local.sh --verify-endpoint-or-name-the-reason ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y6-A', prompt: 'Y6', branch: 'loop/y6-a-onboarding-that-ends-in-a-working-dictation', gated: null,
  title: 'Onboarding ends with one successful pasted dictation, or it tells you exactly what is missing',
  preflight: `
    grep -q 'first_paste\\|firstPaste' desktop/src/Onboarding.tsx
    cd ${APP} && npm ci && npm test
  `,
  spec: `
    MEASURED: \`src/Onboarding.tsx:41\`
    \`STEP_ORDER = ["welcome","permissions","calibration","done"]\`. Calibration
    records a sample; nothing ever pastes. So a user finishes onboarding without
    having seen Yap's one and only trick work, and the four things that can
    break it — microphone TCC, Accessibility TCC, a missing ASR model, a missing
    paste target — each fail later, separately, with no context.

    Add a final step: TRY IT. The user clicks into a real text field inside the
    Yap window, holds the hotkey, speaks, and watches the text arrive. Then:
      * SUCCESS -> the done step, and mark \`onboarded: true\` (lib.rs:420) only
        here. Today \`onboarded\` is set without any proof the app works.
      * FAILURE -> name the stage that failed, using PERM-E's permission health
        and Y5-F's error catalogue. Four distinct dead ends, four distinct
        screens, each with the one button that fixes it:
          no mic grant -> PERM-B's denied screen
          no Accessibility grant -> the Accessibility deep link
          no model -> the model download (ModelSetup.tsx)
          paste refused -> explain the paste target and offer copy-to-clipboard
            instead (the clipboard path exists; \`auto_paste\` is a setting,
            lib.rs:405)
      * A skip is allowed and must be honest: "Skip for now" leaves
        \`onboarded\` true but raises the permission health row until it works.

    Wispr ships 92 i18n keys for this one flow ("Try it yourself", \`tiy_*\`,
    reference_wispr_parity_research §2.8 [BUNDLE], scored 🟡 for Yap as
    "calibration step"). Yap does not need 92 strings; it needs the moment.

    Also fix the ordering hazard already in the file: the model may still be
    downloading when the user reaches calibration, and the step handles it by
    WAITING with a ribbon (Onboarding.tsx:~340, YV54). Reuse that exact pattern
    for the try-it step rather than inventing a second waiting affordance.

    Tests: extract the step machine to \`desktop/src/onboarding.ts\`
    (\`nextStep(step, outcome)\`) with \`onboarding.test.ts\` covering: the happy
    path; each of the four failures routing to its own screen; skip; and
    \`onboarded_is_only_set_after_a_success_or_an_explicit_skip\`.

    Depends on PERM-B, PERM-E, Y5-F.

    What NOT to do:
      - Do NOT paste into another application during onboarding. The target is a
        field inside Yap's own window; pasting into whatever was focused before
        onboarding is a surprise and a paste-target violation (YV21).
      - Do NOT set \`onboarded: true\` on mount.
  `,
  acceptance: `
    test -f desktop/src/onboarding.ts
    test -f desktop/src/onboarding.test.ts
    grep -q 'onboarded_is_only_set_after_a_success_or_an_explicit_skip' desktop/src/onboarding.test.ts
    grep -qE '"try-it"' desktop/src/onboarding.ts
    test 4 -le "$(grep -c 'case ' desktop/src/onboarding.ts)"
    cd ${APP} && npm ci
    npm test         ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build    ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'PRIV-A', prompt: 'Y6', branch: 'loop/priv-a-crash-reporting-stays-local-and-says-so', gated: null,
  title: 'Crash reporting is local, complete and provably offline — no Sentry, no PostHog, ever',
  preflight: `
    test -f desktop/src-tauri/tests/no_outbound_on_the_dictation_path.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test no_outbound
  `,
  spec: `
    \`src-tauri/src/crash.rs\` (777 lines, YV64) is local crash capture with a
    committed fixture (tests/fixtures/crash/wilson-voice-crash.ips), and the
    support bundle has redaction tests
    (tests/support_bundle_redaction.rs, tests/support_bundle_contents.rs).
    \`git grep -i "sentry|posthog" -- desktop\` returns nothing. This item keeps
    it that way and turns the claim into a test, because it is a shipped
    marketing claim and the sharpest one Yap has.

    reference_wispr_parity_research §5.2 records the claim and the missing
    proof, verbatim: "Audio never leaves the machine ... Assert it: a test that
    fails if the dictation path opens any outbound connection (already queued as
    P3.13)". And: "No telemetry — local-only crash capture (YV64). Wispr ships
    PostHog + Sentry + Segment. Name it."

    Do:
      * \`tests/no_outbound_on_the_dictation_path.rs\`:
        - a SOURCE sweep asserting no module on the take path (record, vad,
          transcription, asr_engine, dictation, polish, polish_protocol, paste,
          paste_tx, focus, snippets, db) references an HTTP client, a socket or
          a URL literal. Pattern AND scope, both named in the test.
        - a DEPENDENCY sweep: the modules above must not import the HTTP client
          crate at all. Model-download code may; the take path may not. If the
          crate graph makes that unprovable by grep, state so and assert the
          narrower thing you CAN prove, naming the gap.
        - an assertion that the ONLY network call sites in the whole crate are
          the model download, the revocation list refresh and the updater
          check — an explicit allowlist by file and function that a new call
          site forces you to update.
      * \`crash.rs\` completeness: a crash report must never contain transcript
        text, a file path inside the user's home beyond the app's data dir, or a
        license key. Extend the redaction tests with a report synthesized to
        contain all three and assert all three are gone.
      * Nothing in crash.rs may upload. The support bundle is produced for the
        USER to send; assert there is no send path.
      * Update PRIVACY.md to state the claim in the exact words the test proves,
        and add the test's name to the doc so the claim is traceable. Do not
        soften the claim and do not overstate it: the model download, the
        revocation refresh and the updater DO talk to the network, and PRIVACY.md
        must say which three and that none of them carry audio or text.

    What NOT to do:
      - Do NOT add Sentry, PostHog, Segment or any analytics SDK.
      - Do NOT add an opt-in telemetry toggle. There is no telemetry to toggle.
      - Do NOT claim in PRIVACY.md that Yap makes no network calls at all. Three
        calls exist and naming them is what makes the rest credible.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/no_outbound_on_the_dictation_path.rs
    grep -q 'only_three_network_call_sites_exist' desktop/src-tauri/tests/no_outbound_on_the_dictation_path.rs
    grep -q 'the_take_path_has_no_url_literal' desktop/src-tauri/tests/no_outbound_on_the_dictation_path.rs
    grep -q 'no_send_path_in_crash_or_support' desktop/src-tauri/tests/no_outbound_on_the_dictation_path.rs
    grep -q 'no_outbound_on_the_dictation_path' PRIVACY.md
    test 0 -eq "$(git grep -ci 'sentry' -- desktop | wc -l)"
    test 0 -eq "$(git grep -ci 'posthog' -- desktop | wc -l)"
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test no_outbound_on_the_dictation_path ; test $? -eq 0
    cargo test --features custom-protocol --test support_bundle_redaction          ; test $? -eq 0
    cargo test --features custom-protocol --test support_bundle_contents           ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y6-B', prompt: 'Y6', branch: 'loop/y6-b-menu-bar-is-a-real-surface', gated: null,
  title: 'The menu bar becomes a usable surface: state, the last transcript, hide-for-an-hour, quit',
  preflight: `
    grep -q 'hide_for_an_hour\\|hideForAnHour' desktop/src-tauri/src/lib.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test tray_menu
  `,
  spec: `
    Yap has a tray (YV26) and \`sync_tray\` (lib.rs:4468). Wispr's status menu,
    from reference_wispr_parity_research §2.2 [BUNDLE] \`hub_status_menu_*\`:
    "Formatting options · Languages · Microphone · Paste last transcript ·
    Transcript history · Settings · Hide for 1 hour · Show all", against which
    Yap scores 🟡 "tray menu (YV26)"; and §2.2 scores "Hide the bar" 🟡.

    Ship the subset that is real today and skip the rest:
      * State header (SEC-B / Y2-E already add the license line here — build on
        it, do not duplicate).
      * Paste last transcript. The command EXISTS — \`paste_last_transcript\`
        (lib.rs:2714) with a global binding (shortcuts.rs:86) — and is not in
        the menu. One line to expose, and it is the highest-value item on the
        list.
      * Copy last transcript. Missing entirely
        (reference_wispr_parity_research §2.1 scores it ❌). Add the command and
        the menu item; it is the clipboard sibling of the paste path and must
        use the same receipt-sequenced discipline (YV39) so it cannot race
        a paste.
      * Transcript history -> focus the main window on the History view.
      * Settings -> focus the main window on Settings.
      * HIDE THE PILL FOR ONE HOUR, and "show it now". \`show_floating_pill\` is
        a persistent boolean (lib.rs:406); a temporary hide is a different thing
        and needs a deadline that survives nothing (not the setting, not a
        restart — a restart shows the pill again, which is the correct and
        forgiving behaviour). Say that in the doc comment.
      * Quit, which must run the existing exit drain (there is one — the meeting
        path has \`ABANDONED_FOR_EXIT\`, transcription.rs:84) rather than killing
        the process mid-take.

    \`sync_tray\` stays the ONLY place the tray is rebuilt. Keep
    \`tests/tray_hotkey_no_collision.rs\` green.

    Tests \`tests/tray_menu.rs\`: every menu item maps to a registered command;
    no item is unreachable; hide-for-an-hour expires; a restart during the hide
    window shows the pill; quit drains.

    Depends on Y2-E.

    What NOT to do:
      - Do NOT add Languages or Formatting submenus. Multi-language is Y10 and
        an empty submenu is worse than no submenu.
      - Do NOT make hide-for-an-hour persist across a restart.
  `,
  acceptance: `
    grep -q 'fn copy_last_transcript' desktop/src-tauri/src/lib.rs
    grep -qE 'hide_for_an_hour' desktop/src-tauri/src/lib.rs
    test -f desktop/src-tauri/tests/tray_menu.rs
    grep -q 'every_menu_item_maps_to_a_registered_command' desktop/src-tauri/tests/tray_menu.rs
    grep -q 'a_restart_during_the_hide_window_shows_the_pill' desktop/src-tauri/tests/tray_menu.rs
    grep -q 'quit_runs_the_exit_drain' desktop/src-tauri/tests/tray_menu.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test tray_menu                ; test $? -eq 0
    cargo test --features custom-protocol --test tray_hotkey_no_collision ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y6-C', prompt: 'Y6', branch: 'loop/y6-c-paste-target-and-secure-input-end-to-end', gated: null,
  title: 'The paste lands in the app you dictated into, or it does not paste — including secure-input fields',
  preflight: `
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test paste_target_e2e
  `,
  spec: `
    This is the property Yap can claim and Wispr's docs never do
    (reference_wispr_parity_research §5.2: "Paste goes only to the app you
    dictated into ... This is a security property Wispr's docs never claim").
    The machinery exists — \`focus.rs\` (342), \`paste.rs\` (557),
    \`paste_tx.rs\` (641, receipt-sequenced, YV39),
    \`secure_input.rs\` (395) — and yap8's M1 was an auto-paste-target fix. What
    is missing is one end-to-end test that the whole chain holds under the cases
    that actually happen.

    Cover, each as a named test in \`tests/paste_target_e2e.rs\`:
      * The focused app changes between the hold and the paste (the user
        cmd-tabs while Yap is transcribing). The paste must NOT go to the new
        app. It must be held, offered, or dropped with a message — pick one,
        say which in the doc comment, and make the pill say it (Y5-C's \`pasting\`
        phase and Y5-F's catalogue).
      * The focused app QUITS between the hold and the paste.
      * The target is a SECURE INPUT field (a password box).
        \`secure_input.rs\` exists for this; assert Yap refuses to paste, says
        why, and does not leave the text on the clipboard either — a password
        field's dictation sitting in the clipboard is a worse outcome than a
        refused paste.
      * Accessibility is granted but the target refuses synthesized ⌘V (some
        Electron and Java apps do). Fall back to the clipboard with an explicit
        message, never silently.
      * \`auto_paste: false\` (a real setting, lib.rs:405): the text goes to the
        clipboard and the pill says so. Assert no keystroke is synthesized.
      * A long take (Y3) whose paste arrives minutes after the hold: the target
        check must be re-run at PASTE time, not cached from hold time.
      * Two takes in quick succession cannot interleave their pastes. YV39's
        receipts exist for exactly this; assert ordering.

    Where a case cannot be driven headlessly, drive the decision function and
    say in the PR body which cases were proven by test and which by a named
    manual check with a screenshot. Do not claim a test that does not exist —
    a false capability claim is a blocking finding.

    Depends on Y5-C, Y5-F, Y3-B.

    What NOT to do:
      - Do NOT paste into a target that was not the hold target.
      - Do NOT leave dictated text on the clipboard after a refused secure-input
        paste.
      - Do NOT cache the target from hold time for a long take.
    PANEL 2026-09-12 — DECIDED, do not leave this to a doc comment. The first
    draft said the orphaned text "must be held, offered, or dropped with a
    message — pick one, say which in the doc comment". Those are three different
    products and the most consequential of the three was not in the plan's open
    decisions. It matters more after Y3-B, because the normal case becomes: the
    user stops talking, switches app while the decode runs, and the take has
    nowhere to go (paste.rs:99 \`is_same_paste_target\`, :186-192 samples the
    CURRENT frontmost app immediately before the synthesized paste, :232 "paste
    not confirmed").
    THE ANSWER IS HELD AND OFFERED. The text parks in the pill with ONE key that
    inserts it wherever the user is now, plus a History row. It is never
    silently clipboard-only behind a two-second toast, and it is never dropped —
    twelve minutes of talking is not a thing this app throws away. Name the
    test \`a_long_take_whose_target_moved_is_held_and_insertable\`, and state the
    same terminal state in Y3-B's and Y3-C's pill copy so the three agree.
    Wilson can override the choice; a builder cannot.

  `,
  acceptance: `
    test -f desktop/src-tauri/tests/paste_target_e2e.rs
    grep -q 'focus_changed_between_hold_and_paste_does_not_paste_to_the_new_app' desktop/src-tauri/tests/paste_target_e2e.rs
    grep -q 'secure_input_refuses_and_leaves_no_clipboard_residue' desktop/src-tauri/tests/paste_target_e2e.rs
    grep -q 'target_is_rechecked_at_paste_time_for_a_long_take' desktop/src-tauri/tests/paste_target_e2e.rs
    grep -q 'two_takes_cannot_interleave_their_pastes' desktop/src-tauri/tests/paste_target_e2e.rs
    grep -q 'auto_paste_false_synthesizes_no_keystroke' desktop/src-tauri/tests/paste_target_e2e.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test paste_target_e2e ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'DB-C', prompt: 'Y6', branch: 'loop/db-c-history-search-and-export-hold-up', gated: null,
  title: 'History, FTS search and export hold up at real volume, and Clear History still destroys the words',
  preflight: `
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test history_at_volume
  `,
  spec: `
    \`db.rs\` is 3,531 lines: SQLite WAL + FTS5, a dictionary, snippets, a
    scratchpad table (db.rs:648), insights rollups, and YV78's secure delete
    ("Clear history actually destroys the words — secure_delete, FTS rebuild,
    VACUUM", commit 16e2f71). History is also the surface that must keep working
    forever past the trial — \`KEEP_FOREVER_LINE\` in src/license/status.ts is a
    promise the app makes in writing, and lib.rs:1104-1106 lists history,
    search and export as things the license gate never touches.

    Prove it at volume, in \`tests/history_at_volume.rs\`:
      * Seed 10,000 takes with realistic text lengths (including one 4,000-word
        long-form take from Y3). Assert: the History view's first page query is
        bounded (a LIMIT, not a full scan); FTS search of a common word returns
        in a bounded time; and the day-series insights query does not scan the
        whole table.
      * Pagination correctness: no duplicated and no skipped row across pages
        with takes sharing a timestamp. An ORDER BY on a non-unique column is
        the classic bug here; assert a tiebreaker exists.
      * Export: full export of 10,000 rows streams rather than building one
        string in memory, and the export contains no license key and no absolute
        home path.
      * YV78 regression: after Clear History, the FTS index holds no residue for
        a word that was present, and the DB file has been VACUUMed. Assert the
        WORD is gone from the index, not merely that the row count is zero.
      * The scratchpad table (db.rs:648, :2667-2721) is covered too: it is
        user-typed text (db.rs:1191 classifies it as such) and Clear History must
        make a deliberate, documented choice about it. Say which and test it.
      * Migration idempotence is already covered
        (tests/db_migration_idempotent.rs) — keep it green and extend it to any
        migration this item adds.

    Depends on nothing in this file; can run early. Y3's long takes make the
    4,000-word case real rather than synthetic, so order it after Y3-B if the
    lane allows.

    What NOT to do:
      - Do NOT add an index without measuring. Say what each new index costs on
        insert; the take path is latency-critical (latency.rs instruments it).
      - Do NOT gate history, search or export behind the license under any
        circumstance.
      - Do NOT write transcript text into any log while testing.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/history_at_volume.rs
    grep -q 'first_page_query_is_bounded' desktop/src-tauri/tests/history_at_volume.rs
    grep -q 'pagination_has_a_tiebreaker_and_never_duplicates_a_row' desktop/src-tauri/tests/history_at_volume.rs
    grep -q 'clear_history_leaves_no_fts_residue_for_a_known_word' desktop/src-tauri/tests/history_at_volume.rs
    grep -q 'export_streams_and_leaks_no_key_or_home_path' desktop/src-tauri/tests/history_at_volume.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test history_at_volume      ; test $? -eq 0
    cargo test --features custom-protocol --test db_migration_idempotent ; test $? -eq 0
    cargo test --features custom-protocol --test license_gate            ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'PRIV-B', prompt: 'Y6', branch: 'loop/priv-b-clear-history-erases-the-audio-too', gated: null,
  title: 'Clear History erases the audio and the partial words, not only the SQLite rows',
  preflight: `
    grep -q 'recovery_dir' desktop/src-tauri/src/lib.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test erase_everything
  `,
  spec: `
    PANEL 2026-09-12 — this is a privacy regression this LOOP creates, so it
    ships inside the loop. MEASURED at 4e8c9adf:
      lib.rs:2386-2388  clear_history() is one line: state.db.clear_transcripts()
      lib.rs:565-570    recovery_dir() is "deliberately NOT the recordings dir:
                        record::sweep_stale_wavs empties that at every startup",
                        and is purged only after FAILED_TAKE_RETENTION_DAYS
      db.rs:4005        clear_history_leaves_no_plaintext_on_disk scans only
                        wilson_voice.db / -wal / -shm
    Today a dictation clip is transient. Three items in this loop convert that
    into days of retained raw audio and partial transcript text — Y3-A spills
    the take to disk, Y3-D specifies that "a cancel NEVER deletes the clip, it
    parks it in the recovery dir with the same 7-day purge lifecycle", DB-B
    persists per-take chunk TEXT — while DB-C's erase work covers the .db only.
    So after this loop a user who dictates something regrettable, cancels, and
    clicks Clear History has their rows VACUUMed and the full audio plus the
    partial words still on disk for a week, with nothing saying so.

    Do:
      * Erasure becomes one operation over the whole product: Clear History also
        deletes recovery/ WAVs, spilled take WAVs, take_chunks rows and
        orphaned meetings/ audio. Plus a separate, explicit
        "Delete all recordings" control.
      * Make the retention VISIBLE: Y3-D's and DB-B's parked clips render in
        History as "N clips kept for 7 days — review or delete". Invisible
        retained audio is the scare; visible retained audio is a feature.
      * Test on the FILESYSTEM, not the query layer: plant a sentinel WAV in
        recovery/ and a take_chunks row, run the command, assert both are gone
        from disk. Under YAP_DATA_DIR (Y0-D), never the real data dir.
      * Say in ARCHITECTURE.md what is kept, where, and for how long.

    Runs after DB-B and DB-C, whose retention this item is the counterweight to.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/erase_everything.rs
    grep -q 'a_sentinel_wav_in_recovery_is_gone_from_the_filesystem' desktop/src-tauri/tests/erase_everything.rs
    grep -q 'take_chunks_rows_are_gone_not_just_unqueryable' desktop/src-tauri/tests/erase_everything.rs
    grep -q 'orphaned_meeting_audio_is_swept' desktop/src-tauri/tests/erase_everything.rs
    grep -q 'YAP_DATA_DIR' desktop/src-tauri/tests/erase_everything.rs
    grep -rq 'kept for' desktop/src
    grep -q 'retention' ARCHITECTURE.md
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test erase_everything ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y6-D', prompt: 'Y6', branch: 'loop/y6-d-launch-sleep-wake-and-single-instance', gated: null,
  title: 'Cold launch, sleep/wake, display change and a second copy of Yap all behave',
  preflight: `
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test lifecycle_e2e
  `,
  spec: `
    The matrix tests cover these for MEETINGS and not for the app as a whole:
    \`tests/matrix_row15_single_instance.rs\`, \`matrix_row16_sleep_wake.rs\`,
    \`matrix_row14_output_device_change.rs\`, \`matrix_phase_offline.rs\`,
    \`matrix_row12_macos_144_gate.rs\`. Every one of them is an event that also
    breaks dictation, the pill and the hotkey.

    Cover, in \`tests/lifecycle_e2e.rs\`:
      * COLD LAUNCH with no model, no grants, no settings file: the app opens,
        lands on onboarding, and the pill either does not appear or appears in a
        state that explains itself (Y5-C's \`model_loading\` / PERM-C's
        \`blocked\`). It must never appear as a normal ready pill it cannot honour.
      * SLEEP/WAKE mid-take: the take is either completed or parked in recovery,
        never half-written. \`power.rs\` observes this already — assert the
        dictation path subscribes, not only the meeting path.
      * DISPLAY CHANGE / a monitor unplugged while the pill is docked to it: the
        pill must land on a visible screen, not at a negative coordinate
        off-screen. This is the classic floating-HUD bug and there is no test
        for it.
      * FULLSCREEN: ROADMAP.md records that "a normal NSWindow cannot float above
        FULLSCREEN apps" and names \`tauri-nspanel\` as the real fix. Yap sets
        \`macOSPrivateApi: true\`. Measure the current behaviour over a fullscreen
        app and write the ANSWER into the test as an assertion or into the doc
        as a named limitation with the evidence. Do not claim it works without
        measuring; do not silently leave it unknown.
      * OUTPUT DEVICE CHANGE while muted-for-dictation: YV28 snapshots and
        restores the exact prior mute state; assert a device swap mid-take does
        not leave the Mac permanently muted. That is the worst-feeling bug in
        this list.
      * SECOND INSTANCE: launching Yap twice focuses the first and exits, and
        does not open a second SQLite handle on the same WAL.
      * The four TCC grants surviving a relaunch (PERM-E's watcher) with no
        prompt storm on launch.

    Depends on PERM-C, PERM-E, Y5-C, DB-B.

    What NOT to do:
      - Do NOT claim fullscreen works without a measurement.
      - Do NOT leave the system output muted on any exit path.
      - Do NOT add tauri-nspanel in this item. Measure first; the port is its
        own item if the measurement says it is needed.
    PANEL 2026-09-12 — two corrections; without them this item lands green
    evidence for untested lifecycle behaviour, which is worse than an open gap.
    (a) DELETE "power.rs observes this already — assert the dictation path
        subscribes". It does not (power.rs:63-135 is IOPMAssertion only;
        meeting_matrix.rs:398-408 records the absent call site). Y1-B writes the
        observer and runs first; subscribe to IT.
    (b) A \`cargo test\` process has no window-server session: it cannot sleep
        the machine, unplug a monitor, launch a second copy of Yap or change a
        TCC grant. So SPLIT the seven promises by what can actually be proven:
          * State-machine level (cargo test, keep here): the sleep/wake handler's
            decision table, the display-change placement function, the
            device-swap unmute rule, the single-instance guard's logic.
          * Observed level (Y0-E's windowed smoke, under YAP_DATA_DIR): cold
            launch with nothing installed, the pill landing on a visible screen,
            fullscreen float.
          * MANUAL, and written down as a checklist in docs/RELEASE.md with a
            date and a machine: actual sleep/wake mid-take, an actual monitor
            unplug, TCC surviving a relaunch. A named manual row is honest; a
            green unit test standing in for it is not.
        Do not name a test after a behaviour the test cannot reach.

  `,
  acceptance: `
    test -f desktop/src-tauri/tests/lifecycle_e2e.rs
    grep -q 'cold_launch_with_nothing_installed_never_shows_a_ready_pill' desktop/src-tauri/tests/lifecycle_e2e.rs
    grep -q 'display_change_lands_the_pill_on_a_visible_screen' desktop/src-tauri/tests/lifecycle_e2e.rs
    grep -q 'device_swap_mid_take_never_leaves_the_mac_muted' desktop/src-tauri/tests/lifecycle_e2e.rs
    grep -q 'second_instance_focuses_the_first_and_exits' desktop/src-tauri/tests/lifecycle_e2e.rs
    grep -qE 'fullscreen' desktop/src-tauri/tests/lifecycle_e2e.rs ARCHITECTURE.md
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test lifecycle_e2e                ; test $? -eq 0
    cargo test --features custom-protocol --test matrix_row15_single_instance ; test $? -eq 0
    cargo test --features custom-protocol --test matrix_row16_sleep_wake      ; test $? -eq 0    grep -q 'LIFECYCLE MANUAL CHECKLIST' docs/RELEASE.md
    grep -q 'subscribes to power::' desktop/src-tauri/tests/lifecycle_e2e.rs

  `,
})

ITEMS.push({
  id: 'Y6-E', prompt: 'Y6', branch: 'loop/y6-e-docs-match-the-app', gated: null,
  title: 'README, ARCHITECTURE, ROADMAP and PRODUCT stop describing an app that no longer exists',
  preflight: `
    test 0 -eq "$(grep -c 'MLX Whisper' ROADMAP.md)"
    test 0 -eq "$(grep -c 'Wilson Voice' README.md)"
    grep -q 'Runtime Dependencies' ARCHITECTURE.md
  `,
  spec: `
    MEASURED. \`ROADMAP.md\` opens "Progress as of 2026-07-17" and its "What
    works today (v0.4.1)" table says: Hotkey = "Carbon ⌘⇧V hold", ASR =
    "MLX Whisper large-v3-turbo", Mic = "In-process cpal (TCC identity = Wilson
    Voice)". All three are wrong at 4e8c9adf: YV34 deleted the Python/MLX
    sidecar and made the embedded GGUF engine "the app's ONLY transcriber"
    (lib.rs:539, :1298), the default binding is \`fn⌃\` (lib.rs:404), the product
    is named Yap, and \`tauri.conf.json\` says version 0.8.0. ROADMAP's "Next
    build slices" lists as pending several things that shipped (warm daemon,
    Developer ID notarization).

    A stale ROADMAP is not cosmetic: every agent in this loop reads the repo
    docs as a spec source (docs/loop/HARNESS.md names PRODUCT.md, ROADMAP.md,
    ARCHITECTURE.md and docs/ as the spec sources), so a wrong table is a wrong
    instruction that propagates.

    Do:
      * ROADMAP.md: replace the "what works today" table with the measured truth
        at this commit, and move everything shipped into a "shipped" section
        with its YV number. Keep the research notes — the permissions and
        fullscreen notes are still accurate and load-bearing.
      * README.md: the product is Yap. Keep the bundle identifier
        \`com.wilsonguenther.wilson-voice\` and the data dir \`WilsonVoice\`
        documented as DELIBERATELY unchanged, with the reason (TCC grants and
        the SQLite history). That is the single most important sentence in the
        file for anyone who might "tidy" them.
      * ARCHITECTURE.md gains a RUNTIME DEPENDENCIES table: for each of the ASR
        model, the polish model, the yap-polish sidecar, the yap-diarize sidecar
        and the sherpa-onnx prebuilt archive — is it SHIPPED in the bundle, or
        MANAGED (downloaded+verified by the app), and where does it land on
        disk. Nothing may be listed as "assumed present on the machine". Note
        the sherpa fetch-at-build-time behaviour that .github/workflows/ci.yml
        documents at length, and the two escape hatches
        (\`SHERPA_ONNX_ARCHIVE_DIR\`, \`SHERPA_ONNX_LIB_DIR\`).
      * PRODUCT.md: one honest feature list at 0.8.0 including the notetaker and
        the license model, and the three network calls PRIV-A names.
      * A test that keeps them honest:
        \`tests/docs_match_the_app.rs\` asserting the version in ARCHITECTURE.md
        matches tauri.conf.json, the default binding named in README matches
        \`AppSettings::default().ptt_binding\`, and no doc mentions a deleted
        subsystem (MLX, the Python sidecar, \`⌘⇧V\` as the default).

    What NOT to do:
      - Do NOT rename the bundle identifier or the data directory. Document them.
      - Do NOT delete ROADMAP's research notes.
      - Do NOT write aspirational features into PRODUCT.md as shipped.
    PANEL 2026-09-12 — the docs must also answer the question the plan never
    asks: WHICH MAC IS THE WEAKEST ONE THIS MUST WORK ON. Declare it in
    ARCHITECTURE.md (chip, macOS version, RAM, free disk) and make the config
    honest about it: tauri.conf.json pins \`minimumSystemVersion: "12.0"\`,
    which invites 8 GB Intel Macs, while
    \`git grep -E 'x86_64-apple|universal-apple' -- .github desktop/package.json
    desktop/src-tauri/tauri.conf.json\` returns NOTHING and the only staged
    sidecar is aarch64-apple-darwin — so the shipped DMG cannot run on the
    machines the Info.plist invites. Building universal is out of scope for this
    loop; raising minimumSystemVersion to the arm64 reality is a one-line
    change and is Wilson's call (docs/loop/PLAN.md §4). Whichever he picks,
    ARCHITECTURE.md states the floor and Y3-G's budgets are asserted against it.

  `,
  acceptance: `
    test 0 -eq "$(grep -c 'MLX Whisper' ROADMAP.md)"
    test 0 -eq "$(grep -c 'v0.4.1' ROADMAP.md)"
    grep -q 'Runtime Dependencies' ARCHITECTURE.md
    grep -q 'SHERPA_ONNX_ARCHIVE_DIR' ARCHITECTURE.md
    grep -q 'com.wilsonguenther.wilson-voice' README.md
    grep -q 'WilsonVoice' README.md
    test -f desktop/src-tauri/tests/docs_match_the_app.rs
    grep -q 'version_in_docs_matches_tauri_conf' desktop/src-tauri/tests/docs_match_the_app.rs
    grep -q 'no_doc_mentions_a_deleted_subsystem' desktop/src-tauri/tests/docs_match_the_app.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test docs_match_the_app ; test $? -eq 0
  `,
})

// ── 35-y7-tests-and-smoke.mjs ─────────────────────────────────────────────
// Y7 — TESTS + A REAL SMOKE. The gate in docs/loop/HARNESS.md is eight commands
// and none of them launches the app. Yap has 128 Rust integration test files and
// 25 vitest tests, and Wilson's report ("the app looks broken") was invisible to
// all of them — a green build is not a working app
// (feedback_loop_blind_to_visual_ux; feedback_real_browser_smoke_required).
//
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'Y7-A', prompt: 'Y7', branch: 'loop/y7-a-headless-smoke-against-the-built-binary', gated: null,
  title: 'A headless smoke that runs the real built binary end to end, not a unit test of its parts',
  preflight: `
    test -x scripts/smoke-headless.sh
    ./scripts/smoke-headless.sh
  `,
  spec: `
    Yap ALREADY has the hook this needs and nothing uses it as a gate:
    \`src-tauri/src/cli.rs\` (161 lines) plus "YV32 headless mode
    (\`--transcribe-file <wav>\`)" at lib.rs:3982, and a committed fixture
    \`tests/fixtures/quick-brown-fox-16k.wav\`. So the built binary can be driven
    with no window, no TCC and no microphone.

    Create \`scripts/smoke-headless.sh\` (\`set -euo pipefail\`, exit codes read
    bare), which:
      1. Builds the release binary the way the gate already does (stage
         \`src-tauri/binaries/yap-polish-<triple>\` first, then
         \`cargo build --release --features custom-protocol\`).
      2. Runs the weak-link check that already exists:
         \`./scripts/assert-weak-linked-14_4-symbols.sh
          desktop/target/release/wilson-voice\`. Its comment in ci.yml is the
         reason this whole item matters: "the build is green, the tests are
         green, and the binary is unlaunchable for a whole population of users."
      3. \`--transcribe-file tests/fixtures/quick-brown-fox-16k.wav\` against a
         TEMPORARY data dir (never the user's \`WilsonVoice\` dir) and asserts the
         transcript contains the expected words. If the ASR model is absent it
         must FAIL with "no model installed, run <the documented command>" —
         never skip silently, which is how a smoke test becomes decoration.
      4. Asserts the run wrote NOTHING into the real data dir.
      5. Runs the same file through the cleanup pipeline at the SHIPPED default
         level and prints the before/after, so Y4's "formatting is on" claim is
         visible in the smoke output rather than only in a fixture.
      6. Prints a single PASS/FAIL summary and the version from tauri.conf.json.

    Add \`"smoke:headless"\` to desktop/package.json scripts. This is the
    per-item smoke; Y7-B is the windowed one.

    What NOT to do:
      - Do NOT touch the user's data dir or their \`/Applications/Yap.app\`.
      - Do NOT skip when the model is missing.
      - Do NOT download a model inside the smoke script.
  `,
  acceptance: `
    test -x scripts/smoke-headless.sh
    grep -q 'set -euo pipefail' scripts/smoke-headless.sh
    grep -q 'assert-weak-linked-14_4-symbols.sh' scripts/smoke-headless.sh
    grep -q 'transcribe-file' scripts/smoke-headless.sh
    test 0 -eq "$(grep -c 'WilsonVoice' scripts/smoke-headless.sh)"
    node -e "process.exit(require('./desktop/package.json').scripts['smoke:headless']?0:1)"
    ./scripts/smoke-headless.sh ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y7-B', prompt: 'Y7', branch: 'loop/y7-b-windowed-smoke-that-screenshots-every-view', gated: null,
  title: 'A windowed smoke that launches Yap, walks all seven views and captures them at two sizes',
  preflight: `
    test -x scripts/smoke-windowed.sh
    test -d docs/smoke-shots
    ./scripts/smoke-windowed.sh --check-only
  `,
  spec: `
    Wilson's report was visual and nothing in the gate looks at pixels. This is
    the item that makes "looks broken" a detectable condition.

    \`scripts/smoke-windowed.sh\`:
      * Launches the built app against a TEMPORARY data dir, with a flag that
        seeds a deterministic fixture state (some history, a dictionary entry, a
        scratchpad note) and a SECOND run with an EMPTY state — the empty run is
        the one that catches Y5-B's missing empty states, and it is the more
        important of the two.
      * Walks all seven \`Nav\` views and all eight \`SettingsTab\`s, capturing
        each at 980x700 and at 720x520 (the configured default and the
        \`minWidth\`/\`minHeight\` floor from tauri.conf.json), into
        \`docs/smoke-shots/<run>/\`.
      * Captures the FLOAT PILL for every \`LivePhase\` at all three
        \`pill_position\` values, driven through a debug command that forces a
        phase (add one behind \`#[cfg(feature = "custom-protocol")]\` plus an env
        guard so it cannot be reached in a shipped build — and assert that).
      * FAILS, not warns, on: a view that renders no text at all; a view with an
        \`.animate\`/spinner element still present after 10 seconds; a horizontal
        scrollbar on the window at either size; any element whose bounding box
        extends past the window; and, in the empty run, a view with no
        \`data-empty-state\`. These are mechanical proxies for "looks broken" and
        each one corresponds to a defect this plan found.
      * \`--check-only\` runs the assertions against the last captured run
        without relaunching, so the preflight is cheap.

    Use the Playwright/CDP route only if the Tauri webview exposes a debug port
    in a \`custom-protocol\` build; if it does not, drive it with the OS
    screenshot tools plus the app's own debug commands and say so in the script
    header. Either way, do not add a browser automation dependency to
    desktop/package.json's runtime deps.

    Wire it in: \`npm run smoke:windowed\`, and make Y0-B's loop-smoke script
    call it when a display is available and skip it with a named message when
    there is none.

    Depends on Y5-B, Y5-C, Y5-G, Y7-A.

    What NOT to do:
      - Do NOT commit the screenshots into git history on every run. Commit ONE
        reference run and gitignore the rest, or the repo grows without bound.
      - Do NOT leave the forced-phase debug command reachable in a release build.
      - Do NOT assert pixel equality against a golden image. Assert the
        STRUCTURAL properties above; pixel goldens on a two-theme, two-size,
        animated UI are a permanent source of false red.
    PANEL 2026-09-12 — THIS ITEM IS NOW THE SECOND HALF. Y0-E ships
    scripts/smoke-windowed.sh with the five structural failure conditions BEFORE
    the Y5 lane, because an instrument that arrives ten items after the work it
    judges gates nothing, and build mode dispatches no reviewer. So the
    dependency line "Depends on Y5-B, Y5-C, Y5-G, Y7-A" now means: EXTEND Y0-E's
    script to the new pill phases and the dock positions those items added, and
    re-baseline docs/loop/SMOKE-BASELINE.md against the post-Y5 tree with the
    diff explained. It launches under \`YAP_DATA_DIR\` and \`--smoke\` (Y0-D) —
    never against the real data dir, and never with a global hotkey registered.
    \`--check-only\` remains a pre-flight convenience and is NOT acceptance:
    with no captured run it exits non-zero by Y0-E's contract.
    A display IS available on this run, so "no display" is a failure here, not
    a named skip.

  `,
  acceptance: `
    test -x scripts/smoke-windowed.sh
    grep -q '720' scripts/smoke-windowed.sh
    grep -q '980' scripts/smoke-windowed.sh
    grep -q 'data-empty-state' scripts/smoke-windowed.sh
    grep -q 'check-only' scripts/smoke-windowed.sh
    test -d docs/smoke-shots
    node -e "process.exit(require('./desktop/package.json').scripts['smoke:windowed']?0:1)"
    grep -q 'smoke-windowed' scripts/loop-smoke.sh
    # the forced-phase debug command cannot exist in a shipped build
    grep -q 'custom-protocol' desktop/src-tauri/src/lib.rs
    # PANEL: --check-only asserts against "the last captured run", so with no
    # run it is a no-op that exits 0. Acceptance runs a FULL capture, counts the
    # shots, and proves the detector can fail.
    YAP_DATA_DIR="$(mktemp -d)" ./scripts/smoke-windowed.sh ; test $? -eq 0
    test 28 -le "$(ls docs/smoke-shots/*/*.png | wc -l | tr -d ' ')"
    ./scripts/smoke-windowed.sh --check-only --self-test-must-fail ; test $? -ne 0
  `,
})

ITEMS.push({
  id: 'Y7-C', prompt: 'Y7', branch: 'loop/y7-c-non-vacuous-mutation-proof-for-the-new-tests', gated: null,
  title: 'Every test this loop added is proven non-vacuous by a mutation that makes it fail',
  preflight: `
    test -f docs/loop/MUTATIONS.md
    test -x scripts/assert-tests-are-non-vacuous.sh
    ./scripts/assert-tests-are-non-vacuous.sh
  `,
  spec: `
    The repo already practises this: \`docs/pr-screenshots/YV105/…/non-vacuous-mutations.txt\`,
    YV106, YV107, YV120, YV121 all carry one. Make it mechanical for the tests
    this loop adds, because the failure mode is specific and this plan is full of
    greps: a test that asserts a symbol exists, in a file that always contains
    it, proves nothing; and a grep proving absence with the wrong pattern or the
    wrong scope proves less than nothing (the "verification that verifies
    nothing" rule).

    Create \`scripts/assert-tests-are-non-vacuous.sh\` driven by a committed
    table \`docs/loop/MUTATIONS.md\`: one row per test file added by this loop,
    naming a SINGLE-LINE source mutation and the test that must then fail.
    The script applies each mutation to a scratch copy, runs that one test,
    asserts a NON-ZERO exit, and reverts. It fails if any mutation leaves the
    suite green.

    Seed the table with the mutations named in the earlier items, which are
    already written as acceptance steps there and should move here so they run
    together:
      shipped_defaults      flip \`auto_paste: true\` -> false
      formatting_fixtures   set \`cleanup_level\` back to "light"
      mic_auth_status       return \`Authorized\` unconditionally from
                            \`authorization_status()\`
      mic_gate              delete the microphone check from \`start_recording\`
      trial_state_machine   drop the max-seen-wall-clock floor
      dictation_chunked     remove the seam dedupe
      dictation_capture_memory  restore the unbounded \`raw\` append
      polish_long_form      restore \`MAX_POLISH_WORDS = 400\`
      paste_target_e2e      cache the paste target from hold time
      updater_endpoint      reduce the endpoint list to one
      no_outbound_on_the_dictation_path  add a URL literal to \`record.rs\`
      history_at_volume     remove the pagination tiebreaker
      lifecycle_e2e         skip the mute restore on one exit path

    Also assert the SHAPE of every grep-based acceptance this loop uses: a
    committed checker that scans the item files for \`grep\` invocations without
    a path scope, and fails. A scopeless grep in an acceptance gate is the
    single cheapest way to ship a false green.

    Run it as part of Y0-B's loop-smoke script, gated behind a flag so the
    per-item gate stays fast and the full mutation sweep runs once per part.

    Depends on every test-bearing item; sequence it last in its file.

    What NOT to do:
      - Do NOT mutate the test file to make it fail. Mutate the SOURCE.
      - Do NOT accept "the whole suite went red" as proof. The NAMED test must
        be the one that fails.
    PANEL 2026-09-12 — the item whose purpose is proving other tests non-vacuous
    was itself satisfiable by typing a 13-row markdown table, and it is
    sequenced last, so it can only audit tests that have already merged. Two
    changes: (1) the per-item mutation requirement now lives in the SHARED
    PREAMBLE and binds every item as it is built — this item COLLECTS the rows
    and re-runs them, it does not excuse anyone; (2) the ledger is one row per
    NEW TEST FILE, and scripts/assert-tests-are-non-vacuous.sh must EXECUTE each
    mutation (mutate, re-run, require red, restore, \`git diff --exit-code\`) and
    record the observed transition, not merely list it. A row whose mutation was
    never executed is a failed row.

  `,
  acceptance: `
    test -f docs/loop/MUTATIONS.md
    test -x scripts/assert-tests-are-non-vacuous.sh
    # PANEL: 13 rows is not a ledger for a loop that adds ~50 test files, and a
    # markdown table is not a proof. One row per new test FILE, and the script
    # must EXECUTE each mutation and record the observed red/green transition.
    test "$(ls desktop/src-tauri/tests/*.rs | wc -l | tr -d ' ')" -le "$(grep -c '^| ' docs/loop/MUTATIONS.md)"
    grep -q 'executed' docs/loop/MUTATIONS.md
    grep -q 'git diff --exit-code' scripts/assert-tests-are-non-vacuous.sh
    grep -q 'mic_auth_status' docs/loop/MUTATIONS.md
    grep -q 'no_outbound_on_the_dictation_path' docs/loop/MUTATIONS.md
    grep -q 'scopeless' scripts/assert-tests-are-non-vacuous.sh
    ./scripts/assert-tests-are-non-vacuous.sh ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y7-D', prompt: 'Y7', branch: 'loop/y7-d-frontend-coverage-for-the-pure-modules', gated: null,
  title: 'The pure frontend modules get real coverage, so the pill and the states are testable without a window',
  preflight: `
    test -f desktop/vitest.config.ts
    cd ${APP} && npm ci && npm test
  `,
  spec: `
    The frontend is 25 vitest tests across \`src/pill/live.test.ts\`,
    \`src/errors.test.ts\`, \`src/license/status.test.ts\`,
    \`src/meetings/*.test.ts\` and \`src/support/bundle.test.ts\`. The pattern is
    right — pure module, pure test, no rendering — and ci.yml explains why it is
    a gate: "a behavioural regression there shipped once because a type check was
    the only frontend gate."

    This loop adds a lot of pure modules (permission.ts, viewState.ts, toast.ts,
    diff.ts, onboarding.ts, pill/license.ts, pill/motion.ts, pill/hitbox.ts,
    pill/dock.ts, home/house.ts). Make the discipline enforceable:
      * \`desktop/vitest.config.ts\` with coverage thresholds that apply ONLY to
        the pure modules (an explicit include list — never a repo-wide number,
        which would either be trivially met or permanently red because App.tsx
        cannot be unit-tested).
      * A test that fails when a new file is added under \`src/pill/\` or a new
        \`*.ts\` pure module is added without a sibling \`*.test.ts\`. An explicit
        include list plus that check is what keeps the number honest.
      * Fix the reverse problem too: assert no pure module imports
        \`@tauri-apps/api\` — a pure module that invokes is not testable without a
        window, and that is how \`live.ts\` stays drivable. Components may import
        it; \`*.ts\` modules on the include list may not.

    Depends on Y5-*, Y6-A. Sequence after them.

    What NOT to do:
      - Do NOT set a global coverage threshold.
      - Do NOT add a DOM testing library to chase a number. The value here is
        the pure state machines, which need no DOM.
  `,
  acceptance: `
    test -f desktop/vitest.config.ts
    grep -q 'coverage' desktop/vitest.config.ts
    grep -q 'pill/live.ts' desktop/vitest.config.ts
    test -f desktop/src/purity.test.ts
    grep -q 'no_pure_module_imports_the_tauri_api' desktop/src/purity.test.ts
    grep -q 'every_pure_module_has_a_sibling_test' desktop/src/purity.test.ts
    cd ${APP} && npm ci
    npm test         ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y7-E', prompt: 'Y7', branch: 'loop/y7-e-release-dmg-smoke-on-a-clean-mac-path', gated: null,
  title: 'The shipped DMG is smoke-tested the way a first-time user meets it',
  preflight: `
    test -x scripts/smoke-dmg.sh
    grep -q 'smoke-dmg' docs/RELEASE.md
  `,
  spec: `
    The release path is real: \`.github/workflows/release.yml\` with six repo
    secrets set, a Developer ID certificate, notarization, and
    \`reference_yap_dmg_notarization\` recording a notarized v0.5.5 DMG plus the
    manual flow and the iCloud/keychain gotchas. v0.8.0 shipped with "the first
    working auto-update" (project_yap_build_state). None of that is smoke-tested
    from the user's side, and SEC-A + UPD-A both change things that only show up
    there.

    \`scripts/smoke-dmg.sh <path-to-dmg>\`, run manually and from docs/RELEASE.md:
      * \`spctl --assess --type exec -vv\` on the .app inside the mounted DMG ->
        accepted, source "Notarized Developer ID". An un-notarized build is a
        Gatekeeper wall for every user and nothing else in the pipeline sees it.
      * \`codesign -dv --verbose=4\` -> the signing identity is a Developer ID,
        not ad-hoc, and the team id matches what docs/RELEASE.md documents.
      * \`codesign -d --entitlements :-\` -> assert
        \`com.apple.security.app-sandbox\` is present and FALSE (the VALUE, not
        the key — SEC-A's rule), audio-input is true, and the two
        dylib-injection entitlements are absent.
      * The bundle identifier is \`com.wilsonguenther.wilson-voice\` — assert it
        has NOT changed. A rename silently resets every user's TCC grants.
      * Both sidecars are present inside the bundle's Resources and are
        themselves signed.
      * \`Info.plist\` carries all three usage strings (NSMicrophone,
        NSAudioCapture, NSAppleEvents). A missing one is a TCC failure with no
        dialog.
      * The updater manifest URL from UPD-A resolves and its signature verifies
        against the shipped pubkey — without installing anything.
      * \`xattr\` shows no quarantine-blocking detritus, and the resource-fork
        problem project_yap_build_state describes ("the tauri codesign flakes on
        resource-fork detritus") is checked for by name.
      * Prints nothing secret. No certificate serial, no key material, no
        app-specific password. The script must be safe to paste into a PR.

    Then wire it into docs/RELEASE.md as a required step before a release is
    announced, with the exact command.

    Depends on SEC-A, UPD-A.

    What NOT to do:
      - Do NOT run this in the per-item gate. It needs a built, signed,
        notarized DMG, which is minutes plus Apple's servers; it belongs to the
        release checklist and to \`mode: "review"\` with \`args: {dmg: true}\`.
      - Do NOT print any secret or certificate detail.
      - Do NOT install the DMG over the user's running /Applications/Yap.app.
    PANEL 2026-09-12 — \`bash -n\` is a syntax check, not a smoke test: the
    checklist this item writes (spctl assess, codesign verbose, entitlement
    values, bundle id, sidecar signatures, Info.plist usage strings, the updater
    manifest resolving) was never once executed. Run it for real against the
    DMG the review pass builds (\`args: {dmg: true}\`), or skip with a NAMED
    reason the PR body carries. And split the assertions into SEC-A's two
    profiles: a RELEASE profile (Developer ID + notarized + stapled) and a LOCAL
    profile (Apple Development, unnotarized, stable designated requirement) —
    asserting "Notarized Developer ID" unconditionally makes a correctly signed
    local DMG fail by construction. The updater assertions here are UPD-B's
    published triple (latest.json + .app.tar.gz + .sig), not the DMG alone.

  `,
  acceptance: `
    test -x scripts/smoke-dmg.sh
    grep -q 'spctl --assess' scripts/smoke-dmg.sh
    grep -q 'app-sandbox' scripts/smoke-dmg.sh
    grep -q 'com.wilsonguenther.wilson-voice' scripts/smoke-dmg.sh
    grep -q 'NSAudioCaptureUsageDescription' scripts/smoke-dmg.sh
    grep -q 'smoke-dmg' docs/RELEASE.md
    test 0 -eq "$(grep -c 'APPLE_PASSWORD' scripts/smoke-dmg.sh)"
    # PANEL: \`bash -n\` is a syntax check, not a smoke test. Run it against a
    # real artifact, or skip with a NAMED reason that the PR body carries.
    bash -n scripts/smoke-dmg.sh ; test $? -eq 0
    ./scripts/smoke-dmg.sh --require-artifact-or-name-the-reason ; test $? -eq 0
    grep -q 'codesign -dv' scripts/smoke-dmg.sh
    grep -q 'adhoc' scripts/smoke-dmg.sh
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
${RUNTIME_PROOF}

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
