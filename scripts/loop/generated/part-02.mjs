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
 *              args.lanes (build mode only; default 2) — `lanes: 1` runs ONE builder lane: lane A
 *              walks EVERY item in source order and lane B gets none (see "args.lanes" below).
 *   'review' — the SAME script run again afterwards with args {mode:'review'}, and ONLY part-01
 *              runs it: the pass is PR-DRIVEN. One TRIAGE agent turns the open loop-build PRs into
 *              an ordered queue (closing duplicates and superseded work first), then TWO REVIEW
 *              CHAINS consume that shared queue under a HARD CAP OF THREE CONCURRENT AGENTS.
 */

export const meta = {
  name: "yap-overhaul-all-part-02",
  description:
    "Part 02 of the Yap (wilson-voice) overhaul, two builder lanes: land whatever is already gate-green, pre-flight on unmodified main (already-done items are skipped), build, open a labelled PR. The adversarial review, the independent second-Opus gate and the merge bar run afterwards in the same script with args {mode:'review'}. 46 items (yap-qa-harness..SEC-B), 1 awaiting the Senior Panel. The gate is local (Actions is disabled by the account spending limit); the DMG is not in it.",
  phases: [
    { title: "Recon", detail: "two lane worktrees, one npm ci + one warm cargo build each, the loop-build label, ci-mode measured" },
    { title: "yap-qa-harness", detail: "Real permission + hotkey QA tools: scripts/qa/perm-probe.sh (signature, duplicates, LaunchServices, a labelled helper probe) and scripts/qa/hotkey-walk.sh (a real fn take on the installed Yap)" },
    { title: "yap-perm-truth", detail: "The permission banner tells the truth about THIS binary: re-checked on activation and every 5 s while shown, names the failing condition and the bundle path, names a duplicate Yap, and explains a stale grant" },
    { title: "yap-hotkey-stop", detail: "A take the hotkey started, the hotkey stops: fn key-up from any app, verified tap re-arm within 100 ms, a HID-state second mechanism, a 90 s still-recording prompt, and pill + menu bar stop" },
    { title: "yap-format-structure", detail: "Wispr-style structure from speech: enumerations become lists, topic-then-verdict becomes \"Topic: verdict\" bullets, topic shifts become paragraphs, formatted for the paste target" },
    { title: "yap-meetings-fix", detail: "Meetings works end to end: Stop produces a transcript and a summary, a silent call is never called a denial, and the walk proves it on the installed build" },
    { title: "yap-uiux-2026-10-16-W0", detail: "Walk the main build as a first-run user and a daily user, headless, and write down every screen that reads generic, uncontained, confusing or slow" },
    { title: "yap-uiux-2026-10-16-HOME", detail: "Home reads contained and obvious for both personas: every walk row on Home fixed" },
    { title: "yap-uiux-2026-10-16-DICT", detail: "Dictionary reads contained and obvious for both personas: every walk row on Dictionary fixed" },
    { title: "yap-uiux-2026-10-16-INSIGHTS", detail: "Insights reads contained and obvious for both personas: every walk row on Insights fixed" },
    { title: "yap-uiux-2026-10-16-MEETINGS", detail: "Meetings reads contained and obvious for both personas: every walk row on Meetings fixed" },
    { title: "yap-uiux-2026-10-16-SCRATCH", detail: "Scratchpad reads contained and obvious for both personas: every walk row on Scratchpad fixed" },
    { title: "yap-uiux-2026-10-16-SETTINGS", detail: "Settings is contained: bordered surfaces, a pinned footer, every walk row on Settings and its eight panes fixed" },
    { title: "yap-uiux-2026-10-16-ONBOARD", detail: "First run is obvious: Onboarding, ModelSetup and Permissions say what to grant and why, one step at a time; every walk row fixed" },
    { title: "yap-uiux-2026-10-16-Z", detail: "Re-walk after the flush: both personas, every screen, zero open rows, the full smoke green — the acceptance of the UI/UX pass" },
    { title: "yap-notetaker-onboard", detail: "A \"Set up Notetaker\" card: 2-minute tour, connect Apple Calendar, import from Granola or Otter, set up MCP — progress dots, dismissible, reopen from Help" },
    { title: "yap-notetaker-detect", detail: "Yap notices a meeting — a calendar event with a call link in the next 2 minutes, or Zoom/Meet/Teams/FaceTime with live audio — and the pill offers \"Record this meeting?\" with the consent line; it never auto-records" },
    { title: "yap-notetaker-notes", detail: "Notes worth reading: transcript in speaker turns, a summary with decisions and action items from the user’s chosen model, searchable, Markdown export, MCP-readable — nothing leaves the Mac unless the user picks a cloud model" },
    { title: "yap-structure-followups", detail: "The structure stage keeps every word and finishes every list: narration \"Then\"/\"Finally\" survive, four-step lists stay one list, product names keep their casing" },
    { title: "yap-recovery-followups", detail: "Recovered long takes: no duplicate history entries, no race, confirmation enforced in the backend" },
    { title: "yap-land-gate-pass-required", detail: "No PR lands without an Opus PASS bound to its head: one seat for standard, two different seats for security, enforced by scripts/loop/review-gate.mjs in every merge path (LAND, fix-land, land sweeper, drain)" },
    { title: "yap-tests-hermetic-power", detail: "Meeting tests stop reading the host battery: an injected PowerSource, pmset only in production wiring, so cargo test passes on a Mac at 5% on battery" },
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
  'desktop/src-tauri/capabilities/, desktop/src-tauri/tauri.conf.json, the updater/signing path, desktop/src-tauri/src/db or any migration, the polish_protocol/sidecar IPC, or any path or diff hunk matching updater|sign|notariz|keychain|permission|accessibility|microphone|network|http|upload|telemetry, or the loop harness itself (scripts/loop/, scripts/loop/review-gate.mjs included, scripts/cicd-loop-all.mjs) or .github/workflows/'
const SECURITY_IDS = 'SEC-|UPD-|PRIV-|DB-|PERM-'
/**
 * Stamped by build.mjs: true for every part of a multi-part run EXCEPT the last. Parts share the
 * worktrees, their node_modules and their cargo target dirs, so only the LAST part's Drain is
 * allowed to remove them.
 */
const KEEP_WORKTREE = true
/** Stamped by build.mjs like LOG: which part this is. The status board is keyed on it. */
const PART = 'part-02'
/**
 * Stamped by build.mjs: item id -> lane index, round-robin over the SOURCE ITEM FILES so that a
 * whole prompt group (whose items often depend on one another) stays sequential on one lane.
 */
const LANE_BY_ID = {"yap-qa-harness":1,"yap-perm-truth":1,"yap-hotkey-stop":1,"yap-format-structure":1,"yap-meetings-fix":1,"yap-uiux-2026-10-16-W0":1,"yap-uiux-2026-10-16-HOME":1,"yap-uiux-2026-10-16-DICT":1,"yap-uiux-2026-10-16-INSIGHTS":1,"yap-uiux-2026-10-16-MEETINGS":1,"yap-uiux-2026-10-16-SCRATCH":1,"yap-uiux-2026-10-16-SETTINGS":1,"yap-uiux-2026-10-16-ONBOARD":1,"yap-uiux-2026-10-16-Z":1,"yap-notetaker-onboard":1,"yap-notetaker-detect":1,"yap-notetaker-notes":1,"yap-structure-followups":1,"yap-recovery-followups":1,"yap-land-gate-pass-required":1,"yap-tests-hermetic-power":1,"Y4-A":0,"Y4-I":0,"SEC-C":0,"Y4-C":0,"Y4-D":0,"Y4-E":0,"Y4-F":0,"Y4-G":0,"Y4-H":0,"Y3-A":1,"Y3-B":1,"Y3-C":1,"Y3-D":1,"DB-B":1,"Y3-F":1,"Y3-G":1,"LIC-A":0,"Y2-A":0,"Y2-B":0,"Y2-C":0,"Y2-F":0,"Y2-D":0,"Y2-E":0,"DB-A":0,"SEC-B":0}
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
Add recordHead: "<HEAD>" and recordPosted: true|false to your return.
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

THEN THE REVIEW GATE, AND ONLY THEN THE MERGE:
${REVIEW_GATE_STEP}
--delete-branch is not optional: an undeleted branch is the stale-branch backlog this loop's Drain
then has to sweep. Use --admin so a self-opened PR with no approving review can land.
Every merge comment and every status-board row you write carries ci=local.

OUTCOMES IN THIS MODE:
  - every command 0, sha matches, MERGEABLE, review gate exit 0 -> merge. status "merged", ciCheckConclusion
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
  - that job == success AND mergeable == MERGEABLE AND every blocking finding disposed AND the
    review gate below exits 0 on that same headRefOid -> merge with the command the review gate gives
    (--match-head-commit <headRefOid>). status "merged" with the squash sha; the board row carries ci=github.
  - it is queued / in_progress / not yet created -> status "ci-pending", reason "ci-pending".
    DO NOT POLL, DO NOT SLEEP, DO NOT WAIT — the land sweeper settles it.
  - it == failure -> label ci-red and comment the FAILING STEP NAME. status "ci-red".
  - mergeable == CONFLICTING after your rebase -> label needs-rebase, status "needs-rebase".
NOTE: even in this mode the CI workflow does NOT run clippy or fmt as a gate (both are '|| true'),
and it does not build or stage yap-diarize at all, so a github-mode merge still owes the local
clippy run, the local fmt check and both sidecar builds. Run them and paste them.
${REVIEW_GATE_STEP}`

// ── verdict helpers (string-safe; agent returns may be objects or strings) ──
const asText = (v) => (typeof v === 'string' ? v : JSON.stringify(v ?? null))
const saysBlock = (v) => /"verdict"\s*:\s*"BLOCK"/i.test(asText(v))
const saysReviewed = (v) => /"status"\s*:\s*"reviewed"/i.test(asText(v))
const hasEmptyBlocking = (v) => /"blocking"\s*:\s*\[\s*\]/.test(asText(v))
/** A BLOCK verdict with an empty blocking[] is malformed → recorded as PASS + an advisory note. */
const isMalformedBlock = (v) => saysBlock(v) && hasEmptyBlocking(v)
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
  const heads = (vs) => vs.map((v) => String((v && typeof v === 'object' && v.recordHead) || '').toLowerCase()).filter((h) => SHA40.test(h))
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
// Kill switch: seat-models.json "enabled": false, or launch with args {seatModels: false}.
const SEAT_MODELS = {"enabled":true,"escalate":{"model":"opus","effort":"high"},"seats":{"record-failure":{"model":"haiku","effort":"low","require":["boardWritten"]},"preflight":{"model":"sonnet","effort":"medium","require":["exits","allZero"]},"acceptance":{"model":"sonnet","effort":"medium","require":["exits","allZero"]},"acceptance-rerun":{"model":"sonnet","effort":"medium","require":["exits","allZero"]},"recon":{"model":"sonnet","effort":"high","require":["ciMode"]},"reflect":{"model":"sonnet","effort":"medium"}}} // stamped by build.mjs from scripts/loop/seat-models.json
const TIER_OFF = typeof args !== 'undefined' && args && args.seatModels === false
const NEEDS_OPUS = 'NEEDS_OPUS'
const ESCALATE_MODEL = 'opus' // build.mjs refuses any other escalate model
const seatKeyOf = (label) => String(label || '').split(':')[0]
function seatTier(label) {
  if (TIER_OFF || !SEAT_MODELS || SEAT_MODELS.enabled !== true || !SEAT_MODELS.seats) return null
  const t = SEAT_MODELS.seats[seatKeyOf(label)]
  return t && typeof t.model === 'string' && t.model !== 'opus' ? t : null
}
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
/** agentR: tiered seats try the small model (twice on a null, once otherwise), then the opus path. */
async function agentR(prompt, opts) {
  const label = (opts && opts.label) || 'agent'
  const tier = seatTier(label)
  if (!tier) return agentRRetry(prompt, opts)
  const seat = seatKeyOf(label)
  const small = { ...opts, model: tier.model, effort: tier.effort || (opts && opts.effort) }
  let why = null
  for (let attempt = 1; attempt <= 2; attempt++) {
    const r = await agentRRetry(prompt + SMALL_SEAT_CONTRACT(seat, tier.model), small, 1)
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
const BUILD_LANES =
  MODE === 'build' && typeof args !== 'undefined' && args && (args.lanes === 1 || args.lanes === '1') ? 1 : 2
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

# ═══ LAND FIRST — MERGE WHAT IS ALREADY GREEN (${CI_MODE === 'local' ? '≤ 14 TOOL CALLS, ONE PR' : '≤ 6 TOOL CALLS'}, THEN MOVE ON) ═══
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
 * in the REVIEW-STAGE barrier); each seat posts its OWN record on the head it read (review-gate.mjs).
 * (b) on a BLOCK, fix-land in FIX mode — rebase, fix or refute every blocking finding, push, and NOT
 * merge — then the reviewer seats are re-dispatched (same lens, fresh context) at the new head. At
 * most MAX_FIX_ROUNDS fix rounds; a further BLOCK parks the PR as needs-human. (c) once every
 * required seat has PASSed the current head, fix-land in LAND mode runs CI_GATE (which runs the
 * review gate) and merges that sha. It NEVER waits for CI.
 * Re-dispatch covers the seat(s) that blocked AND every other required seat: a push voids every
 * record, so a security PR's non-blocking seat must PASS the new head too or the gate refuses it.
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
and gh pr diff -R ${REPO} ${pr}. You may read code in ${dir} (do NOT check out a branch there —
the fix agent owns that worktree next; read-only greps only).
TRIAGE ALREADY ESTABLISHED: ${facts}
${reReview(round, prior)}
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
and gh pr diff -R ${REPO} ${pr}. Read-only greps in ${dir}; never check out a branch there.
TRIAGE ALREADY ESTABLISHED: ${facts}
${reReview(round, prior)}
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
  const blockingOf = (v) => (v && Array.isArray(v.blocking) ? v.blocking : [])
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
    for (const [who, v] of [['review-opus', primary], ['review-opus2', lens]]) {
      if (v && isMalformedBlock(v)) log(`PR#${pr} (${itemId}) round ${round}: ${who} returned BLOCK with an empty blocking[] — no valid record, the PR stays review-pending`)
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
    const reviewedHeads = [primary, lens].map((v) => (v && typeof v === 'object' && v.recordHead) || '')
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
      primaryBlocking: blockingOf(primary),
      lensBlocking: blockingOf(lens),
      dispositions: fix && Array.isArray(fix.blockingDispositions) ? fix.blockingDispositions : fixText.slice(0, 4000),
      newHead: fixHead.after,
    }
    log(`PR#${pr} (${itemId}, ${klass}): fix round ${round} pushed ${prior.newHead} — re-dispatching ${seats === 2 ? 'review-opus + review-opus2' : 'review-opus'} at the new head`)
  }
  const verdicts = { primary, lens, seats, rounds: rounds.length }

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
OPEN BLOCKING FINDINGS: ${JSON.stringify({ primary: blockingOf(primary), lens: blockingOf(lens) })}
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

# 3 — THE GATE, EVERY COMMAND, SEPARATELY, ON THE REBASED HEAD
${GATE_CMDS(dir, target)}
Then git push --force-with-lease, and read the new head: gh pr view -R ${REPO} ${pr} --json headRefOid
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
         blockingDispositions: [{finding, disposition: "fixed"|"refuted", evidence}],
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
  const landHeld = /"status"\s*:\s*"merged"/i.test(landText) ? '' : landBlockedLine(land, [primary, lens].map((v) => (v && typeof v === 'object' && v.recordHead) || ''))
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
// ── 08-yap-uiux-2026-10-16.mjs ────────────────────────────────────────────
// yap-perm-hotkey-2026-10-16 — THE BANNER WAS RIGHT, SETTINGS LIED; THE HOTKEY STARTED A TAKE IT
// COULD NOT STOP. Written 2026-10-16 by the yap-perm-audit seat from Wilson's 0.9.0 report (16:14Z):
// System Settings shows Yap ON in Accessibility while Yap's home says "Accessibility and Input
// Monitoring are off"; and a hotkey take that 15 fn presses would not end. He asked for tools that
// test it for real. FIX, no panel (unambiguous intent). These three items sit at the TOP of
// 08-yap-uiux-2026-10-16.mjs: one file = one lane, run in order, so all three run ahead of
// yap-uiux-2026-10-16-W0 (two adjacent files would land on opposite lanes and run concurrently). Order inside the file: the harness first (it is
// the acceptance tool of yap-hotkey-stop), then perm-truth, then hotkey-stop.
// Review: Opus only (review-opus + review-opus2). No Codex, no Gemini, no third-party reviewer.
// HAZARD (same as 07): no agent launches the GUI app, posts events into, signals or quits Wilson's
// running /Applications/Yap.app, or writes TCC (no tccutil, no sqlite writes). Every automated proof
// is a cargo test, a vitest, or a script self-test. SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.
//
// DIAGNOSIS (observed on Wilson's machine 2026-10-16, read-only):
//   - TCC rows for com.wilsonguenther.wilson-voice: kTCCServiceAccessibility (2026-07-22 20:35Z) and
//     kTCCServiceListenEvent (20:36Z) store csreq = identifier + leaf CN "Apple Development: Wilson
//     Guenther (U8BP8Z86T2)". Yap 0.9.0 is signed "Developer ID Application: Wilson Guenther
//     (VHYV8C2JNU)" (cdhash 5c551c8a...). codesign -v -R <stored csreq> FAILS for 0.9.0 and PASSES for
//     the 0.8.0 backup (Apple Development, cdhash 9f5b8aa7...). The Settings row is keyed by bundle id,
//     so it shows ON; the OS answers false for this binary. The microphone row re-prompted and was
//     re-stamped with the Developer ID requirement at 16:13:46Z; Accessibility/ListenEvent never re-prompt.
//   - yap.log 2026-10-16: 14:03:28Z "Accessibility not trusted"; 16:13:51Z "PTT double-tap ->
//     hands-free ON" + "recording started"; then 7x "tap disabled by kCGEventTapDisabledByUserInput
//     ... re-arm #N issued" (16:13:59Z to 16:15:46Z) and ZERO fn edges logged after 16:13:51Z: no "fn
//     tap ends hands-free". The take ended 16:15:02Z from the window; the next take started 16:15:18Z
//     with no PTT line (window). pasted=false on both (Accessibility false).

ITEMS.push({
  id: 'yap-qa-harness', prompt: 'yap-qa-harness', branch: 'loop/yap-qa-harness-perm-probe-and-hotkey-walk', gated: null,
  title: 'Real permission + hotkey QA tools: scripts/qa/perm-probe.sh (signature, duplicates, LaunchServices, a labelled helper probe) and scripts/qa/hotkey-walk.sh (a real fn take on the installed Yap)',
  preflight: `
    test -x scripts/qa/perm-probe.sh
    test -x scripts/qa/hotkey-walk.sh
    test -f scripts/qa/perm-helper.swift
    grep -q 'helper_ax_trusted=' scripts/qa/perm-probe.sh
  `,
  spec: `
    Panel: not required — FIX (Wilson 2026-10-16: "tools that test it for real")
    DEPENDS: a green main. Blocks: yap-hotkey-stop (its acceptance runs these scripts).

    WHY: the loop QA'd 0.9.0 in headless Chrome against desktop/dist with shimmed permission answers.
    Nothing ever asked macOS what it answers for the SIGNED, INSTALLED binary, and nothing ever pressed
    fn. The signing identity changed (Apple Development -> Developer ID) and every TCC grant pinned to
    the old certificate went dead while System Settings kept showing ON.

    DO
      1. scripts/qa/perm-probe.sh [--app PATH] [--compare PATH] [--self-test]. Default app
         /Applications/Yap.app. READ-ONLY. Prints key=value lines, one per fact:
           app_path, bundle_id (plutil), version, team_id, authority_leaf, cdhash (codesign -dvvv),
           designated_requirement (codesign -d -r-), same_id_bundles (mdfind on kMDItemCFBundleIdentifier
           plus a scan of /Applications and ~/Applications, comma list, the probed app excluded),
           lsregister_count (lsregister -dump, registrations whose identifier is the bundle id, with the
           paths and a stale=1 flag for paths that no longer exist),
           running_pids (pgrep -f on the bundle executable path; read only, never signalled),
           helper_ax_trusted and helper_listen_event (from the helper, item 2; the KEY NAMES carry the
           helper_ prefix and a comment line says "helper values describe the QA helper, NOT Yap").
         --compare NEW.app prints dr_change=yes|no between the installed app and NEW.app, and on yes
         prints "warning: this build changes the designated requirement; every Accessibility, Input
         Monitoring and Full Disk Access grant for the old build stops matching". This is the release
         check that would have caught 0.9.0; docs/RELEASING.md (create if absent) gets one line telling
         the release seat to run it and to put the warning in the release notes when dr_change=yes.
         It never reads or writes TCC.db.
         --self-test runs every code path against /System/Applications/Calculator.app (and --compare
         Calculator vs Calculator, expecting dr_change=no) and exits non-zero on any missing key.
      2. scripts/qa/perm-helper.swift: about 20 lines, prints ax_trusted=<AXIsProcessTrusted()> and
         listen_event=<CGPreflightListenEventAccess()>. perm-probe.sh compiles it with swiftc into
         desktop/target/qa/perm-helper (gitignored target dir), signs it ad hoc (codesign -s -), and
         never uses Yap's identity. The helper is NOT the app and its answers are never presented as Yap's.
      3. scripts/qa/hotkey-walk.sh [--self-test]. Drives the INSTALLED Yap through a real take:
         posts a synthetic fn down (CGEvent flagsChanged with maskSecondaryFn, via a second tiny Swift
         helper scripts/qa/fn-post.swift built the same way), holds 1.5 s, posts fn up; tails
         ~/Library/Application Support/WilsonVoice/logs/yap.log and asserts, in order and within 5 s
         each: "PTT hold armed", "recording started", "PTT hold release", and the take's
         "transcript ok" or "no speech" line; then a second pass: double-tap fn (hands-free ON), wait
         3 s, tap fn once, assert "tap ends hands-free" and that the take ended. Exit codes: 0 pass,
         1 an assertion failed (prints which line never came), 3 the CALLER lacks Accessibility (posting
         CGEvents needs it; print "hotkey-walk needs Accessibility for <terminal/app> — grant it in
         System Settings -> Privacy & Security -> Accessibility, then rerun"), 4 Yap is running and
         YAP_QA_DRIVE_RUNNING is not 1 (print "Yap is running (pid N); this walk posts fn into it —
         rerun with YAP_QA_DRIVE_RUNNING=1 when you are not dictating"). It never launches, quits or
         signals Yap. --self-test exercises the refusal paths and the log-assertion parser against a
         fixture log (scripts/qa/fixtures/yap-walk-pass.log and yap-walk-stuck.log, the stuck one copied
         from the 16:13:51Z excerpt above with the transcript text replaced) and posts no events.
      4. Both scripts: set -u, no pipes in the exit-code path, shellcheck clean.

    WALK SEAT: the loop has no seat named walk; the walks are items (yap-uiux-2026-10-16-W0 and -Z in
    08-yap-uiux). Those specs now tell the walker to run both scripts and attach the output. The full
    hotkey-walk (events into the installed app) is Wilson's walk step, listed in the PR body.
  `,
  acceptance: `
    test -x scripts/qa/perm-probe.sh
    test -x scripts/qa/hotkey-walk.sh
    test -f scripts/qa/perm-helper.swift
    test -f scripts/qa/fn-post.swift
    bash scripts/qa/perm-probe.sh --self-test
    mkdir -p desktop/target && bash scripts/qa/perm-probe.sh --app /System/Applications/Calculator.app > desktop/target/qa-perm-probe.txt
    grep -q '^bundle_id=com.apple.calculator$' desktop/target/qa-perm-probe.txt
    grep -q '^cdhash=' desktop/target/qa-perm-probe.txt
    grep -q '^lsregister_count=' desktop/target/qa-perm-probe.txt
    grep -q '^helper_ax_trusted=' desktop/target/qa-perm-probe.txt
    grep -q '^helper_listen_event=' desktop/target/qa-perm-probe.txt
    bash scripts/qa/perm-probe.sh --app /System/Applications/Calculator.app --compare /System/Applications/Calculator.app > desktop/target/qa-perm-compare.txt
    grep -q '^dr_change=no$' desktop/target/qa-perm-compare.txt
    bash scripts/qa/hotkey-walk.sh --self-test
    shellcheck scripts/qa/perm-probe.sh scripts/qa/hotkey-walk.sh
  `,
})

ITEMS.push({
  id: 'yap-perm-truth', prompt: 'yap-perm-truth', branch: 'loop/yap-perm-truth-banner-names-binary-and-duplicates', gated: null,
  title: 'The permission banner tells the truth about THIS binary: re-checked on activation and every 5 s while shown, names the failing condition and the bundle path, names a duplicate Yap, and explains a stale grant',
  preflight: `
    grep -q 'WatchReason::Activate' desktop/src-tauri/src/lib.rs
    grep -q 'Another copy of Yap at' desktop/src/permission.ts
    test -f desktop/src-tauri/tests/perm_truth.rs
  `,
  spec: `
    Panel: not required — FIX (Wilson 2026-10-16 report; diagnosis at the top of this file)
    DEPENDS: yap-qa-harness (perm-probe.sh is how the PR proves the binary-level facts).

    EVIDENCE (main b2a1e2e)
      - permissions.rs:198 report() reads AXIsProcessTrusted (permissions.rs:126) and IOHIDCheckAccess
        (permissions.rs:384) fresh on every call; nothing is cached. The watcher (permissions.rs:684)
        re-reads on Launch, Focus (lib.rs:5613), Wake and Refusal (lib.rs:4161, lib.rs:5967,
        paste.rs:247) and on a 45 s timer only while something is denied (permissions.rs:542,
        permissions.rs:597). No app-activation trigger; a Settings change while the window is unfocused
        waits up to 45 s.
      - The banner (permission.ts:246) was CORRECT on 0.9.0: the OS answered false for this binary
        because the July grants are pinned to the Apple Development certificate. It just did not say
        why, so Wilson trusted the Settings toggle over it.
      - input_monitoring_state (permissions.rs:425) treats tap_health().re_arms == 0 as "the tap is
        delivering" (permissions.rs:409). A re-arm count is not delivery evidence: on 0.9.0 the tap was
        re-armed 7 times and delivered nothing.

    DO
      1. Re-check on activation: register NSApplicationDidBecomeActiveNotification (one observer, in
         the same place Focus is poked) and poke WatchReason::Activate. While any grant nags, the
         watcher interval is 5 s (WATCH_INTERVAL = 5 s, used ONLY while the banner is shown; parked
         otherwise, as today).
      2. The report carries checkedBundlePath (NSBundle.mainBundle bundlePath of the running process),
         signingAuthority (leaf certificate CN from SecCodeCopySigningInformation on self), and
         failingConditions: a list of "Accessibility false (AXIsProcessTrusted)" / "Input Monitoring
         false (IOHIDCheckAccess listen = denied)". The banner names each failing condition and the path
         it checked, e.g. "Accessibility is off for /Applications/Yap.app (AXIsProcessTrusted = false)".
      3. Stale grant: persist lastTrustedSignature (authority + cdhash + version) whenever
         Accessibility reads true. When it reads false and the stored signature differs from the running
         one, the banner adds: "macOS still lists Yap as allowed, but that permission was given to Yap
         <old version>, signed differently. In System Settings, select Yap, click the minus button, then
         click plus and choose /Applications/Yap.app." (same for Input Monitoring). Pure decision function
         with unit tests; no TCC reads or writes.
      4. Duplicate bundles: at launch, scan /Applications and ~/Applications (one level, plus
         *.app inside folders one level down) for bundles whose CFBundleIdentifier equals ours and whose
         path differs; if any, the banner adds "Another copy of Yap at <path> may hold the permission."
         Pure function over a directory listing, unit-tested with a temp dir.
      5. input_monitoring_state takes delivery evidence = "a key or flags edge was received since the
         last re-arm" (a counter ptt_macos.rs bumps in tap_callback), never re_arms == 0.
      6. Tests: desktop/src-tauri/tests/perm_truth.rs (>= 6 tests: stale-signature message, no message
         when signatures match, duplicate found, duplicate absent, failing-conditions wording, delivery
         evidence after re-arm); desktop/src/permission.truth.test.ts (>= 3 vitest cases on the banner
         text). Keep Tauri, app-sandbox false, never rename the bundle id or the data dir.

    WILSON'S WALK (PR body): bash scripts/qa/perm-probe.sh; open Yap: the banner names the path and,
    until he re-adds Yap in Settings, the stale-grant sentence.
  `,
  acceptance: `
    grep -q 'WatchReason::Activate' desktop/src-tauri/src/lib.rs
    grep -q 'Another copy of Yap at' desktop/src/permission.ts
    grep -q 'checkedBundlePath' desktop/src/permission.ts
    grep -qE 'WATCH_INTERVAL: Duration = Duration::from_secs\\(5\\)' desktop/src-tauri/src/permissions.rs
    test -f desktop/src-tauri/tests/perm_truth.rs
    test -f desktop/src/permission.truth.test.ts
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && npx vitest run src/permission.truth.test.ts)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    (cd desktop/src-tauri && cargo test --features custom-protocol --test perm_truth)
    test "$(cd desktop/src-tauri && cargo test --features custom-protocol --test perm_truth -- --list 2>/dev/null | grep -c ': test$')" -ge 6
    bash scripts/qa/perm-probe.sh --self-test
  `,
})

ITEMS.push({
  id: 'yap-hotkey-stop', prompt: 'yap-hotkey-stop', branch: 'loop/yap-hotkey-stop-fn-ends-every-take', gated: null,
  title: 'A take the hotkey started, the hotkey stops: fn key-up from any app, verified tap re-arm within 100 ms, a HID-state second mechanism, a 90 s still-recording prompt, and pill + menu bar stop',
  preflight: `
    test -f desktop/src-tauri/tests/hotkey_stop.rs
    grep -q 'STILL_RECORDING_AFTER_SECS: u64 = 90' desktop/src-tauri/src/ptt_macos.rs
    grep -q 'CGEventTapIsEnabled' desktop/src-tauri/src/ptt_macos.rs
  `,
  spec: `
    Panel: not required — FIX (Wilson 2026-10-16: 15 fn presses, the take would not stop)
    DEPENDS: yap-qa-harness (scripts/qa/hotkey-walk.sh is this item's acceptance tool; if it is not
      on main when you start, rebase after it lands; never write a second copy).

    EVIDENCE (main b2a1e2e; yap.log 2026-10-16, quoted at the top of this file)
      - START and STOP share ONE mechanism: the listen-only HID CGEventTap (ptt_macos.rs:309-341).
        Hold start = on_combo_down -> hold-arm thread (ptt_macos.rs:456-514); hold stop = on_combo_up
        (ptt_macos.rs:516-534); hands-free (double-tap) start = on_combo_up (ptt_macos.rs:545-556) and
        stop = the next combo down (ptt_macos.rs:466-478). No Carbon/global-shortcut fallback for fn
        (shortcuts.rs:22: there is no Code::Fn); the legacy toggle is off ("secondary disabled").
      - The tap is created whatever the grants say (lib.rs:5961-5981 only logs + pokes); it ran under an
        untrusted 0.9.0 and delivered the 16:13:45-51Z edges, then was disabled by user input 7 times.
      - Re-arm is fire-and-forget: re_arm_tap (ptt_macos.rs:707-716) calls CGEventTapEnable and logs
        "issued" with no check that the tap is enabled or that any edge arrives afterwards.
      - handle_combo_edge returns early when want == was (ptt_macos.rs:436-438): a missed up-edge leaves
        combo_down stuck true and the next press is swallowed; nothing reconciles against
        CGEventSourceFlagsState.
      - No take-duration prompt anywhere (no hold timeout, no "still recording").

    DO
      1. Verified re-arm: after CGEventTapEnable, read CGEventTapIsEnabled within 100 ms and log
         "re-arm verified" or "re-arm FAILED"; after 3 disables in 60 s, invalidate the port and create
         a fresh tap on the same run loop, logged. Count delivered edges since the last re-arm (shared
         with yap-perm-truth DO 5).
      2. Second mechanism for stop: while a take is live (and only then), a 30 ms poll of
         CGEventSourceFlagsState(HID system state) for maskSecondaryFn. A fn up seen by the poll ends a
         hold-mode take; an fn down-up seen by the poll ends a hands-free take. The poll and the tap
         feed the SAME edge function, deduplicated by edge time, so a take is never stopped twice. The
         poll also reconciles a stuck combo_down. Measure and record in the PR whether the HID flags
         read works without Input Monitoring on Wilson-shaped grants (helper values from perm-probe.sh).
      3. Any focused app: the stop path never depends on Yap being frontmost (tap at kCGHIDEventTap,
         poll is process-independent). Unit-test the edge state machine as a pure function: hold
         up-edge, hands-free tap, missed up-edge recovered by the poll, tap disabled mid-take then fn up.
      4. Still recording: STILL_RECORDING_AFTER_SECS: u64 = 90. A hold take with no key-up for 90 s, or
         ANY take during which the tap was disabled, shows on the pill "Still recording — press fn or
         click stop"; one fn tap or the hotkey then ends it.
      5. Crash-safe stop: the menu bar Stop item and the pill's stop control both end the take through
         the same stop function the hotkey uses, and both call end_hands_free (ptt_macos.rs:295) so the
         next fn press is not swallowed. Tests prove both call sites reach it.
      6. desktop/src-tauri/tests/hotkey_stop.rs: >= 8 tests. Keep Tauri, app-sandbox false, never rename
         the bundle id or the data dir.

    WILSON'S WALK (PR body): scripts/rebuild_app.sh; then YAP_QA_DRIVE_RUNNING=1 bash
    scripts/qa/hotkey-walk.sh from a terminal that holds Accessibility; then by hand in Notes and in
    Safari: hold fn, talk, release (take ends); double-tap fn, talk, tap fn (take ends).
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/hotkey_stop.rs
    grep -q 'STILL_RECORDING_AFTER_SECS: u64 = 90' desktop/src-tauri/src/ptt_macos.rs
    grep -q 'CGEventTapIsEnabled' desktop/src-tauri/src/ptt_macos.rs
    grep -q 're-arm verified' desktop/src-tauri/src/ptt_macos.rs
    grep -rq 'Still recording' desktop/src desktop/src-tauri/src
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    (cd desktop/src-tauri && cargo test --features custom-protocol --test hotkey_stop)
    test "$(cd desktop/src-tauri && cargo test --features custom-protocol --test hotkey_stop -- --list 2>/dev/null | grep -c ': test$')" -ge 8
    bash scripts/qa/hotkey-walk.sh --self-test
    bash scripts/qa/perm-probe.sh --self-test
  `,
})

// yap-format-structure — WISPR-STYLE STRUCTURE FROM SPEECH. Written 2026-10-16 by the
// yap-format-item seat. Wilson dictated a long review by voice today; Wispr Flow turned it into an
// intro paragraph, a bulleted list, prose, and a second list of "topic: verdict" bullets. His words:
//   "I just use Wispr Flow for that. You see how it does the formatting? That's what Yap needs to be
//    able to do. It knew I was doing a list of things."
// Yap pastes the same take as flat prose. FIX, no panel (unambiguous intent, Wispr parity).
// ORDER: this block sits in 08-yap-uiux-2026-10-16.mjs after yap-hotkey-stop and BEFORE W0 (one file =
// one lane, run in order), so this item runs ahead of yap-uiux-2026-10-16-W0 — the same placement as
// PR #208's three items. A separate adjacent file would land on the opposite lane and run concurrently.
// WHY A NEW ITEM, NOT AN EXTENSION OF Y4: every Y4 item (10-y4-formatting.mjs) is MERGED or
// ALREADY-DONE and retires at pre-flight. Y4-D proves EXPLICIT-cue lists only ("one, two, three",
// "first/second", the word "bullet": dictation.rs detect_and_format_list ~:1776, test
// list_bullets_only_on_explicit_cue) and guards against over-eager bullets; Y4-C paragraphs at pauses
// (insert_paragraphs ~:2606); Y4-F maps apps to six DictationModes (mode_for_app ~:65). None of them
// INFERS structure from parallel clauses or "topic: verdict" speech, and none emits Markdown per
// target. This item EXTENDS those modules (same pipeline, same fixture discipline) — it never builds
// a parallel formatter.
// Review: Opus only (review-opus + review-opus2), ADVERSARIAL on "did it invent content". No Codex,
// no Gemini, no third-party reviewer. SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.
//
// HOW YAP POLISHES TODAY (main, 2026-10-16, read-only):
//   desktop/src-tauri/src/dictation.rs run_cleanup (~:856): dictionary -> clean_backtrack ->
//     format_dictation (~:123, explicit-cue list detection only, else prose) -> apply_spoken_marks ->
//     format_email_shape (Email) -> insert_paragraphs -> trailing-period rule -> optional LLM polish.
//   desktop/yap-polish/src/main.rs SYSTEM_BASE (~:62) + mode_overlay (~:80): a llama.cpp rewriter
//     that "preserves existing line breaks and list numbering"; it never infers structure, and with
//     the shipped polish_model "" (lib.rs ~:476) it never runs at all. Paste is clipboard + synthetic
//     Cmd-V (paste.rs), so newlines in a paste never SEND a chat message.

ITEMS.push({
  id: 'yap-format-structure', prompt: 'yap-format-structure', branch: 'loop/yap-format-structure-wispr-style-lists-and-paragraphs', gated: null,
  title: 'Wispr-style structure from speech: enumerations become lists, topic-then-verdict becomes "Topic: verdict" bullets, topic shifts become paragraphs, formatted for the paste target',
  preflight: `
    test -f desktop/src-tauri/src/structure.rs
    grep -q 'src-tauri/src/structure.rs' desktop/yap-polish/src/main.rs
    test 8 -le "$(ls -d desktop/yap-polish/fixtures/structure/*/ | wc -l)"
    grep -q 'fn structure_fixture_01_two_lists_and_topic_verdict_bullets' desktop/src-tauri/src/structure.rs
  `,
  spec: `
    Panel: not required — FIX (Wilson 2026-10-16: "That's what Yap needs to be able to do. It knew I
    was doing a list of things.")
    DEPENDS: a green main; Y4-C, Y4-D, Y4-F (all on main). Runs ahead of yap-uiux-2026-10-16-W0.

    GOAL (Wilson): dictate a review the way he talks and get what Wispr Flow gives him — the lists
    are lists, the "this thing: verdict" lines are bullets, the topic shifts are paragraphs — pasted
    in the form the app he is in actually renders.

    WHERE IT LIVES (one module, two crates)
      - desktop/src-tauri/src/structure.rs: a PURE, rules-only, deterministic module. No LLM, no
        clock in the output path, no randomness, no HashMap iteration order in the output. It runs in
        run_cleanup right after format_dictation and before insert_paragraphs (insert_paragraphs must
        never break inside a list it produced), at the shipped level (Medium) and with NO polish model
        installed. Y4-D stays law: list detection is never LLM-dependent.
      - desktop/yap-polish/src/main.rs includes it with
        #[path = "../../src-tauri/src/structure.rs"] mod structure;  — the exact pattern already used
        for polish_protocol.rs — so the module is compiled and tested in BOTH crates and the sidecar
        can re-apply structure if the LLM flattens it. Add a SYSTEM_BASE rule "Preserve every bullet
        and numbered line exactly" (system_prompt_is_stable_per_mode_and_style must stay green). The
        module therefore depends only on std (no serde, no crate-local paths).
      - Tests live INSIDE structure.rs (#[cfg(test)] mod tests, every fn named structure_*) because a
        bin-only crate has no integration-test surface. Fixtures are read from
        concat!(env!("CARGO_MANIFEST_DIR"), "/../yap-polish/fixtures/structure") — a path that
        resolves from both crates because they are siblings under desktop/.

    RULES
      1. Enumerations become bulleted lists. Cues: "one, two, three"; "first / second / then / also /
         and then"; three or more parallel clauses or noun phrases in one run ("the security, the data
         pulling, the telemetry"); repeated sentence openers ("make sure X. make sure Y."); a topic
         followed by a colon-like pause (a PauseSpan, or an ASR comma/period) then a verdict.
      2. Numbered list when order matters ("first ... then ... finally", "step one").
      3. Paragraph breaks at topic shifts and after a verdict sentence that closes a list; reuse
         insert_paragraphs for pause-based breaks, do not duplicate it.
      4. "Topic: comment" bullets when he names a thing and then judges it:
         "Account and licence, perfect." -> "- Account and licence: perfect."
      5. Headings ONLY when he says "heading" or "title" explicitly; never invented.
      6. Target-aware output: pub fn structure_format_for(bundle_id: &str, setting: &str) ->
         StructureFormat { Markdown, PlainBullets, Off }.
           Markdown ("- ", "1. ", "## "): editors, notes, Slack, Notion-like — com.tinyspeck.slackmacgap,
             notion.id, md.obsidian, com.apple.Notes, com.microsoft.Word; a browser only when the user
             setting says so (browser = ambiguous, the Y4-F conservative rule).
           PlainBullets ("• " or "- " with real newlines, never "#" or "**"): com.apple.mail,
             com.apple.MobileSMS, com.microsoft.Outlook, and every unknown text field (the
             conservative default).
           Off: code editors and terminals (com.microsoft.VSCode, com.apple.Terminal,
             com.googlecode.iterm2, dev.warp.Warp-Stable, Cursor, com.apple.dt.Xcode) unless a mode is
             set; DictationMode::Code is always Off.
         Chat: Y4-C/Y4-F say chat gets no paragraph breaks because a typed Return sends. Paste is
         clipboard + Cmd-V (paste.rs), which does not send, so a list in Slack pastes as a list. Keep
         the no-blank-line rule for chat, allow single-newline list items, and pin it with a test.
      7. Never change his words beyond what polish already does: no new content, no summaries, no
         reordering, no dropped words except the cue words consumed as markers ("first", "second",
         "next", "number one", "bullet", "heading", "title"), kept in ONE named const list.
      8. Settings: a global "Structure" switch (default ON) and a per-app override list in
         desktop/src/views/settings/Dictation.tsx, persisted in the settings struct in lib.rs
         (structure_enabled: bool, structure_app_overrides keyed by bundle id), read in run_cleanup.
         Plain words on the control ("Turn spoken lists into bullets"), no engineer words (Y4-H).
      9. Latency: structure inference adds <= 150 ms p95 on a 400-word take on Apple Silicon (the lane
         machine is Wilson's M-series Mac). Measure 50 runs over the 400-word fixture, print p95.
     10. Deterministic: same transcript (+ pauses + target) -> byte-identical output.

    FIXTURES  desktop/yap-polish/fixtures/structure/NN-name/{input.txt, target.txt, expected.md,
      pauses.json (optional)} — at least 8 folders. target.txt holds a bundle id. Required:
      01-wilson-review-2026-10-16 (below); 02 numbered steps "first ... then ... finally";
      03 "make sure X / make sure Y" parallel openers; 04 topic-then-verdict alone; 05 explicit
      "heading"; 06 a 400-word take (the latency fixture); and two NEGATIVE controls that must come out
      as unchanged prose: 07 a narrative "and then we drove, and then we ate" (a story, not a list),
      08 fixture 03's text with target com.microsoft.VSCode (Off). A single-item "list" must not
      become a list (Y4-D single_item rule). Invented, bland text for 02-08; never real dictation.

      FIXTURE 01 \u{2014} Wilson's review, the FULL Wispr Flow output from 2026-10-16 (ground truth, saved at
      Wilson-Brain/Projects/Loop-Logs/yap-fixture-01-wispr-2026-10-16.md). expected.md is exactly the text
      below, byte-for-byte after stripping the 8-space indent of this spec. input.txt is the SAME words as
      flat speech: no list markers, no line breaks, no colon after a topic (use a comma or a period as ASR
      would), so the structure stage must recreate the lists, the "Topic: verdict" bullets and the paragraphs.
      target.txt: notion.id
      expected.md:
        Okay, I really, really liked the slideshow. It was very good, very great, very intuitive, and easily understood. I love everything.
        One thing I'm mainly concerned about is to make sure all of the components are properly structured and straight, and to polish every one of them. Make sure there's no spillage or bubbles that are not properly confined and contained. Every page is good. We think about scrolling, pads, error states, handling, and stuff like that.
        Also, one of my biggest concerns is external devices. Make sure it's properly built:
        - the security
        - the data pulling
        - the telemetry
        - the health
        - constant-time health checks to make sure devices are connected and healthy and not trying to put viruses or inject something into the programme, etc.
        Make sure the whole app actually has proper security handling. Please think about the user experience.
        Is AP okay? The MCP's security, telemetry, deep metadata, and editing capabilities for all AI agents and stuff like that: I don't know if there should be an API system. I don't know. That's up to you.
        Everything is good:
        - First run permissions: make sure that's really verifiable and it's always good.
        - Mic test, all of those.
        - Make sure all of the devices, all the external devices, are properly health-checked, working smoothly, with alerts for the user, data reporting, and all of that.
        - Make sure people can send reports back to me, back to Drivia.
        - Account and licence: perfect. Make sure, just like we did it for Yap with the Stripe and all of that, people can go on it on any device, any account. All they have to do is just copy and paste that licence, so if they have it saved, type it out. I think that's what you mean, but all of that to make sure it's all smooth.
        - Confirmation number, all of that.
        - Guided tour: it looks good. I love all of the tour. It looks very nice and polished.
        - Check the components and make sure everything is smooth.
        - Remember screen record and UI screenshot: you can make that.
        - Library: make sure it looks smooth.
        - Storage: make sure that's properly built out.
        - User experience: users can see a timed live health check update, so they can see lazy loading in the background, skeletons, all of that.
        - Programme: all of that.
        - Prior to server-side SQL Lite: make sure all of that schema is properly built out. Think about every role of security and data isolation, and make sure nobody can hack them, but still fast.
        - New session: make sure that's smooth. All the health check and proper stuff are handled before they get into it. That's good.
        - Solo with gas consent shows the status here. Perfect.
        - Crash report data is properly this telemetry and metadata, so all of that can be used to upgrade for AI agents like Codex or Claude, mostly Claude, because Claude is better.
        - Record pre-roll perfectly. Make sure it's thought out.
        - Record live, perfect.
        - Live check: always okay.
        - Metadata: deep, extensive telemetry, metadata on this, on all levels, from visual, audio, transcript, all of that, because all of that needs to be used for building and everything.
        - Timer: I love that.
        - Guess web page: perfect.
        - Time dial: make sure there's a dark component too, plus stop.
        - Okay, looks good.
        - Hold after stop.
        - Transcribe each check.
        - Make sure to constantly know what's going on and where the system is at. That's perfect.
        - Editor: ooh, perfect. Looks good.
        - Talk: make sure everything is tight, smooth, glossy, polished, and make sure the programme overlays Wilson. Okay, all of that.
        - Struck Ember AI suggestion.
        - Cut word.
        - Except make sure all those tool suites are properly defining whatever coding language that needs to be.
        - Everything looks smooth here. Good job.
        - This looks very intuitive.
        - The clips: perfectly.
        - The long, split, fix, landscape: you got to check the hard.
        - I'll have to connect the hard drive so you can see how we did.
        - They did those videos because all of the script is pretty much what I want those clips and the editing for those videos to be.
        - Top 5 short categories and genre: you can check the session for the social media reel.
        - Honestly, that'd be better.
        - What we did: audio tools, perfect.
        - Improve. Make sure they're good.
        - Programme is good. It's properly structured.
        - People: make sure you think that's well thought out.
        - Security and deletion are good.
        - Not fucking up the computer.
        - Everything is a health check.

    TESTS (all in structure.rs, all named structure_*)
      structure_fixture_01_two_lists_and_topic_verdict_bullets — at least 2 lists, the "Topic: verdict"
        lines present
      structure_golden_fixtures_match — every folder, input + target -> expected.md, byte-equal
      structure_is_deterministic_three_runs — every fixture 3x, outputs identical
      structure_latency_p95_under_150ms — prints "structure p95_ms=<n>", fails above 150
      structure_never_invents_content_words — for every fixture: the lowercase alphanumeric words of
        the output minus list markers are a sub-multiset of the input words, AND every non-cue input
        word appears in the output (nothing dropped). The review's adversarial check; also run it over
        seeded (fixed seed) shuffles of the fixture sentences.
      structure_format_for_bundle_id — the rule-6 table, incl. unknown -> PlainBullets, code -> Off
      structure_chat_paste_has_no_blank_lines
      structure_single_item_is_not_a_list, structure_narrative_and_then_is_not_a_list
      Settings: extend tests/formatting_settings_persist.rs with structure_settings_persist (global
      switch + one per-app override round-trip). UI: a vitest for Dictation.tsx asserting the switch
      renders and calls the setter if the repo has a view test harness; otherwise one line in
      docs/loop/MANUAL-WALK.md (create if absent): "Settings > Dictation > Turn spoken lists into
      bullets: off -> dictate fixture 02 into Notes -> flat prose; on -> numbered list".

    WHAT NOT TO DO
      - Do NOT route structure through the LLM or make it depend on polish_model.
      - Do NOT relax any existing formatting_fixtures / app_aware_formatting / paragraphing expectation
        to make room; a conflict is a finding to report, not a row to edit.
      - Do NOT add words, summaries, or headings he did not say.
      - Do NOT launch the GUI app or touch Wilson's installed /Applications/Yap.app.
  `,
  acceptance: `
    test 8 -le "$(ls -d desktop/yap-polish/fixtures/structure/*/ | wc -l)"
    grep -q 'Account and licence' desktop/yap-polish/fixtures/structure/01-wilson-review-2026-10-16/input.txt
    grep -q 'Account and licence: perfect' desktop/yap-polish/fixtures/structure/01-wilson-review-2026-10-16/expected.md
    grep -q 'src-tauri/src/structure.rs' desktop/yap-polish/src/main.rs
    for t in structure_fixture_01_two_lists_and_topic_verdict_bullets structure_golden_fixtures_match structure_is_deterministic_three_runs structure_latency_p95_under_150ms structure_never_invents_content_words structure_format_for_bundle_id structure_single_item_is_not_a_list structure_narrative_and_then_is_not_a_list; do grep -q "fn $t" desktop/src-tauri/src/structure.rs || exit 1; done
    grep -q 'structure_settings_persist' desktop/src-tauri/tests/formatting_settings_persist.rs
    grep -qi 'structure' desktop/src/views/settings/Dictation.tsx
    cd ${APP} && npm ci
    test 8 -le "$(cargo test -p yap-polish --release structure_ -- --list 2>/dev/null | grep -c ': test$')"
    cargo test -p yap-polish --release structure_ -- --nocapture ; test $? -eq 0
    cd src-tauri
    cargo test --features custom-protocol structure_ ; test $? -eq 0
    cargo test --features custom-protocol --test formatting_fixtures ; test $? -eq 0
    cargo test --features custom-protocol --test app_aware_formatting ; test $? -eq 0
    cargo test --features custom-protocol --test formatting_settings_persist ; test $? -eq 0
  `,
})

// yap-meetings-fix — MEETINGS WORKS END TO END. Written 2026-10-16 by the yap-notetaker seat (Opus) from
// Wilson's 20:30Z report: "the meeting thing is not working for shit. I don't know if you ever fix it …
// we need to reverse engineer the Notetaker because ours is not working whatsoever." FIX, no panel
// (unambiguous intent). ORDER: after yap-format-structure and BEFORE yap-uiux-2026-10-16-W0 (one file =
// one lane, run in order), so W0 walks a Meetings view that can actually produce a transcript.
// Parity doc: ~/Obsidian/Wilson-Brain/Projects/Yap/wispr-notetaker-parity-2026-10-16.md.
// Review: Opus only (review-opus + review-opus2). No Codex, no Gemini, no third-party reviewer.
// HAZARD (same as 07/08): no agent launches the GUI app, posts events into, signals or quits Wilson's
// running /Applications/Yap.app, or writes TCC. scripts/qa/meeting-walk.sh drives the INSTALLED app
// only with YAP_QA_DRIVE_RUNNING=1 (the walk seat's consent), exactly like hotkey-walk.sh.
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.
//
// DIAGNOSIS (observed on Wilson's Mac 2026-10-16 ~20:35Z, read-only; main d6cd36a):
//   1. NO TRANSCRIPT, EVER. wilson_voice.db has 4 meetings (2026-08-16, 08-20, 10-10, 10-16 20:05Z), ALL
//      state=transcribing, processed_through_seconds=0.0. meeting_control.rs ~:656 sets Transcribing with the
//      comment "YV93's transcription pipeline moves it on to complete" — but Database::append_meeting_segments
//      and set_meeting_progress have NO caller in src/ (only tests/). yap.log: "meeting 8f9b3c78… stopped after
//      00:00:10 (user stopped) → transcribing" and nothing after. The wiring is yap24-NT2, gated: 'panel' —
//      never built. Audio is NOT the blocker: parakeet-unified-en-0.6b-Q8_0.gguf, qwen2.5-1.5b (polishModel)
//      and wespeaker CAM++ are all installed in ~/Library/Application Support/WilsonVoice/models.
//   2. SILENCE = DENIAL, AND IT LOCKS. TCC kTCCServiceAudioCapture auth=2 at 20:04:52Z with the 0.9.0
//      Developer ID csreq (valid). The meeting at 20:05:04Z ran 10.5 s; permission_verdict (syscapture.rs
//      ~:2334) says LooksDenied after DENIAL_GRACE = 3 s (~:2243) with no non-zero sample — what a silent Mac
//      output produces. meeting_control.rs ~:1094 wrote settings_kv meeting_system_audio_setup_ack_v1 =
//      "looks_denied 20:05:15Z"; track_b_plan (~:2471) now returns MicOnly for every future meeting, so the
//      tap never reopens and the verdict can never heal (only Settings' run_system_audio_setup rewrites it).
//      The row's error tells Wilson "macOS has not granted System Audio Recording" — false. sys_wav_path NULL.
//      This is yap24-NT3, gated: 'panel'.
//   3. NOT SCREEN RECORDING. The capture is a CoreAudio process tap (syscapture.rs header): the grant is
//      "System Audio Recording" (kTCCServiceAudioCapture), not Screen Recording. Do not add an SCK path.
//   4. Side note for perm-truth: kTCCServiceListenEvent's csreq is still "Apple Development … U8BP8Z86T2"
//      (dead for 0.9.0); Accessibility/FDA/Mic/AudioCapture are re-stamped to Developer ID.

ITEMS.push({
  id: 'yap-meetings-fix', prompt: 'yap-meetings-fix', branch: 'loop/yap-meetings-fix-transcript-and-system-audio-end-to-end', gated: null,
  title: 'Meetings works end to end: Stop produces a transcript and a summary, a silent call is never called a denial, and the walk proves it on the installed build',
  preflight: `
    test -x scripts/qa/meeting-walk.sh
    test -f desktop/src-tauri/tests/meeting_pipeline_wired.rs
    test -f desktop/src-tauri/tests/system_audio_verdict_silence_is_not_denial.rs
    grep -q "output_was_running" desktop/src-tauri/src/syscapture.rs
    grep -rln "append_meeting_segments" desktop/src-tauri/src | grep -v "db.rs" | grep -q .
  `,
  spec: `
    Panel: not required — FIX (Wilson 2026-10-16 20:30Z: "the meeting thing is not working for shit … ours
    is not working whatsoever." His observation is ground truth; the DB proves it: 4 of 4 meetings stuck.)
    DEPENDS: a green main; yap-perm-truth (#211, merged), yap-qa-harness (#210, merged). Runs ahead of
    yap-uiux-2026-10-16-W0. ABSORBS the core of the panel-gated yap24-NT1, NT2 and NT3: read their specs in
    scripts/loop/items/01-yap24-notetaker.mjs (~:107-425) as the detailed design, and make their PREFLIGHT
    markers true so they retire when the panel ungates them. Do NOT pull in NT0's eval corpus gate: the WER
    run (meeting_eval) is the thing that hit the 120 s cap earlier today and stays in NT0/NT2.

    ROOT CAUSE FIRST (from the diagnosis block above — fix these before any UI):
      A. Wire stop → transcribe. When MeetingController::stop lands a whole capture (state Transcribing), hand
         the meeting id to ONE background job that runs the shipped MeetingAsr path (meeting_asr.rs
         plan_windows / assemble / merge_two_tracks_by_host_time, asr_engine::transcribe_timed) over t0 (and t1
         when present), calls db.append_meeting_segments per window with on_track, advances
         set_meeting_progress monotonically, and sets Complete (or Failed with a plain-words error). One job
         at a time; dictation keeps priority on the shared engine (the job yields between windows). Remove any
         allow(dead_code) from meeting_asr.rs that this makes live.
      B. Reconcile stranded rows on launch (reconcile_stranded_meetings): a row in transcribing/summarizing
         whose WAV still exists is re-queued from processed_through_seconds; one whose WAV is gone becomes
         Failed "the audio for this meeting was removed before it was transcribed". Retention never purges a
         WAV whose meeting is not Complete (NT1). Wilson's 4 stuck rows are the real-world case: 2 have WAVs.
      C. Silence is not a denial (NT3). The verdict reads "did the OUTPUT device run during the meeting"
         (kAudioDevicePropertyDeviceIsRunningSomewhere on the default output, sampled by the pump; field
         output_was_running). Zero samples + output never ran = Unknown (write nothing). Zero samples +
         output ran >= 60 s across the whole meeting, in TWO consecutive meetings = LooksDenied (NT3 guards,
         01-yap24-notetaker.mjs ~:376-378: a muted call or an output-device switch runs the output with a silent
         tap; one meeting or DENIAL_GRACE alone is never a denial). And the verdict is never sticky: a Virtual/Unknown meeting
         ALWAYS attempts the tap (a fresh tap costs ~200 ms) and lets this meeting's evidence overwrite the
         row; LooksDenied only changes the BADGE, never skips the attempt. In-person meetings never attach.
      D. After Complete, if a local summary model is installed (settings polishModel, today
         qwen2.5-1.5b-instruct-q4_k_m), run summarize_meeting automatically; "no model" leaves the transcript
         and shows "Add a summary model in Settings" — a state, not an error.

    LIVE STATE (the user can see it is working): the pill shows recording + elapsed + two level meters
    (You / Call audio) while recording; after Stop it shows "Transcribing 42%" then "Notes ready". The
    Meetings view row shows the same state and progress (meeting:progress event), and a Failed row shows the
    error and Retry. Keep the locked pixel-art pill and mascot; no new visual language.

    PERMISSION TRUTH: extend the perm-truth banner (permission.ts / PermissionHealthRow.tsx) with System Audio
    Recording, with the SAME truth rules as #211 (re-checked on activation and every 5 s while shown; names
    the failing condition and the bundle path). Source of truth = this binary's own probe (a short tap open in
    run_system_audio_setup with output running), never the settings_kv row alone. Copy names the real
    System Settings path: Privacy & Security › Screen & System Audio Recording › System Audio Recording Only.

    scripts/qa/meeting-walk.sh (new; same shape and refusal discipline as hotkey-walk.sh):
      - Refuses unless Yap is running (exit 6) and YAP_QA_DRIVE_RUNNING=1 (exit 4); caller needs
        Accessibility to post the shortcut (exit 3); usage / app / DB / log missing (exit 2).
      - Makes its sample at run time with say -o "$TMP/sample.aiff" -f scripts/qa/fixtures/meeting-script.txt
        (a ~120-word two-voice script, committed as text; no audio binary in git), plays it in a loop with
        afplay for 30 s, and starts/stops the meeting with ⌃⌘M posted through a QA Swift helper built ad hoc
        (pattern: fn-post.swift) — the documented keyboard path; no new socket.
      - Reads the DB READ-ONLY (sqlite3 -readonly) and Yap's log: the newest meeting reaches complete within
        90 s of Stop; segments on track 1 (call audio) total > 40 words; summary IS NOT NULL when a summary
        model is installed (else prints "summary skipped: no model" and still requires the transcript).
      - If the meeting's own verdict is LooksDenied while afplay was running (true denial), exit 5 with
        "System Audio Recording is not granted for /Applications/Yap.app (designated requirement …) — allow it
        in System Settings › Privacy & Security › Screen & System Audio Recording, then rerun". Never a pass.
      - Bare exits: every "exit N" is the script's own; no pipeline masks a status. Exit 0 pass, 1 assertion.
      - --self-test: refusal paths + the DB/log parsers against fixtures in scripts/qa/fixtures/meeting/;
        posts no events, plays no audio.
      - Cleanup: the walk deletes the meeting it created (through the app's own delete path) at the end —
        no fixture residue.

    RUNTIME DEPENDENCIES (README rule: ship it or manage its install — nothing found on the machine):
      | parakeet-unified-en-0.6b-Q8_0 | downloaded by the app (ModelSetup), sha256-verified | exists |
      | qwen2.5-1.5b-instruct (summary) | downloaded by the app on opt-in | exists |
      | CoreAudio process tap | OS (macOS 14.4+), gated by os_version_gate | exists |
      | say / afplay | macOS built-ins, QA only, never in the app | QA only |

    TESTS: tests/meeting_pipeline_wired.rs (real SQLite file, a 6 s two-track WAV fixture, a stub decoder
    behind the existing seam so it finishes in < 20 s: stop → segments on both tracks → Complete; a second
    run resumes from processed_through); tests/system_audio_verdict_silence_is_not_denial.rs (silent output =
    Unknown, output ran + zeros for < 60 s or in one meeting only = Unknown, output ran >= 60 s + zeros in two
    consecutive meetings = LooksDenied, LooksDenied row still attempts the tap next meeting, in-person
    never attaches); tests/meeting_retention_keeps_untranscribed_audio.rs (NT1); a vitest for the banner row.

    WHAT NOT TO DO
      - Do NOT add ScreenCaptureKit or ask for Screen Recording.
      - Do NOT mark a meeting Complete without segments, and do NOT write a verdict from a silent output.
      - Do NOT drive /Applications/Yap.app from a builder or gate; the walk seat runs meeting-walk.sh.
      - Do NOT run meeting_eval here (the 120 s cap); it belongs to NT0/NT2.

    Critiqued once (self-pass): the first draft only re-ran setup on launch, which still leaves a silent
    first meeting locking every later one — replaced by "always attempt, badge only" (C). It also asserted on
    track 0 words, which a mic in a quiet room can fail; the walk now asserts on track 1, the audio it plays.
  `,
  acceptance: `
    test -x scripts/qa/meeting-walk.sh
    test -f scripts/qa/fixtures/meeting-script.txt
    bash scripts/qa/meeting-walk.sh --self-test ; test $? -eq 0
    test -f desktop/src-tauri/tests/meeting_pipeline_wired.rs
    test -f desktop/src-tauri/tests/system_audio_verdict_silence_is_not_denial.rs
    test -f desktop/src-tauri/tests/meeting_retention_keeps_untranscribed_audio.rs
    grep -q "output_was_running" desktop/src-tauri/src/syscapture.rs
    grep -q "fn reconcile_stranded_meetings" desktop/src-tauri/src/meetings.rs desktop/src-tauri/src/lib.rs desktop/src-tauri/src/meeting_control.rs
    grep -rln "append_meeting_segments" desktop/src-tauri/src | grep -v "db.rs" | grep -q .
    ! grep -n "allow(dead_code)" desktop/src-tauri/src/meeting_asr.rs
    grep -rl "System Audio Recording" desktop/src --include=*.ts --include=*.tsx | grep -q .
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_pipeline_wired ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test system_audio_verdict_silence_is_not_denial ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_retention_keeps_untranscribed_audio ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_manual_start_stop ; test $? -eq 0
  `,
})

// yap-uiux-2026-10-16 — THE UI/UX PASS. Written 2026-10-16 by the yap-prep seat.
// Wilson, 2026-10-06: "All of the Yap stuff: the UI/UX ... There are many items you'll find and
// improve on." Memory: feedback_ui_containment (every panel and shell ships contained),
// feedback_builder_e2e_product_walk ("working" = the full flow as the persona).
// Shape: W0 walks main's build as two personas and writes the ledger; one child per screen fixes
// that screen's rows; Z re-walks after the flush and IS the acceptance of the pass.
// Locked: the pixel-art pill stays (no vector mascot, no origami); NO NEW FEATURES; controls
// first, prose last; contained panels (surface + border, pinned action footer, no infinite scroll,
// no page-level horizontal scroll). Headless only: scripts/smoke-windowed.sh drives headless
// Chrome (--headless=new) against desktop/dist; it never launches the app and never opens a
// visible window. Lane port: export YAP_SMOKE_PORT to your lane's vite port (A 5273, B 5274).
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap-uiux-2026-10-16-W0', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-w0-first-run-and-daily-walk', gated: null,
  title: 'Walk the main build as a first-run user and a daily user, headless, and write down every screen that reads generic, uncontained, confusing or slow',
  preflight: `
    test -f docs/loop/UIUX-WALK-2026-10-16.md
    test -f scripts/smoke-fixtures-first-run.json
    test -f scripts/smoke-fixtures-daily.json
    grep -q 'YAP_SMOKE_FIXTURES' scripts/smoke-windowed.mjs
    grep -q 'YAP_SMOKE_VIEWS' scripts/smoke-windowed.mjs
  `,
  spec: `
    Panel: not required — a walk that writes a ledger and changes no view
    DEPENDS: yap-flakes-2026-10-16 (a green main); Y0-E's smoke (scripts/smoke-windowed.sh, on main)
      QA TOOLS (yap-qa-harness, earlier in this same file, so the same lane, already merged): also run bash scripts/qa/perm-probe.sh and
      bash scripts/qa/hotkey-walk.sh --self-test and paste both outputs into the ledger header and the PR
      body. The full hotkey-walk (it posts fn into the installed Yap) is Wilson's walk step, never an agent's.

    EVIDENCE
      - scripts/smoke-windowed.sh + scripts/smoke-windowed.mjs walk the seven Nav views at 980x700
        and 720x520 in headless Chrome against desktop/dist with a shimmed __TAURI_INTERNALS__,
        and read every answer from ONE file, scripts/smoke-fixtures.json (smoke-windowed.mjs:74).
        One fixture set cannot show both the empty first run and a lived-in day.
      - The 2026-09-15 builders logged "NOT visually QA'd" on Y2-B, Y5-B, Y5-C and Y5-D.
      - Screens: desktop/src/views/Home.tsx (+ src/home/), Dictionary.tsx, Insights.tsx,
        Meetings.tsx (+ src/meetings/), Permissions.tsx, Scratchpad.tsx, Settings.tsx +
        views/settings/{Advanced,Audio,Companion,Dictation,License,Privacy,Shortcut,Snippets}.tsx,
        src/Onboarding.tsx, src/ModelSetup.tsx, src/PermissionHealthRow.tsx.

    DO
      1. smoke-windowed.mjs reads YAP_SMOKE_FIXTURES (a path; default scripts/smoke-fixtures.json,
         so behaviour is unchanged when it is unset) and YAP_SMOKE_VIEWS (a comma list of Nav
         labels, case-insensitive; unset = all views). It also captures the first-run overlay
         (Onboarding / ModelSetup) as the pseudo-view "Onboarding" when the fixture set shows it.
      2. Two fixture sets, only shapes already certain (the _README rule in smoke-fixtures.json):
         scripts/smoke-fixtures-first-run.json (no history, no meetings, permissions not granted,
         no model installed, trial day 14) and scripts/smoke-fixtures-daily.json (40 dictations,
         3 meetings with one still transcribing, every permission granted, trial day 5). Every
         string is invented and bland: never real dictation text (RESUME §11.10).
      3. Walk both personas with YAP_SMOKE_SHOTS pointing at
         ~/Obsidian/Wilson-Brain/Projects/Loop-Logs/yap-uiux-2026-10-16/first-run/ and .../daily/.
         PNGs stay out of git.
      4. Write docs/loop/UIUX-WALK-2026-10-16.md: a header, then one table row per finding, each
         row starting with "| <Screen> |" where Screen is one of Home, Dictionary, Insights,
         Meetings, Scratchpad, Settings, Onboarding:
         | Screen | view file | persona | verdict | evidence (PNG path + what it shows) | fix | child id | status |
         verdict is one or more of generic / uncontained / confusing / slow; fix is named and
         scoped to the view file; status is open, fixed or none (a screen with no finding gets
         one "none" row). Judge by: controls first, prose last; every panel contained; empty
         states say what to do next; nothing spins past 10 s; the pill is out of scope here (it
         stays pixel-art; yap24-PILL owns its states). A finding that needs a new capability goes
         to an "Out of scope (new feature)" list at the bottom, never into a row.
      5. This item changes no view. It ships the env vars, the fixture sets, the ledger and the
         baseline structural findings for both personas.
  `,
  acceptance: `
    test -f docs/loop/UIUX-WALK-2026-10-16.md
    test -f scripts/smoke-fixtures-first-run.json
    test -f scripts/smoke-fixtures-daily.json
    node -e "for (const f of ['scripts/smoke-fixtures-first-run.json','scripts/smoke-fixtures-daily.json']) JSON.parse(require('fs').readFileSync(f,'utf8'))"
    grep -q 'YAP_SMOKE_FIXTURES' scripts/smoke-windowed.mjs
    grep -q 'YAP_SMOKE_VIEWS' scripts/smoke-windowed.mjs
    for s in Home Dictionary Insights Meetings Scratchpad Settings Onboarding; do grep -q "^| $s |" docs/loop/UIUX-WALK-2026-10-16.md || exit 1; done
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
  `,
})

ITEMS.push({
  id: 'yap-uiux-2026-10-16-HOME', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-home', gated: null,
  title: 'Home reads contained and obvious for both personas: every walk row on Home fixed',
  preflight: `
    grep -q '^| Home |' docs/loop/UIUX-WALK-2026-10-16.md
    ! grep '^| Home |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
  `,
  spec: `
    Panel: not required — fixes the W0 ledger names, no new features
    DEPENDS: yap-uiux-2026-10-16-W0
    FILES: desktop/src/views/Home.tsx, desktop/src/home/ (YappyHouse stays pixel-art)
    DO: read every "| Home |" row in docs/loop/UIUX-WALK-2026-10-16.md and fix each in the named
      file only. Controls first, prose last; contained surfaces; the empty state says what to do
      next. Never show a real transcript list in any capture. Re-walk Home for both fixture sets,
      put the after PNGs in the Obsidian walk dir under after/home/, attach one before/after pair
      to the PR, and flip the rows to fixed with the PR number.
  `,
  acceptance: `
    ! grep '^| Home |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Home YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-first-run.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
    YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Home YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-daily.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
  `,
})

ITEMS.push({
  id: 'yap-uiux-2026-10-16-DICT', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-dictionary', gated: null,
  title: 'Dictionary reads contained and obvious for both personas: every walk row on Dictionary fixed',
  preflight: `
    grep -q '^| Dictionary |' docs/loop/UIUX-WALK-2026-10-16.md
    ! grep '^| Dictionary |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
  `,
  spec: `
    Panel: not required — fixes the W0 ledger names, no new features
    DEPENDS: yap-uiux-2026-10-16-W0
    FILES: desktop/src/views/Dictionary.tsx
    DO: read every "| Dictionary |" row in docs/loop/UIUX-WALK-2026-10-16.md and fix each in the
      named file only. Controls first (add word, search), prose last; a contained list with a
      pinned action footer, no infinite scroll. Re-walk Dictionary for both fixture sets, after
      PNGs to the Obsidian walk dir under after/dictionary/, one before/after pair on the PR, rows
      flipped to fixed with the PR number.
  `,
  acceptance: `
    ! grep '^| Dictionary |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Dictionary YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-first-run.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
    YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Dictionary YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-daily.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
  `,
})

ITEMS.push({
  id: 'yap-uiux-2026-10-16-INSIGHTS', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-insights', gated: null,
  title: 'Insights reads contained and obvious for both personas: every walk row on Insights fixed',
  preflight: `
    grep -q '^| Insights |' docs/loop/UIUX-WALK-2026-10-16.md
    ! grep '^| Insights |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
  `,
  spec: `
    Panel: not required — fixes the W0 ledger names, no new features
    DEPENDS: yap-uiux-2026-10-16-W0
    FILES: desktop/src/views/Insights.tsx
    DO: read every "| Insights |" row in docs/loop/UIUX-WALK-2026-10-16.md and fix each in the
      named file only. The first-run persona has no data: the empty state says what produces
      insights instead of a zero-filled chart. Re-walk Insights for both fixture sets, after PNGs
      to the Obsidian walk dir under after/insights/, one before/after pair on the PR, rows flipped
      to fixed with the PR number.
  `,
  acceptance: `
    ! grep '^| Insights |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Insights YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-first-run.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
    YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Insights YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-daily.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
  `,
})

ITEMS.push({
  id: 'yap-uiux-2026-10-16-MEETINGS', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-meetings', gated: null,
  title: 'Meetings reads contained and obvious for both personas: every walk row on Meetings fixed',
  preflight: `
    grep -q '^| Meetings |' docs/loop/UIUX-WALK-2026-10-16.md
    ! grep '^| Meetings |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
  `,
  spec: `
    Panel: not required — fixes the W0 ledger names, no new features
    DEPENDS: yap-uiux-2026-10-16-W0; coordinate with yap24-NT3/NT4 if they are open (same files):
      rebase onto whichever merged first and keep both
    FILES: desktop/src/views/Meetings.tsx, desktop/src/meetings/
    DO: read every "| Meetings |" row in docs/loop/UIUX-WALK-2026-10-16.md and fix each in the
      named files only. Record/stop is the first control; a transcribing meeting shows progress,
      never a bare spinner past 10 s; a needs-model state offers the one-click install, never an
      error. Re-walk Meetings for both fixture sets, after PNGs to the Obsidian walk dir under
      after/meetings/, one before/after pair on the PR, rows flipped to fixed with the PR number.
  `,
  acceptance: `
    ! grep '^| Meetings |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Meetings YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-first-run.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
    YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Meetings YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-daily.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
  `,
})

ITEMS.push({
  id: 'yap-uiux-2026-10-16-SCRATCH', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-scratchpad', gated: null,
  title: 'Scratchpad reads contained and obvious for both personas: every walk row on Scratchpad fixed',
  preflight: `
    grep -q '^| Scratchpad |' docs/loop/UIUX-WALK-2026-10-16.md
    ! grep '^| Scratchpad |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
  `,
  spec: `
    Panel: not required — fixes the W0 ledger names, no new features
    DEPENDS: yap-uiux-2026-10-16-W0
    FILES: desktop/src/views/Scratchpad.tsx
    DO: read every "| Scratchpad |" row in docs/loop/UIUX-WALK-2026-10-16.md and fix each in the
      named file only. Controls first, prose last; the editor surface is contained and its actions
      sit in a pinned footer. Re-walk Scratchpad for both fixture sets, after PNGs to the Obsidian
      walk dir under after/scratchpad/, one before/after pair on the PR, rows flipped to fixed with
      the PR number.
  `,
  acceptance: `
    ! grep '^| Scratchpad |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Scratchpad YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-first-run.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
    YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Scratchpad YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-daily.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
  `,
})

ITEMS.push({
  id: 'yap-uiux-2026-10-16-SETTINGS', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-settings', gated: null,
  title: 'Settings is contained: bordered surfaces, a pinned footer, every walk row on Settings and its eight panes fixed',
  preflight: `
    grep -q '^| Settings |' docs/loop/UIUX-WALK-2026-10-16.md
    ! grep '^| Settings |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
  `,
  spec: `
    Panel: not required — fixes the W0 ledger names, no new features (10-06 new item "settings
      containment (borders/surfaces/pinned footer)")
    DEPENDS: yap-uiux-2026-10-16-W0; follow the Y5-A token layer if #190 has merged, otherwise use
      the existing App.css variables and add none
    FILES: desktop/src/views/Settings.tsx, desktop/src/views/settings/*.tsx
    DO: read every "| Settings |" row in docs/loop/UIUX-WALK-2026-10-16.md and fix each in the
      named files only. Each pane is one contained surface; the toggle or picker comes first and
      its explanation follows in one line; save/reset actions sit in a pinned footer; nothing
      overflows at 720x520. Re-walk Settings for both fixture sets, after PNGs to the Obsidian walk
      dir under after/settings/, one before/after pair on the PR, rows flipped to fixed with the
      PR number.
  `,
  acceptance: `
    ! grep '^| Settings |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Settings YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-first-run.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
    YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Settings YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-daily.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
  `,
})

ITEMS.push({
  id: 'yap-uiux-2026-10-16-ONBOARD', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-onboarding-and-permissions', gated: null,
  title: 'First run is obvious: Onboarding, ModelSetup and Permissions say what to grant and why, one step at a time; every walk row fixed',
  preflight: `
    grep -q '^| Onboarding |' docs/loop/UIUX-WALK-2026-10-16.md
    ! grep '^| Onboarding |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
  `,
  spec: `
    Panel: not required — fixes the W0 ledger names, no new features (10-06 new item "first-run /
      permissions flow (mic, accessibility, input monitoring) with real screenshots"; the real
      screenshots are the headless walk's PNGs, never a visible app window)
    DEPENDS: yap-uiux-2026-10-16-W0; the Y1 permission items if open (same files)
    FILES: desktop/src/Onboarding.tsx, desktop/src/ModelSetup.tsx, desktop/src/views/Permissions.tsx,
      desktop/src/PermissionHealthRow.tsx
    DO: read every "| Onboarding |" row (and any "| Permissions |"-file finding W0 filed under
      Onboarding) in docs/loop/UIUX-WALK-2026-10-16.md and fix each in the named files only. One
      permission per step, the grant button first, one line of why after it; the model download
      shows size and progress; a denied grant says exactly where to fix it in System Settings.
      Never touch the bundle id (TCC grants). Re-walk with the first-run set (YAP_SMOKE_VIEWS
      Onboarding,Permissions), after PNGs to the Obsidian walk dir under after/onboarding/, one
      before/after pair on the PR, rows flipped to fixed with the PR number.
  `,
  acceptance: `
    ! grep '^| Onboarding |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Onboarding,Permissions YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-first-run.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
    YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Permissions YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-daily.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
  `,
})

ITEMS.push({
  id: 'yap-uiux-2026-10-16-Z', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-z-rewalk-after-flush', gated: null,
  title: 'Re-walk after the flush: both personas, every screen, zero open rows, the full smoke green — the acceptance of the UI/UX pass',
  preflight: `
    test -f docs/loop/UIUX-REWALK-2026-10-16.md
    ! grep -q '| open |' docs/loop/UIUX-WALK-2026-10-16.md
  `,
  spec: `
    Panel: not required — verification of the pass
    DEPENDS: every yap-uiux-2026-10-16 child above merged (HOME, DICT, INSIGHTS, MEETINGS, SCRATCH,
      SETTINGS, ONBOARD)
      QA TOOLS (yap-qa-harness, earlier in this same file, so the same lane, already merged): also run bash scripts/qa/perm-probe.sh and
      bash scripts/qa/hotkey-walk.sh --self-test and paste both outputs into the ledger header and the PR
      body. The full hotkey-walk (it posts fn into the installed Yap) is Wilson's walk step, never an agent's.
    DO: re-run the FULL walk (no YAP_SMOKE_VIEWS) for both fixture sets, PNGs to
      ~/Obsidian/Wilson-Brain/Projects/Loop-Logs/yap-uiux-2026-10-16/rewalk-first-run/ and
      .../rewalk-daily/. Write docs/loop/UIUX-REWALK-2026-10-16.md: per screen, the before and
      after verdict, the PR that fixed it, and the PNG paths. A NEW finding on the re-walk becomes a
      ledger row and is fixed here when it stays inside one view file; anything wider becomes a
      numbered entry in docs/loop/DEFERRED.md. The full smoke exiting 0 for both fixture sets is
      the acceptance of the whole UI/UX pass.
  `,
  acceptance: `
    bash scripts/qa/perm-probe.sh --self-test
    bash scripts/qa/hotkey-walk.sh --self-test
    test -f docs/loop/UIUX-REWALK-2026-10-16.md
    ! grep -q '| open |' docs/loop/UIUX-WALK-2026-10-16.md
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-first-run.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
    YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-daily.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
  `,
})

// yap-notetaker-* — WISPR FLOW NOTETAKER PARITY, YAP'S WAY (local, private). Written 2026-10-16 by the
// yap-notetaker seat (Opus). Wilson 20:30Z: "Wispr Flow is open right now … check the website, they'll
// literally tell you all of their features. You see how to set up Notetaker. I already like what our Yap
// looks like, I like the mascot, but we need to reverse engineer the Notetaker." His screenshot: a
// "Set up Notetaker" card — Take 2-min tour · Connect calendar · Import from Granola or Otter · Set up MCP.
// Sources (cited in ~/Obsidian/Wilson-Brain/Projects/Yap/wispr-notetaker-parity-2026-10-16.md):
// https://wisprflow.ai/notetaker, https://docs.wisprflow.ai/articles/1737760052 (calendar: Google +
// Outlook, NOT iCloud; Cloud Sync required), https://docs.wisprflow.ai/articles/9551372685 (remote MCP,
// read-only, no dictation history), https://docs.wisprflow.ai/articles/5166327995 (Granola/Otter kept as
// second MCP connectors). ORDER: after yap-uiux-2026-10-16-Z in this file (same lane, sequential), all
// three DEPEND on yap-meetings-fix. Not blockers for the uiux pass. They absorb the core of the
// panel-gated yap24-NT4, NT5, NT8 and satisfy their preflight markers.
// Yap stays local: nothing leaves the Mac unless the user picks a cloud model. We do NOT copy: Cloud Sync,
// remote account-wide MCP, auto-start recording, web search in ask, hosted share links, persistent voiceprints.
// Consent sentence (one, everywhere a recording is offered): "Recording is sacred: tell everyone on the call
// that Yap is taking notes, and it stays on this Mac."
// Review: Opus only (review-opus + review-opus2). SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap-notetaker-onboard', prompt: 'yap-notetaker', branch: 'loop/yap-notetaker-onboard-set-up-card', gated: null,
  title: 'A "Set up Notetaker" card: 2-minute tour, connect Apple Calendar, import from Granola or Otter, set up MCP — progress dots, dismissible, reopen from Help',
  preflight: `
    test -f desktop/src/meetings/NotetakerSetupCard.tsx
    test -f desktop/src-tauri/src/meeting_import.rs
    test -d desktop/yap-mcp
  `,
  spec: `
    Panel: not required — Wilson named the four steps from Wispr's card (2026-10-16).
    DEPENDS: yap-meetings-fix (a real transcript must exist for the tour, import and MCP to mean anything).

    THE CARD: top of the Meetings view until done or dismissed; four rows, one progress dot each (empty /
    done), a plain verb per row, contained panel (surface + border, pinned footer), mascot allowed, no new
    visual language. Dismiss = X with "Reopen anytime from Help"; Help menu gains "Set up Notetaker".
    State persists in settings_kv notetaker_setup_v1 (JSON: steps done + dismissed).
      1. Take the 2-minute tour — a local scripted walkthrough on a bundled SAMPLE meeting (a read-only
         fixture row, never recorded): start, live meters, stop, transcript, notes, export. Done = finished.
      2. Connect calendar — v1 Apple Calendar via EventKit, READ-ONLY, on-device (EKEventStore
         requestFullAccessToEvents on 14+, NSCalendarsFullAccessUsageDescription in Info.plist). This also
         sees Google/Exchange accounts the user already added to macOS Calendar — say so in one line.
         Google/Microsoft OAuth listed as "Coming". Done = access granted and >= 1 calendar readable.
      3. Import from Granola or Otter — pick a file or folder. Otter: .txt / .docx / .srt exports (formats
         per a third-party guide — UNVERIFIED against Otter's own docs: the builder must confirm with real
         sample exports and record what it confirmed in the PR). Granola: export format UNVERIFIED — research
         first (Granola help pages; its Markdown/Notion export); if no stable file format is documented,
         ship Otter only and show Granola as "Coming" with the reason. Imports become meetings with
         source = 'imported:otter' | 'imported:granola', segments + any summary, searchable via FTS.
         Never call their cloud APIs; files only.
      4. Set up MCP — Yap has NO MCP server today (grep: zero MCP code). Build desktop/yap-mcp: a local
         stdio MCP server (official Rust SDK rmcp — pin a released version after reading its docs), shipped
         inside the bundle (Contents/MacOS/yap-mcp, signed with the app), opening wilson_voice.db READ-ONLY.
         Resources/tools: list_meetings, get_transcript, get_summary, list_action_items, search_meetings.
         Permission table (settings_kv mcp_permissions_v1), each OFF until the user turns it on: meetings
         list · transcripts · summaries · action items. Dictation history is NEVER exposed (Wispr's line
         too). The row shows a "Copy config" button with the claude_desktop_config / Cursor JSON snippet
         pointing at the bundled binary. Done = config copied and a self-test call answered.

    RUNTIME DEPENDENCIES: | EventKit | OS | | yap-mcp | built + signed in our bundle | | rmcp crate | vendored
    at build time, pinned | — nothing found on the machine.

    TESTS: vitest NotetakerSetupCard (4 rows, dots, dismiss persists, Help reopens); cargo test
    meeting_import (Otter txt/srt fixtures → segments; malformed file → a plain error, no partial row);
    cargo test -p yap-mcp (permission OFF → resource refused; read-only: an INSERT through the connection
    fails); tests/calendar_permission_readonly.rs (no EKEventStore save/remove call anywhere: grep gate).

    WHAT NOT TO DO: no OAuth, no network in import or MCP, no write access over MCP, no new mascot.
    Critiqued once (self-pass): the draft exposed MCP by default; Wispr's "account-wide, no per-meeting
    consent" is exactly what we are not copying, so every class now starts OFF.
  `,
  acceptance: `
    test -f desktop/src/meetings/NotetakerSetupCard.tsx
    test -f desktop/src-tauri/src/meeting_import.rs
    test -d desktop/yap-mcp
    grep -q "NSCalendarsFullAccessUsageDescription" desktop/src-tauri/Info.plist
    ! grep -rn "saveEvent\|removeEvent\|save(event\|remove(event" desktop/src-tauri/src
    grep -q "Set up Notetaker" desktop/src/meetings/NotetakerSetupCard.tsx
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
    cd ${APP} && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test -p yap-mcp ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol meeting_import ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test calendar_permission_readonly ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'yap-notetaker-detect', prompt: 'yap-notetaker', branch: 'loop/yap-notetaker-detect-offer-to-record', gated: null,
  title: 'Yap notices a meeting — a calendar event with a call link in the next 2 minutes, or Zoom/Meet/Teams/FaceTime with live audio — and the pill offers "Record this meeting?" with the consent line; it never auto-records',
  preflight: `
    test -f desktop/src-tauri/src/meeting_detect.rs
    test -f desktop/src-tauri/tests/calendar_prompt_policy.rs
  `,
  spec: `
    Panel: not required — Wilson asked for Wispr parity (2026-10-16); Wispr: "Notetaker will detect that you
    are in a call and ask if you want to start taking notes" (wisprflow.ai/notetaker).
    DEPENDS: yap-meetings-fix, yap-notetaker-onboard (EventKit access lives there).

    SIGNALS (any one raises the offer; meeting_detect.rs, a pure policy fn + thin OS adapters):
      a. Calendar: an EventKit event starting within 2 minutes (or in progress < 5 min) whose URL, location
         or notes contain a conferencing link (zoom.us/j, meet.google.com, teams.microsoft.com,
         facetime.apple.com, whereby, webex). Read-only.
      b. App: a running zoom.us / Microsoft Teams / FaceTime process AND that app's audio is live
         (CoreAudio process object kAudioProcessPropertyIsRunningInput or IsRunningOutput, macOS 14.4+; below
         14.4 the app signal is off and only the calendar signal runs). Meet in a browser: a browser window
         title matching "Meet - " / "meet.google.com" via Accessibility (already granted for paste) AND the
         browser's audio live.
    OFFER: the pill (locked pixel-art mascot) expands with "Record this meeting?" + the meeting title when
    known + the consent sentence + [Record] [Not now]. Record starts kind = Virtual. Not now snoozes that
    event / app session (no repeat for the same calendar event or the same app run). Never starts recording
    on its own; there is no auto-start setting. Settings › Meetings: "Offer to record detected meetings"
    (default ON) and "Use my calendar" (default follows step 2 of the setup card). Polling: event-driven
    (NSWorkspace launch/terminate + an EventKit change notification + a single 30 s timer) — never faster.

    TESTS: tests/calendar_prompt_policy.rs (table: link+2 min → offer; no link → none; snoozed → none; in
    progress 4 min → offer, 6 min → none); meeting_detect unit tests for the app table (audio live required;
    Zoom open but silent → none); vitest for the pill offer (consent sentence present; Not now emits snooze).

    RUNTIME DEPENDENCIES: | EventKit, NSWorkspace, CoreAudio process objects | OS |.
    WHAT NOT TO DO: no auto-record, no network, no reading meeting content to decide, no polling < 30 s.
    Critiqued once (self-pass): "Zoom is running" alone would prompt all day for people who leave Zoom open;
    the app signal now requires live audio from that app.
  `,
  acceptance: `
    test -f desktop/src-tauri/src/meeting_detect.rs
    test -f desktop/src-tauri/tests/calendar_prompt_policy.rs
    grep -q "Recording is sacred" desktop/src/pill/*.tsx desktop/src/meetings/*.tsx
    ! grep -rn "auto_record\|autoRecord" desktop/src desktop/src-tauri/src
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test calendar_prompt_policy ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol meeting_detect ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'yap-notetaker-notes', prompt: 'yap-notetaker', branch: 'loop/yap-notetaker-notes-speakers-summary-actions', gated: null,
  title: 'Notes worth reading: transcript in speaker turns, a summary with decisions and action items from the user’s chosen model, searchable, Markdown export, MCP-readable — nothing leaves the Mac unless the user picks a cloud model',
  preflight: `
    grep -q "CREATE TABLE IF NOT EXISTS meeting_actions" desktop/src-tauri/src/db.rs
    test -f desktop/src-tauri/tests/meeting_notes_auto_summary.rs
    test -f desktop/src-tauri/tests/meeting_notes_stay_local.rs
  `,
  spec: `
    Panel: not required — Wilson 2026-10-16 (Wispr parity). Wispr: live transcript "separates you from other
    speakers"; summary pulls out "timelines, decisions, and next steps"; labels "not voiceprints".
    DEPENDS: yap-meetings-fix (segments exist), yap-notetaker-onboard (yap-mcp exists).

      1. Speaker turns: two-track meetings label track 0 "You" and track 1 "Call" (TrackSpeakers); in-person
         and track 1 with >1 voice run the SHIPPED diarize sidecar (diarize.rs; diarize::pool() gets its first
         caller) after transcription: "Speaker 1/2/…", renamable once and applied across the meeting (Wispr
         parity). Calendar attendees (when step 2 is done) are offered as rename choices. Embeddings are per
         meeting and discarded — no cross-meeting voiceprints.
      2. Live transcript (light): while recording, a rolling decode every ~10 s on each track shows the last
         lines under You / Call in the Meetings view. The final transcript replaces it after Stop.
      3. Summary + action items: automatic after Complete via the user's chosen model — local default
         (polishModel; qwen2.5-1.5b today) or a BYO-key cloud model ONLY if the user picked one in Settings.
         Sections: Summary · Decisions · Action items (owner, text, cited segment) · Open questions.
         Action items are rows in meeting_actions (meeting_id, owner, text, segment_id, done). No model =
         "Add a summary model in Settings", never an error.
      4. Search: FTS over segments + summaries + action items, from the Meetings search box.
      5. Export: Markdown with speakers, timestamps, summary, action items as "- [ ]" (export_meeting_markdown).
      6. MCP: yap-mcp resources return the same transcript/summary/action items, under the permission table.
    PRIVACY TEST: tests/meeting_notes_stay_local.rs — with a local model, the summarize path makes zero
    network calls (a deny-all HTTP client seam that panics on use).

    RUNTIME DEPENDENCIES: | wespeaker CAM++ / diarize sidecar | downloaded by the app (already in models/) |
    | summary model | downloaded by the app on opt-in | | cloud model | only with the user's own key |.
    TESTS: tests/meeting_notes_auto_summary.rs (Complete → summary + >= 1 action item row on the fixture
    transcript with a stub model; no model → state message); diarize wiring test on a 2-voice fixture;
    vitest for the Meetings detail (speaker turns, rename applies to all, action item checkbox persists).
    WHAT NOT TO DO: no upload of audio or transcript by default; no persistent voiceprints; no web search.
    Critiqued once (self-pass): live transcript on a 3B-param budget would starve dictation; it is capped to
    a light rolling decode that yields to any dictation take.
  `,
  acceptance: `
    grep -q "CREATE TABLE IF NOT EXISTS meeting_actions" desktop/src-tauri/src/db.rs
    test -f desktop/src-tauri/tests/meeting_notes_auto_summary.rs
    test -f desktop/src-tauri/tests/meeting_notes_stay_local.rs
    grep -rln "diarize::pool()" desktop/src-tauri/src | grep -v "diarize.rs" | grep -q .
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
    cd ${APP} && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test -p yap-mcp ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_notes_auto_summary ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_notes_stay_local ; test $? -eq 0
  `,
})

// yap-rereview-followups-2026-10-16 — the advisories the #216/#217 re-reviews raised and did not block on.
// Written 2026-10-16 by the items-followups seat. Review: Opus only (review-opus + review-opus2).
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap-structure-followups', prompt: 'yap-format-structure', branch: 'loop/yap-structure-followups-keep-every-word-finish-every-list', gated: null,
  title: 'The structure stage keeps every word and finishes every list: narration "Then"/"Finally" survive, four-step lists stay one list, product names keep their casing',
  // Pre-flight asserts COMPLETION (the six regression inputs live in structure.rs's tests and the
  // module's tests pass), never a dependency: a passing pre-flight retires the item as already-done.
  preflight: `
    grep -qF 'Then the meeting ran long.' desktop/src-tauri/src/structure.rs
    grep -qF "Finally I'm home." desktop/src-tauri/src/structure.rs
    grep -qF 'Then we test it. Then we ship it.' desktop/src-tauri/src/structure.rs
    grep -qF 'Then you tap General. Finally, tap Reset.' desktop/src-tauri/src/structure.rs
    grep -qF 'Finally at checkout' desktop/src-tauri/src/structure.rs
    grep -qF 'eBay' desktop/src-tauri/src/structure.rs
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib structure ; test $? -eq 0
  `,
  spec: `
    Panel: not required — FIX (re-review advisories on PR #217, merged cf7c8c2).
    Wilson's words (2026-10-16): "You see how Wispr Flow does the formatting? That's what Yap needs to be able to do. It knew I was doing a list of things."
    DEPENDS: none (PR #217 merged cf7c8c2).

    WHY: the structure stage (desktop/src-tauri/src/structure.rs) turns spoken enumerations into lists. The re-review
    found cases where it still drops a word, stops a list early, or forces prose. Each advisory below becomes ONE unit
    test in structure.rs with the exact input and expected output; write the test first, watch it fail, then fix.

    TESTS (exact input -> expected output):
      1. Narration after a list keeps "Then". Input: a list followed by "Then my wife called…" and, separately,
         "Then the meeting ran long." Expected: the sentence stays as prose with "Then" intact. The narration check
         knows only pronouns today; extend it so a sentence-initial "Then" followed by a noun phrase (a determiner or a
         possessive: my, the, our, his, her, their, a) is narration, not a list cue.
      2. "Finally I'm home." keeps "Finally" (a "Finally" followed by a pronoun + contraction or a pronoun is narration).
      3. "First, we design it. Then we build it. Then we test it. Then we ship it." -> ONE list of FOUR items
         (today it stops at three and drops the last step to prose). Find why the run ends at three and let a list
         continue while each following sentence opens with a cue.
      4. "First, open Settings. Then you tap General. Finally, tap Reset." -> a list of THREE items (regressed to prose
         because "you" read as narration). An imperative-with-"you" step inside an established list is a step.
      5. "Finally at checkout, pay…" keeps every word and is not forced to prose only because of "at".
      6. Product names keep their dictated casing when list items are capitalised: iPhone, eBay, macOS stay exactly
         as dictated (assert "iPhone", "eBay", "macOS" appear verbatim in the output; only a lowercase ordinary first
         letter is capitalised). Use a small never-recase list or the rule "first letter is lowercase and the second
         letter is uppercase -> leave alone".

    GOLDENS: desktop/yap-polish/fixtures/structure/* stay unchanged. If one must change, name that fixture and the reason
    in the PR body.
    NOTHING DROPPED: the existing every-input-word-appears-in-the-output check runs over all six new inputs.
    RUNTIME DEPENDENCIES: none new.
    WHAT NOT TO DO: no model calls, no new crate, no rewording of the user's words.
    Critiqued once (self-pass): widening "Then" as narration could break genuine "First ... Then ..." lists; test 3 and 4
    guard that, and the narration rule only applies when a determiner or possessive follows.
  `,
  acceptance: `
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib structure ; test $? -eq 0
    cd ${APP} && npx tsc --noEmit
  `,
})

ITEMS.push({
  id: 'yap-recovery-followups', prompt: 'yap-hotkey-stop', branch: 'loop/yap-recovery-followups-no-duplicates-no-race-backend-confirm', gated: null,
  title: 'Recovered long takes: no duplicate history entries, no race, confirmation enforced in the backend',
  // Pre-flight asserts COMPLETION (the nine named tests exist), never a dependency: a passing
  // pre-flight retires the item as already-done, a failing one dispatches the builder. PR #216 is MERGED
  // (2026-10-17, 07f738d), so the gh pr view 216 state check (first acceptance command) now passes; it is
  // kept as a guard so a build cut from a base without #216 can never go green or open a mergeable PR.
  preflight: `
    git grep -q 'fn db_b_two_concurrent_saves_yield_one_row' -- desktop/src-tauri
    git grep -q 'fn db_b_keep_without_confirm_is_refused' -- desktop/src-tauri
    git grep -q 'fn db_b_keep_while_retry_in_flight_is_refused' -- desktop/src-tauri
    git grep -q 'fn db_b_retry_and_keep_after_live_partial_leave_one_row' -- desktop/src-tauri
    git grep -q 'fn db_b_all_loop_or_unreadable_resume_converts_without_keep' -- desktop/src-tauri
    git grep -q 'db_b_already_transcribed_shows_gated_words' -- desktop
    git grep -q 'fn db_b_short_loop_then_clean_words_kept_on_retry' -- desktop/src-tauri
    git grep -q 'fn db_b_no_model_still_offers_keep' -- desktop/src-tauri
    git grep -q 'fn db_b_missing_audio_keeps_recovered_words' -- desktop/src-tauri
  `,
  spec: `
    Panel: not required — FIX (re-review advisories on PR #216).
    Wilson's words (2026-10-16): "A crash or quit mid-long-take loses nothing."
    DEPENDS: PR #216 (long-take recovery) is MERGED (2026-10-17, 07f738d); the FIRST acceptance command checks it and
    passes. If it ever fails, stop, push nothing, report BLOCKED on #216. Do not re-implement #216's work here.

    ADVISORIES, each with a test:
      (a) Two concurrent saves can add the same take twice. Add a backend idempotency key per take (the take id, a
          UNIQUE column or an in-memory claim taken under the same lock as the insert) so the second save is a no-op
          that returns the first entry. Test: two concurrent saves of one take yield exactly one history row.
      (b) The "Keep what was transcribed" confirmation exists only in the UI. The backend command must require an
          explicit confirm flag (confirm: true; absent or false returns a typed refusal) and must refuse while a
          retry for that take is in flight. Tests: no flag -> refused, nothing written; flag while retry in flight ->
          refused; flag otherwise -> saved.
      (c) Retry or Keep on a live take that partly failed creates a second history entry beside the one saved live.
          Merge into the existing entry (update its text and status) instead of inserting. Test: after a live partial
          save, Retry and Keep each leave exactly one history row for the take.
      (d) The toast after a failed Finish reads like engineer output. Rewrite it in plain words naming what the user
          can do: Retry, Keep what was transcribed, or Discard. No error codes, no ids, no stack text. Vitest asserts
          the exact copy and that it names all three actions.
      (e) When the recovered words are entirely a repetition loop, or the resume point cannot be read, the retry already
          re-decodes the whole clip from the start. Do NOT flag a gate cut in that case: flag only when the gated
          recovered words are non-empty AND shorter than the raw, so a clean full decode converts on its own. Test:
          both histories (all-loop recovered words; unreadable resume point) convert without Keep.
      (f) The row's "Already transcribed" line must show the GATED recovered words (the same text as "Transcribed so
          far"), never the raw loop. Test: vitest or Rust test as appropriate, asserting a row whose raw text is a loop
          renders the gated text on both lines.
      (g) The hallucination gate drops every word after a loop. For retries, keep clean words that resume after a short
          loop (bounded: loop <= N tokens followed by >= M distinct tokens; pick N and M as named constants) so Keep does
          not discard 40 clean words after "the" x8. Test with that exact input: "the" repeated 8 times followed by 40
          distinct clean words keeps all 40 on retry.
      (h) With no model installed, or the audio file missing, Finish must still offer Keep when words are saved, and the
          missing-audio path must never delete the row's recovered words. Test both: no model -> Keep offered; missing
          audio -> recovered words still present after the attempt.

    TESTS: backend tests in the db tests module, named EXACTLY (the pre-flight greps for them):
    db_b_two_concurrent_saves_yield_one_row (a), db_b_keep_without_confirm_is_refused and
    db_b_keep_while_retry_in_flight_is_refused (b), db_b_retry_and_keep_after_live_partial_leave_one_row (c),
    db_b_all_loop_or_unreadable_resume_converts_without_keep (e), db_b_short_loop_then_clean_words_kept_on_retry (g),
    db_b_no_model_still_offers_keep and db_b_missing_audio_keeps_recovered_words (h); for (f) the test's name (Rust fn
    or vitest title) must contain db_b_already_transcribed_shows_gated_words. The pre-flight greps all nine, so the
    item is never retired as already-done while any of (a)-(c) or (e)-(h) is unimplemented;
    desktop/src-tauri/tests/long_take_recovery.rs
    extended for the same three; a vitest next to the toast component for (d).
    RUNTIME DEPENDENCIES: none new.
    WHAT NOT TO DO: no schema change that needs a destructive migration; no UI-only enforcement; do not weaken the
    UI confirmation, the backend check is in addition.
    Critiqued once (self-pass): an in-memory-only idempotency claim would not survive a restart, so the key is also
    persisted with the row; the confirm flag defaults to refusing so an old caller cannot bypass it.
  `,
  acceptance: `
    gh pr view 216 -R wilsonguenther-dev/wilson-voice --json state -q .state | grep -qx MERGED || { echo "BLOCKED: PR #216 (long-take recovery) is not merged yet; this item builds on it"; exit 1; }
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib db_b_ ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test long_take_recovery ; test $? -eq 0
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
  `,
})

// yap-review-pass-2-audit (2026-10-17T01:2xZ, seat yap-pass-summary, read-only audit of review pass
// wf_d0209003-efe 20:17Z->01:20Z). Two items, FIX, no panel (harness + test hygiene, unambiguous):
//   1. THE LAND GATE WAS CI-ONLY. All six merged PRs (#178 #192 #195 #212 security, #196 #213 standard)
//      and #214 landed with NO reviewer PASS on the merged head. The reviewers' verdicts were BLOCK on the
//      pre-fix heads; the fix-land seat fixed its own findings and posted "Primary/Independent adversarial
//      review (Opus)" bodies on the new head itself ("Verdict at review time: BLOCK. Disposition after the
//      fix round: FIXED"). The land sweeper's gate was the CI check on headRefOid plus sidecar build, fmt and
//      clippy ("I did not run cargo test locally"). template.mjs says "THERE IS NO SECOND REVIEW ROUND IN THIS
//      PASS" and CI_GATE never reads a review record. #196's only PASS was on cb60606; it landed at 16f7241
//      after a float.css conflict resolution nobody reviewed. #214 was merged by the drain seat on its own
//      fixes to Codex-bot suggestions, with no Opus seat at all.
//   2. meeting::tests ARE NOT HERMETIC. SessionConfig::new (meeting.rs) defaults env to SystemEnv, whose
//      battery() runs /usr/bin/pmset. Seven meeting::tests failed at recon with
//      "LowBattery { percent: 11, budgeted: 10800s }" because the host Mac was at 11% on battery.

ITEMS.push({
  id: 'yap-land-gate-pass-required', prompt: 'yap-land-gate', branch: 'loop/yap-land-gate-pass-required-review-record-on-head', gated: null,
  title: 'No PR lands without an Opus PASS bound to its head: one seat for standard, two different seats for security, enforced by scripts/loop/review-gate.mjs in every merge path (LAND, fix-land, land sweeper, drain)',
  preflight: `
    test -f scripts/loop/review-gate.mjs
    test -f scripts/loop/review-gate.test.mjs
    grep -q "review-gate.mjs gate" scripts/loop/template.mjs
    ! grep -q "THERE IS NO SECOND REVIEW ROUND IN THIS PASS" scripts/loop/template.mjs
  `,
  spec: `
    Panel: not required — harness defect found by the review pass 2 audit (2026-10-17). Opus only, every seat.
    EVIDENCE (pass wf_d0209003-efe): every merged head carries only the FIXER's disposition review, never a
    reviewer PASS. #178 96e5380, #192 45fa4cd, #195 1dfb328, #212 0b7449b, #213 d956c11: reviewer verdicts BLOCK
    on the earlier heads, fixer self-attested FIXED on the merged head. #196 16f7241: reviewer PASS on cb60606
    only. #214 8bfd8a5: no Opus review at all (drain fixed Codex-bot suggestions and merged). The sweeper's gate
    was CI on headRefOid + build/fmt/clippy, no cargo test.

      1. Port Learn's gate: scripts/loop/review-gate.mjs, modelled line for line on Drivia Learn's H-GATE-1 gate
         (Learn repo, main branch, file scripts/loop/review-gate.mjs; read it with gh api). DEFAULT_REPO wilsonguenther-dev/wilson-voice,
         RECORD_MARKER "loop-review (Opus)", seats review-opus and review-opus2, EXIT_REVIEW 6, EXIT_USAGE 64,
         REVIEW_AUTHORS the account the seats post as. Subcommands: gate --pr N [--head SHA], record --pr N
         --seat --lens --head --verdict PASS|BLOCK --blocking N, klass --pr N. The record is a PR comment whose
         first line is the marker and which names seat, lens, head (40 hex) and "verdict: PASS blocking=0" or
         "verdict: BLOCK blocking=n". record refuses a sha that is not the PR's current head.
      2. Decision against the CURRENT head only. SECURITY class (the triage klass rule: SECURITY_IDS,
         SECURITY_PATHS, a security label, or a file list longer than one page) needs a PASS from review-opus
         AND a PASS from review-opus2 on the head. STANDARD needs one PASS from either seat on the head. There is
         NO unreviewed exception in Yap: no record on the head refuses. Not a record: a fixer seat (any seat
         starting "fix"), a drain/sweeper/land seat, an unknown seat, a bot author, an edited comment, PASS with
         blocking > 0, a short head. A seat whose newest record on the head is BLOCK refuses. Records on older
         heads are history and never count, so any push (a rebase, a conflict resolution, a fix) needs a fresh
         PASS.
      3. Wire it into every agent that can merge: the build-mode LAND step, fix-land, the land sweeper and the
         drain. Each runs "node scripts/loop/review-gate.mjs gate --pr N --head <headRefOid>" immediately before
         gh pr merge --match-head-commit <same sha>, and does not merge on a non-zero exit (it labels the PR
         review-pending and names the missing seat). CI_GATE and the local gate stay as they are — they are
         necessary, not sufficient.
      4. The review pass gets its second round. After fix-land pushes a fix it does NOT merge: the chain
         re-dispatches the reviewer seat(s) that blocked (same lens, fresh context) at the new head, and they
         post their own record with "record". Cap: two fix rounds; a third BLOCK ends as needs-human. Replace
         the "THERE IS NO SECOND REVIEW ROUND IN THIS PASS" contract text accordingly. Reviewer seats post the
         record themselves; the fixer never posts a verdict body titled as a review.
      5. build.mjs validation: every prompt that contains "gh pr merge" also contains "review-gate.mjs gate";
         review-gate.mjs is in the harness SECURITY_PATHS; and ONLY reviewer-seat prompts (review-opus,
         review-opus2) contain "review-gate.mjs record" — a fix, fix-land, land, sweeper or drain prompt that
         contains it fails validation.
      6. TRUST LIMIT — say it, do not over-claim it. Every seat posts through the same GitHub account, so the
         record's seat field is self-declared: the gate CANNOT authenticate which seat wrote a record, and a
         seat that runs "record --seat review-opus" is indistinguishable from the real reviewer. What this item
         enforces is (a) a PASS bound to the current head exists, and (b) only reviewer prompts are ever handed
         the record subcommand (validation in 5). Write that limit, in those words, in the review-gate.mjs
         header and in the PR body. Never describe the gate as authenticating or unforgeable. Seat
         authentication (per-seat credentials, or records the controller signs) is out of scope here.
    TESTS: scripts/loop/review-gate.test.mjs (node --test), hermetic — gh is a stub fed from fixture JSON via an
    injectable runner, never the network. Cases: (a) CI green, MERGEABLE, no record on any head -> gate exit 6;
    (b) standard PR, one review-opus PASS on the head -> 0; (c) PASS on an older head only (the #196 shape) -> 6;
    (d) security PR, PASS from review-opus only -> 6; two PASSes both from review-opus -> 6; review-opus +
    review-opus2 PASS on head -> 0; (e) a fix-land seat record saying PASS on the head (the #178 shape) -> 6;
    (f) newest review-opus2 record on head is BLOCK -> 6; (g) edited record -> 6; (h) record --head with a sha
    that is not the current head -> 64; (i) build.mjs validation fails on a stub fix-land prompt containing
    "review-gate.mjs record".
    WHAT NOT TO DO: no Codex/Gemini/bot review counts; never accept a verdict from a loop log instead of the PR.
  `,
  acceptance: `
    test -f scripts/loop/review-gate.mjs
    test -f scripts/loop/review-gate.test.mjs
    node --test scripts/loop/review-gate.test.mjs
    grep -q "review-gate.mjs gate" scripts/loop/template.mjs
    ! grep -q "THERE IS NO SECOND REVIEW ROUND IN THIS PASS" scripts/loop/template.mjs
    cd ${APP} && npm run loop:build && npm run loop:validate
  `,
})

ITEMS.push({
  id: 'yap-tests-hermetic-power', prompt: 'yap-tests-hermetic', branch: 'loop/yap-tests-hermetic-power-injected-power-source', gated: null,
  title: 'Meeting tests stop reading the host battery: an injected PowerSource, pmset only in production wiring, so cargo test passes on a Mac at 5% on battery',
  preflight: `
    grep -q "trait PowerSource" desktop/src-tauri/src/meeting.rs
    grep -q "YAP_HOST_BATTERY_OVERRIDE" desktop/src-tauri/src/meeting.rs
  `,
  spec: `
    Panel: not required — test-design defect (non-hermetic tests), found by the review pass 2 audit.
    EVIDENCE: recon 2026-10-16T20:2xZ, cargo test exit 101, 7 failures all in meeting::tests
    (a_dictation_take_mid_meeting_costs_the_meeting_nothing, a_meeting_registers_and_deregisters_the_active_capture,
    a_meeting_session_holds_its_own_audio_stream_for_its_whole_length, a_meeting_whose_stream_refuses_never_starts,
    a_refused_start_releases_its_claim, a_second_meeting_is_refused_not_silently_swapped_in,
    a_session_that_lost_the_slot_finalizes_partial_and_leaves_it_alone), panic "LowBattery { percent: 11,
    budgeted: 10800s }"; pmset showed the host on battery at 11%. Root cause: SessionConfig::new sets
    env: Arc::new(SystemEnv { path }), SystemEnv::battery() calls probe_battery() (/usr/bin/pmset -g batt), and
    the tests build sessions with ..SessionConfig::new(&dir, ...). The CaptureEnv seam exists
    (tests/matrix_row4_disk_preflight.rs uses FakeDisk) but the default constructor wires the real machine, and
    tests/matrix_row3_mic_denied_system_only.rs plus matrix_row4 inherit it for battery and thermal too.

      1. A PowerSource trait (fn battery(&self) -> BatterySnapshot) in meeting.rs. PmsetPower is the only
         implementation that runs pmset. SystemEnv takes its battery from a PowerSource it is given.
      2. Production wiring passes the real machine explicitly: the session constructors used by
         meeting_control.rs (~942) and syscapture.rs (~2106, ~2749) build SystemEnv with PmsetPower. The
         generic constructor used by tests (SessionConfig::new or a new SessionConfig::hermetic) defaults to a
         HermeticEnv: AC power at 100%, nominal thermal, ample disk — so no unit or integration test reads pmset,
         statvfs or the thermal state unless it injects them on purpose.
      3. A debug-only seam to PROVE hermeticity: PmsetPower honours YAP_HOST_BATTERY_OVERRIDE="<percent>,<ac|battery>"
         under cfg(debug_assertions) only, so a test run can simulate a dying host. Release builds compile it out.
      4. Battery behaviour stays tested, through injection: a test that injects 5% on battery with the default
         3 h budget asserts PreflightError::LowBattery, and one on AC asserts it starts.
    TESTS: meeting::tests::hermetic_power_override_is_real (with the override set to 5,battery, PmsetPower reports
    5% on battery — so the acceptance run below is not vacuous); the injected-5% LowBattery test; every existing
    meeting::tests case unchanged in intent.
    WHAT NOT TO DO: do not mark the seven tests #[ignore]; do not loosen the LowBattery threshold; do not read the
    override in release builds.
  `,
  acceptance: `
    grep -q "trait PowerSource" desktop/src-tauri/src/meeting.rs
    grep -q "YAP_HOST_BATTERY_OVERRIDE" desktop/src-tauri/src/meeting.rs
    cd ${APP}/src-tauri && YAP_HOST_BATTERY_OVERRIDE="5,battery" YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib meeting:: ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_HOST_BATTERY_OVERRIDE="5,battery" YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test matrix_row3_mic_denied_system_only --test matrix_row4_disk_preflight ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib meeting::tests::hermetic_power_override_is_real ; test $? -eq 0
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
  notes: `STATUS 2026-09-26: MERGED (PR #168) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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
  notes: `STATUS 2026-09-26: MERGED (PR #169) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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
  notes: `STATUS 2026-09-26: MERGED (PR #171 then #184) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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
  notes: `STATUS 2026-09-26: MERGED (PR #173) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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
  notes: `STATUS 2026-09-26: ALREADY-DONE — the executed pre-flight passed on main in run wf_f6cd9dfa-860; no PR.`,
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
  notes: `STATUS 2026-09-26: MERGED (PR #175) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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
  notes: `STATUS 2026-09-26: MERGED (PR #177) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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
  notes: `STATUS 2026-09-26: MERGED (PR #179) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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
  notes: `STATUS 2026-09-26: MERGED (PR #182) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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
  notes: `STATUS 2026-09-26: MERGED (PR #170) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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
  notes: `STATUS 2026-09-26: MERGED (PR #172) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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
  notes: `STATUS 2026-09-26: MERGED (PR #174) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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
  notes: `STATUS 2026-09-26: MERGED (PR #176) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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
  notes: `STATUS 2026-09-26: BUILT, PR #178 OPEN and stale (main moved 30+ times under it). Rebase that PR onto main and finish it; do not open a second PR.`,
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
  notes: `STATUS 2026-09-26: MERGED (PR #180) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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
  notes: `STATUS 2026-09-26: MERGED (PR #181) in run wf_f6cd9dfa-860 — acceptance failed twice before merge (needs-human): the review pass re-checks it. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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
  notes: `STATUS 2026-09-26: BUILT, PR #183 OPEN and stale (main moved 30+ times under it) — Panel revision 2026-09-26T17:35:00Z: DO NOT LAND #183 AS-IS. It targets "a dedicated Yap Supabase project," which the product ledger (2026-09-26 panel) now overrides: issuance moves to the Drivia Consulting app's Supabase DB #2 (the products licensing backend, already provisioned and already licensing other products) — see the Panel revisions block in the spec below. Relabel #183 needs-human until that redirect is done; a scoped notetaker-only loop pass must never merge it as written (see the LAND-scope fix in scripts/loop/template.mjs). RE-VERIFIED 2026-09-26: the revocation host license.rs:117 (forge sslip.io) does not answer (curl HTTP 000) and Wilson's yap.log shows "revocation refresh skipped" on every launch; with Forge off there is NO issuer, so a new purchase cannot receive a key, AND every launch still sends an unauthenticated GET to a hostname resolving to an IP Wilson no longer controls, accepting whatever unsigned {kids:[]} revocation list comes back with no signature check (license.rs:1100-1124) — ship the one-line mitigation in the Panel revisions block FIRST, independent of the rest of this item.`,
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

    Panel revisions 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, BLOCKING ledger-conflict + HIGH
    security, GROUNDED):
      - SHIP THIS ONE LINE NOW, independent of the rest of the item: in license.rs, make
        REVOCATION_URL an Option set to None so no fetch happens at all until the real endpoint
        below exists. Today every launch sends an unauthenticated GET to
        forge.87-99-149-214.sslip.io — sslip.io maps any name to its embedded IP, so whoever
        holds that IP today can also get a TLS cert for it — and deserializes whatever
        {kids:[]} comes back with NO signature check (license.rs:1100-1124), which contradicts
        both license.rs:111-113's own "the ONLY host this module ever contacts" comment and the
        "nothing leaves the Mac" privacy claim (it sends the client IP and launch timing to a
        third party). Correct docs/ARCHITECTURE-AUDIT-2026-09-26.md §9's "harmless no-op"
        characterization to match.
      - THE ISSUER MOVES TO THE DRIVIA CONSULTING APP'S SUPABASE DB #2 (the products licensing
        backend), NOT a new "dedicated Yap Supabase project" — the product ledger (2026-09-26
        panel, binding) is explicit that issuance moves there; the panel shapes HOW, never
        WHETHER. DB #2 already seeds a 'yap' product (purchase_enabled=true) with
        /api/v1/licensing/{activate,deactivate,refresh,entitlements,settings} and a Stripe
        webhook that issues licenses — this item becomes "make Yap a CLIENT of that issuer,"
        not "build a second issuer." Everything below in "WHAT TO BUILD" that describes a new
        Supabase Edge Function in THIS repo is superseded: delete the
        supabase/functions/yap-license/index.ts step; instead license.rs gains a verifier for
        DB #2's token format (a signed JWS carrying sub, act, prd, plan, ent — Ed25519/EdDSA,
        kid in the header, key pinned from licensing.signing_keys), kept ALONGSIDE the legacy
        base64url-claims-plus-signature format so Wilson's existing lifetime license keeps
        verifying. PAYMENT_LINK_URL points at the Consulting product's checkout. Activation
        happens once, when the token is pasted or deep-linked — never on a hot path — to honor
        "the ONLY host this module ever contacts" as far as it can still be kept true.
      - Pricing, trial length and whether the Stripe payment link goes live are Wilson's calls
        (ledger), unaffected by this redirect — this item does not resolve $29-lifetime vs the
        DB #2 seeded monthly/annual plans; flag both to Wilson (see forWilson).
      - Cross-repo follow-up (drivia-consulting, out of THIS repo's loop): a Yap-facing checkout
        page and license-delivery email on the Consulting app so a purchase actually reaches a
        Yap user. Record this as a handoff, not an item in this file.

    THE SUPABASE PROJECT IS A RUNTIME DEPENDENCY WILSON PROVISIONS. [SUPERSEDED BY THE PANEL
    REVISION ABOVE — kept verbatim below for history; do not build it as written.]
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
  notes: `STATUS 2026-09-26: MERGED (PR #185) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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
  notes: `STATUS 2026-09-26: BUILT, PR #186 OPEN and stale (main moved 30+ times under it). Rebase that PR onto main and finish it; do not open a second PR. AMENDED 2026-09-26 (architecture audit): the display POLICY this item draws is superseded in part by yap24-PILL1 — the pill must show the trial from day 14, not only from day 7 (pill/license.ts:136-165 is silent above 7 days; Wilson 2026-09-26 "the pill does not indicate ... 14 days"). Build Y2-B to the current policy; PILL1 changes the policy on top of it.`,
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
  notes: `STATUS 2026-09-26: MERGED (PR #187) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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
  notes: `STATUS 2026-09-26: MERGED (PR #189) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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
  notes: `STATUS 2026-09-26: BUILT, PR #191 OPEN and stale (main moved 30+ times under it). Rebase that PR onto main and finish it; do not open a second PR.`,
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
  notes: `STATUS 2026-09-26: MERGED (PR #194) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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
  notes: `STATUS 2026-09-26: GATED (owner decision pending) — unchanged.`,
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
  notes: `STATUS 2026-09-26: BUILT, PR #195 OPEN and stale (main moved 30+ times under it). Rebase that PR onto main and finish it; do not open a second PR.`,
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
    npm run build ; echo "distA=$?"
    cargo build -p yap-polish --release ; echo "sidecarA=$?"
    cargo build -p yap-diarize --release ; echo "diarizeA=$?"
    TRIPLE="$(rustc -vV | awk '/^host:/ {print $2}')" ; mkdir -p src-tauri/binaries && cp ${CARGO_TARGET_A}/release/yap-polish "src-tauri/binaries/yap-polish-$TRIPLE" && cp ${CARGO_TARGET_A}/release/yap-diarize "src-tauri/binaries/yap-diarize-$TRIPLE" ; echo "stageA=$?"
    cd ${WORKDIR}/${APP}/src-tauri && cargo build --release --features custom-protocol ; echo "appA=$?"
and the same for lane B with ${WORKDIR_B} and ${CARGO_TARGET_B}.
STAGING BOTH SIDECARS IS NOT OPTIONAL: bundle.externalBin names yap-polish AND yap-diarize, a precondition of
EVERY cargo build of the app (and desktop/dist must exist or tauri::generate_context! panics), so a missing one fails the build for ENVIRONMENT reasons and a later agent
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
Then, per chain, with that chain's CARGO_TARGET_DIR exported: npm run build (desktop/dist must exist or
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
const laneItems =
  BUILD_LANES === 1
    ? [ITEMS.slice(), []]
    : [ITEMS.filter((it) => laneOf(it) === 0), ITEMS.filter((it) => laneOf(it) === 1)]
const indexOfItem = (item) => ITEMS.indexOf(item)

log(
  `args.lanes = ${BUILD_LANES}${BUILD_LANES === 1 ? ' (ONE builder: lane A runs every item in order, lane B idle)' : ''} || ` +
  `lane A (${WORKDIR}, port ${PREVIEW_PORT}): ${laneItems[0].map((it) => it.id).join(', ') || '(none)'} || ` +
    `lane B (${WORKDIR_B}, port ${PREVIEW_PORT_B}): ${laneItems[1].map((it) => it.id).join(', ') || '(none)'}`
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
  for (const item of laneItems[lane]) {
    if (halted) break
    const at = indexOfItem(item)
    const skipped = onlySkip(item) || panelSkip(item)
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

  return { triage, queued: REVIEW_QUEUE.length, closedByTriage, held: heldByTriage, sweep, mainVerify }
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
  lanes: { A: laneItems[0].map((it) => it.id), B: laneItems[1].map((it) => it.id) },
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
