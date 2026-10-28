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
  name: "yap-overhaul-all-part-02",
  description:
    "Part 02 of the Yap (wilson-voice) overhaul, two or three builder lanes (args.lanes): land whatever is already gate-green, pre-flight on unmodified main (already-done items are skipped), build, open a labelled PR. The adversarial review, the independent second-Opus gate and the merge bar run afterwards in the same script with args {mode:'review'}. 41 items (yap-qa-harness..yap24-OS5), 20 awaiting the Senior Panel. The gate is local (Actions is disabled by the account spending limit); the DMG is not in it.",
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
    { title: "yap-notetaker-onboard", detail: "A \"Set up Notetaker\" card: 2-minute tour, connect Apple Calendar, import from Granola or Otter, MCP shown as Coming — progress dots, dismissible, reopen from Help" },
    { title: "yap-notetaker-detect", detail: "Yap notices a meeting — a calendar event with a call link in the next 2 minutes, or Zoom/Meet/Teams/FaceTime with live audio — and the pill offers \"Record this meeting?\" with the consent line; it never auto-records" },
    { title: "yap-notetaker-notes", detail: "Notes worth reading: the transcript in track turns (You / Call), a light live transcript while recording, search across segments, Markdown export - nothing leaves the Mac. Summary and action items are owned by PR 250 and yap24-NT4, not here." },
    { title: "yap-structure-followups", detail: "The structure stage keeps every word and finishes every list: narration \"Then\"/\"Finally\" survive, four-step lists stay one list, product names keep their casing" },
    { title: "yap-recovery-followups", detail: "Recovered long takes: no duplicate history entries, no race, confirmation enforced in the backend" },
    { title: "yap-land-gate-pass-required", detail: "No PR lands without an Opus PASS bound to its head: one seat for standard, two different seats for security, enforced by scripts/loop/review-gate.mjs in every merge path (LAND, fix-land, land sweeper, drain)" },
    { title: "yap-tests-hermetic-power", detail: "Meeting tests stop reading the host battery: an injected PowerSource, pmset only in production wiring, so cargo test passes on a Mac at 5% on battery" },
    { title: "yap24-PILL1", detail: "The trial is visible from day 14 to day 1: a quiet numeral for the first week, the Y2-B treatment for the last" },
    { title: "yap24-PILL2", detail: "The backend says which stage a take is in, so \"polishing\" and \"pasting\" finally appear on the pill" },
    { title: "yap24-PILL3", detail: "When the hotkey cannot hear you, the pill says so: Secure Input, a disabled tap, and missing Accessibility are pill states" },
    { title: "yap24-PILL4", detail: "Hands-free looks different from hold: a lock mark and a stop hint that names the key the user actually bound, in both characters and all docks" },
    { title: "yap24-NT0", detail: "Provisioning, not product: the pinned parakeet model sits sha256-verified in the lane cache and the meeting eval corpus is materialised at its fixed path" },
    { title: "yap24-NT1", detail: "RESIDUAL: launch recovers a crashed meeting (recover_orphaned_meetings gets its first production caller) and non-complete audio has a 30-day window and a 5 GB ceiling" },
    { title: "yap24-NT2", detail: "RESIDUAL: the shipped meeting pipeline gets its guards: live recording never counts toward max_yield, a crash-loop guard, waiting_for_model, asr_model on the row, and a REQUIRED WER/RTF run" },
    { title: "yap24-NT3", detail: "Silence is not a denial: an in-person meeting never attaches the system tap, and a quiet call is never told macOS refused it" },
    { title: "yap24-NT4", detail: "RESCOPED after #250: the default notes are MEASURED (action-item recall and precision at least 0.8 on a planted-commitment fixture) and installing the notes model leaves polish_model byte-unchanged" },
    { title: "yap24-NT5", detail: "Who said what: in-person meetings run the shipped diarization sidecar after transcription, with the honest accuracy framing" },
    { title: "yap24-NT6", detail: "The Meetings view tells the truth about every meeting: progress, notes, failure with Retry, audio kept or expired" },
    { title: "yap24-NT7", detail: "The pill and the menu bar follow the meeting after stop: transcribing 42%, notes ready, or what went wrong" },
    { title: "yap24-NT9", detail: "Phase-closing proof: one headless command runs a two-track fixture through capture, transcript, notes and export" },
    { title: "yap24-NT10", detail: "A meeting survives a lid close or sleep: IOKit holds the sleep, the journal finalizes, and the row ends partial with one plain sentence instead of staying recording forever" },
    { title: "yap24-NT11", detail: "A call on built-in speakers is transcribed once: an AEC3 pre-pass cancels the far side out of the mic track (derived file, raw WAV untouched), with a per-span Speaker label and a 3-word residue rule as the fallback" },
    { title: "yap24-OS1", detail: "The first word is never lost and the start is 280 ms sooner: capture arms on key-down and a tap throws the pre-roll away" },
    { title: "yap24-OS2", detail: "Launch at login through SMAppService.mainApp, the way macOS 13+ expects, instead of a LaunchAgent plist" },
    { title: "yap24-OS3", detail: "Permission truth: the hotkey asks for Input Monitoring by name, the purpose strings describe today's app, and a grant is noticed on focus" },
    { title: "yap24-OS4", detail: "Hardened-runtime diet: prove whether allow-jit and allow-unsigned-executable-memory are needed, and drop what is not" },
    { title: "yap24-OS5", detail: "Idle means idle: the fixed polls (Secure Input 2 s, pill space-keeper, permission watch) are consolidated and measured" },
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
const LOG = '/Users/wilsonguenther/Obsidian/Wilson-Brain/Projects/Loop-Logs/2026-09-12-yap-part02.md'
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
const PART = 'part-02'
/**
 * Stamped by build.mjs: item id -> the item FILE's ordinal (informational) or a pinned lane letter
 * ('A'|'B'|'C', item field `lane`). Only the PIN is read: since 2026-10-27 lanes are assigned PER PART
 * at run time (splitLanes below) — the old per-file round-robin left lane C idle for a whole part.
 */
const LANE_BY_ID = {"yap-qa-harness":5,"yap-perm-truth":5,"yap-hotkey-stop":5,"yap-format-structure":5,"yap-meetings-fix":5,"yap-uiux-2026-10-16-W0":5,"yap-uiux-2026-10-16-HOME":5,"yap-uiux-2026-10-16-DICT":5,"yap-uiux-2026-10-16-INSIGHTS":5,"yap-uiux-2026-10-16-MEETINGS":5,"yap-uiux-2026-10-16-SCRATCH":5,"yap-uiux-2026-10-16-SETTINGS":5,"yap-uiux-2026-10-16-ONBOARD":5,"yap-uiux-2026-10-16-Z":5,"yap-notetaker-onboard":5,"yap-notetaker-detect":5,"yap-notetaker-notes":5,"yap-structure-followups":5,"yap-recovery-followups":5,"yap-land-gate-pass-required":5,"yap-tests-hermetic-power":5,"yap24-PILL1":6,"yap24-PILL2":6,"yap24-PILL3":6,"yap24-PILL4":6,"yap24-NT0":7,"yap24-NT1":7,"yap24-NT2":7,"yap24-NT3":7,"yap24-NT4":7,"yap24-NT5":7,"yap24-NT6":7,"yap24-NT7":7,"yap24-NT9":7,"yap24-NT10":7,"yap24-NT11":7,"yap24-OS1":8,"yap24-OS2":8,"yap24-OS3":8,"yap24-OS4":8,"yap24-OS5":8}
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
// ── 00e-yap-uiux-2026-10-16.mjs ───────────────────────────────────────────
// yap-perm-hotkey-2026-10-16 — THE BANNER WAS RIGHT, SETTINGS LIED; THE HOTKEY STARTED A TAKE IT
// COULD NOT STOP. Written 2026-10-16 by the yap-perm-audit seat from Wilson's 0.9.0 report (16:14Z):
// System Settings shows Yap ON in Accessibility while Yap's home says "Accessibility and Input
// Monitoring are off"; and a hotkey take that 15 fn presses would not end. He asked for tools that
// test it for real. FIX, no panel (unambiguous intent). These three items sit at the TOP of
// 00e-yap-uiux-2026-10-16.mjs: one file = one lane, run in order, so all three run ahead of
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
// ORDER: this block sits in 00e-yap-uiux-2026-10-16.mjs after yap-hotkey-stop and BEFORE W0 (one file =
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
  title: 'A "Set up Notetaker" card: 2-minute tour, connect Apple Calendar, import from Granola or Otter, MCP shown as Coming — progress dots, dismissible, reopen from Help',
  preflight: `
    test -f desktop/src/meetings/NotetakerSetupCard.tsx
    test -f desktop/src-tauri/src/meeting_import.rs
  `,
  spec: `
    Panel: not required — Wilson named the four steps from Wispr's card (2026-10-16).
    DEPENDS: yap-meetings-fix (a real transcript must exist for the tour, import and MCP to mean anything).

    THE CARD: top of the Meetings view until done or dismissed; three working rows (tour, calendar, import) plus a fourth Set up MCP row shown as Coming with no action, one progress dot each (empty /
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
      4. [STRUCK 2026-10-26: MCP is DEFERRED to after the first 100 users; the row shows "Coming" and nothing is built. The yap-mcp binary, the rmcp crate, the mcp_permissions_v1 table and the Copy config button are all out of scope. The old step is in git history. yap24-X2 was cut as a duplicate of this step, so this paragraph is the single record of the MCP deliverable.]

    RUNTIME DEPENDENCIES: | EventKit | OS | — nothing found on the machine. [yap-mcp and rmcp STRUCK 2026-10-26.]

    TESTS: vitest NotetakerSetupCard (4 rows, dots, dismiss persists, Help reopens); cargo test
    meeting_import (Otter txt/srt fixtures → segments; malformed file → a plain error, no partial row);
    [cargo test -p yap-mcp STRUCK 2026-10-26]  tests/calendar_permission_readonly.rs (no EKEventStore save/remove call anywhere: grep gate).

    WHAT NOT TO DO: no OAuth, no network in import or MCP, no write access over MCP, no new mascot.
    Critiqued once (self-pass): the draft exposed MCP by default; Wispr's "account-wide, no per-meeting
    consent" is exactly what we are not copying, so every class now starts OFF.

    Panel revisions 2026-10-26 (Senior Panel plan-audit; ONE OWNER PER DELIVERABLE; strikes applied in place above and listed here):
      - STRUCK: step 4 (Set up MCP), the yap-mcp / rmcp runtime dependencies, the cargo test -p yap-mcp test, and the yap-mcp preflight/acceptance lines. The
        acceptance now FAILS if desktop/yap-mcp exists, so an agent cannot rebuild it by accident. MCP: one owner (this paragraph), deferred.
      - CALENDARS PERMISSION is requested ONLY by an explicit click on the card's Connect calendar button: never at launch, never by default, never by
        another item. Wilson named that step himself from Wispr's card (2026-10-16), which is the explicit yes the deferred NT8 asked for. Calendar
        reading is opt-in for the whole chain.
  `,
  acceptance: `
    test -f desktop/src/meetings/NotetakerSetupCard.tsx
    test -f desktop/src-tauri/src/meeting_import.rs
    ! test -d desktop/yap-mcp
    grep -q "NSCalendarsFullAccessUsageDescription" desktop/src-tauri/Info.plist
    ! grep -rnE 'saveEvent|removeEvent|save[(]event|remove[(]event' desktop/src-tauri/src
    grep -q "Set up Notetaker" desktop/src/meetings/NotetakerSetupCard.tsx
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
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
      b. STRUCK 2026-10-26 (Senior Panel, calls + BYOM): signal (b) is struck. meeting_detect.rs ships calendar + offer only; the
         app/browser signals (per-process mic-in-use attribution and browser window-title matching) are owned by yap-call-0 and
         yap-call-2, which call into this file's offer. Do not build UI-pid attribution or Meet AX title matching here.
    OFFER: the pill (locked pixel-art mascot) expands with "Record this meeting?" + the meeting title when
    known + CONSENT_PILL_LINE ("Let them know you're recording.", exported from desktop/src/meetings/consent.ts — the three-paragraph
    CONSENT_NOTICE is too long for a pill, and its "Nothing is uploaded" claim stops being true once any Leaves-this-Mac label is Yes,
    which yap-call-1 and yap-byom-0 handle) + [Record] [Not now]. Record starts kind = Virtual. Not now snoozes that
    event / app session (no repeat for the same calendar event or the same app run). Never starts recording
    on its own; there is no auto-start setting. Settings › Meetings: "Offer to record detected meetings" (default ON; this item creates the toggle, yap-call-0 reads it for the app signal and turns its timer off when it is off; it needs no permission) and "Use my calendar" (default OFF; it turns on only from the setup card's Connect calendar click [STRUCK 2026-10-26: any default-ON calendar behaviour]). Polling: event-driven
    (NSWorkspace launch/terminate + an EventKit change notification + a single 30 s timer) — never faster.

    TESTS: tests/calendar_prompt_policy.rs (table: link+2 min → offer; no link → none; snoozed → none; in
    progress 4 min → offer, 6 min → none); vitest for the pill offer (CONSENT_PILL_LINE rendered; Not now emits snooze).

    RUNTIME DEPENDENCIES: | EventKit, NSWorkspace, CoreAudio process objects | OS |.
    WHAT NOT TO DO: no auto-record, no network, no reading meeting content to decide, no polling < 30 s.
    Critiqued once (self-pass): "Zoom is running" alone would prompt all day for people who leave Zoom open;
    the app signal now requires live audio from that app.

    Panel revisions 2026-10-26 (Senior Panel plan-audit; ONE OWNER PER DELIVERABLE; strikes applied in place above and listed here):
      - OWNS what the cut yap24-NT8 would have built (a calendar-aware Record prompt, tests/calendar_prompt_policy.rs). It is now the SINGLE owner.
      - STRUCK: any default-ON calendar behaviour. The calendar signal (a) runs only after the user has connected the calendar in the setup card; the
        app signal (b) needs no TCC permission and keeps its default. Nothing here ever requests the Calendars permission itself.
      - STRUCK (calls + BYOM panel, 2026-10-26): signal (b), the app/browser audio signal. It is OWNED by yap-call-0 (per-process mic-in-use + helper attribution; UI-pid attribution never produces audio for a browser) and yap-call-2 (browser title). The grep "Recording is sacred" (absent from main) is replaced by CONSENT_PILL_LINE gates; consent.ts:68's "Nothing is uploaded" is made conditional by yap-call-1.
  `,
  acceptance: `
    test -f desktop/src-tauri/src/meeting_detect.rs
    test -f desktop/src-tauri/tests/calendar_prompt_policy.rs
    grep -q 'export const CONSENT_PILL_LINE' desktop/src/meetings/consent.ts
    grep -rq 'CONSENT_PILL_LINE' desktop/src/pill
    grep -rq 'CONSENT_PILL_LINE' --include='*.test.ts' --include='*.test.tsx' desktop/src
    ! grep -rn "auto_record\|autoRecord" desktop/src desktop/src-tauri/src
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test calendar_prompt_policy ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol meeting_detect ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'yap-notetaker-notes', prompt: 'yap-notetaker', branch: 'loop/yap-notetaker-notes-speakers-summary-actions', gated: null,
  title: 'Notes worth reading: the transcript in track turns (You / Call), a light live transcript while recording, search across segments, Markdown export - nothing leaves the Mac. Summary and action items are owned by PR 250 and yap24-NT4, not here.',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/summary_jobs.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 250 (default meeting notes) has not landed; land it, then build this"; exit 0; fi; test -f desktop/src-tauri/tests/meeting_notes_stay_local.rs && test -f desktop/src/meetings/LiveTranscript.tsx )
  `,
  spec: `
    Panel: not required — Wilson 2026-10-16 (Wispr parity). Wispr: live transcript "separates you from other
    speakers"; summary pulls out "timelines, decisions, and next steps"; labels "not voiceprints".
    DEPENDS: yap-meetings-fix (segments exist), PR 250 (default notes, summary_jobs.rs). [yap-mcp dependency STRUCK 2026-10-26.]

      1. Speaker turns: two-track meetings label track 0 "You" and track 1 "Call" (TrackSpeakers); [STRUCK 2026-10-26: the diarization sentence (in-person and multi-voice track 1 running the diarize sidecar, "Speaker 1/2", rename, calendar attendees as rename choices, per-meeting embeddings). diarize::pool() keeps its single owner, the DEFERRED yap24-NT5, so no speaker clustering is wired here.] In-person meetings show the single mic track with no speaker labels.
      2. Live transcript (light): while recording, a rolling decode every ~10 s on each track shows the last
         lines under You / Call in the Meetings view. The final transcript replaces it after Stop.
      3. [STRUCK 2026-10-26: summary + action items + the meeting_actions table are OWNED by PR 250 (default 1.5B notes, Markdown sections in meetings.summary, summary_jobs.rs) and yap24-NT4 (the measured recall eval). This item builds none of it and must not create a meeting_actions table.]
      4. Search: FTS over segments + summaries + action items, from the Meetings search box.
      5. Export: Markdown with speakers, timestamps, summary, action items as "- [ ]" (export_meeting_markdown).
      6. [STRUCK 2026-10-26: MCP is deferred to after the first 100 users; see yap-notetaker-onboard.]
    PRIVACY TEST: tests/meeting_notes_stay_local.rs — with a local model, the summarize path makes zero
    network calls (a deny-all HTTP client seam that panics on use).

    RUNTIME DEPENDENCIES: none beyond what the app already ships. [diarize sidecar, summary model and cloud model rows STRUCK 2026-10-26.]
    TESTS: vitest for the Meetings detail (track turns, live transcript). [meeting_notes_auto_summary.rs, the diarize wiring test, rename and the action item checkbox STRUCK 2026-10-26.]
    WHAT NOT TO DO: no upload of audio or transcript by default; no persistent voiceprints; no web search.
    Critiqued once (self-pass): live transcript on a 3B-param budget would starve dictation; it is capped to
    a light rolling decode that yields to any dictation take.

    Panel revisions 2026-10-26 (Senior Panel plan-audit; ONE OWNER PER DELIVERABLE; strikes applied in place above and listed here):
      - STRUCK: the diarization sentence of step 1 (diarize::pool() wiring, Speaker 1/2, rename, calendar attendees as rename choices), all of step 3
        (summary + action items + the meeting_actions table), step 6 (MCP), the diarize / summary-model / cloud-model runtime rows, the
        auto-summary and diarize tests, and the preflight/acceptance lines that required meeting_actions, meeting_notes_auto_summary.rs, diarize::pool()
        and yap-mcp. The acceptance now FAILS if meeting_actions, a diarize::pool() caller or desktop/yap-mcp appears.
      - KEPT and still buildable: You / Call track turns, the light live transcript (a rolling decode that yields to any dictation take), search over
        segments, Markdown export (extending PR 250's export, not a second one), and the zero-network privacy test.
      - Owners now: summary + action items = PR 250 and yap24-NT4; diarization = yap24-NT5 (deferred); calendar prompt = yap-notetaker-detect; MCP =
        deferred (recorded once, in yap-notetaker-onboard).
    Audit 2026-10-26: track turns cannot be shown while a stored segment is a whole 30 s window (avg 29.4-30.1 s on every real meeting). Utterance splitting is NEW yap-audit-utterance-segments (panel), and VAD cuts are NEW yap-audit-vad-boundaries. The transcript also has no find-in-meeting and no copy (TranscriptList.tsx:44-55). See docs/audits/meeting-notetaker-audit-2026-10-26.md issues #3, #4, #12.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/meeting_notes_stay_local.rs
    test -f desktop/src/meetings/LiveTranscript.tsx
    ! grep -rn 'meeting_actions' desktop/src-tauri/src
    ! (grep -rln "diarize::pool()" desktop/src-tauri/src | grep -vE "diarize\\.rs|meeting_matrix\\.rs" | grep -q .)
    ! test -d desktop/yap-mcp
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_notes_stay_local ; test $? -eq 0
  `,
})

// yap-rereview-followups-2026-10-16 — the advisories the #216/#217 re-reviews raised and did not block on.
// Written 2026-10-16 by the items-followups seat. Review: Opus only (review-opus + review-opus2).
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap-structure-followups', prompt: 'yap-format-structure', branch: 'loop/yap-structure-followups-keep-every-word-finish-every-list', gated: null, builder: 'sonnet',
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

// ── 00f-yap24-pill-and-trial.mjs ──────────────────────────────────────────
// yap24-PILL — THE PILL SAYS WHAT IS HAPPENING. Written 2026-09-26 by the architecture audit
// (docs/ARCHITECTURE-AUDIT-2026-09-26.md §3). Wilson, 2026-09-26: "the pill does not indicate
// when [state] … 14 days".
//
// What the pill fails to indicate on main today, each verified from code:
//   1. The 14-day trial for its first week: pillLicense() is SILENT while more than 7 days remain
//      (desktop/src/pill/license.ts:136-165, "trial, more than 7 days -> nothing"), and the numeral
//      itself is not drawn on main — Y2-B is still an open PR (#186).
//   2. Polishing and pasting: both are declared phases (pill/live.ts:411, :413) but no event
//      produces them — reduceTakePhase (live.ts:1152-1196) only preserves them. After decode the
//      pill shows "thinking" labelled "Transcribing" through the LLM polish and the paste.
//   3. A blind hotkey: Secure Input (secure_input.rs:48, 2 s poll; >100 "Secure Input ENABLED"
//      lines in Wilson's yap.log, from loginwindow and Chrome) and a disabled event tap (Y1-A tap
//      health) are on the status payload (lib.rs:893-922) but float-main.tsx:60-65 reads only
//      recording / busy / engine_loading / last_error. The user presses fn and nothing happens.
//   4. Hands-free vs hold: status carries hands_free (lib.rs:896); the pill draws the same
//      "listening" for both, so a double-tapped take (ptt_macos.rs:34, 450 ms) looks like a hold.
//
// This file is ONE lane. Pass 2 runs it beside 03-yap24-hotkeys-permissions-os.mjs:
//   args: {mode:'build', only:['yap24-PILL','yap24-OS'], panelApproved:[...]}
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs. Headless only.

ITEMS.push({
  id: 'yap24-PILL1', prompt: 'yap24-PILL', branch: 'loop/yap24-pill1-trial-visible-for-the-whole-trial', gated: 'panel',
  title: 'The trial is visible from day 14 to day 1: a quiet numeral for the first week, the Y2-B treatment for the last',
  preflight: `
    ( if ! ( test -f scripts/pill-matrix.mjs && grep -q 'pub const PURCHASE_URL' desktop/src-tauri/src/license.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap24-UI3 (scripts/pill-matrix.mjs) has not landed; build and land UI3 first"; exit 0; fi; grep -q "14d" desktop/src/pill/license.test.ts && grep -q 'purchase_open' desktop/src/pill/license.ts && grep -q 'PILL_QUIET_FROM_DAYS' desktop/src-tauri/src/license.rs )
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26 (product copy/threshold — the panel may keep a quiet style, it may not keep silence)
    DEPENDS: [UPDATED 2026-10-26: #186 is CLOSED, not open; cherry-pick branch loop/y2-b-pill-trial-countdown-in-both-styles at 8d85cdf, see the last paragraph; the Buy path depends on yap-lic-04 (PR #232 lands that item) and the matrix on yap24-UI3] Y2-B (formerly open PR #186 — the numeral in both pill styles and the 30px side dock).
      Y2-B lives in 20-y2 on the SAME lane and runs AFTER this file, so do not wait for it: if #186
      is still open when this item starts, carry its commits onto this branch (rebase onto main,
      keep its tests), then close #186 with a "superseded by #<this PR>" comment naming the commits.
      Do not redraw the numeral from scratch. Y2-B's own pre-flight then retires it.

    EVIDENCE
      - pill/license.ts:136-165 display policy: "trial, more than 7 days -> nothing (ambient
        silence)". For half the trial the pill says nothing; Wilson's report is exactly this.

    Panel revisions 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, MEDIUM + HIGH, GROUNDED):
      - Introduce a SEPARATE shared constant for the new "quiet" threshold (e.g.
        PILL_QUIET_FROM_DAYS = 14) in BOTH license.rs and license.ts. Do NOT repurpose
        PILL_SHOW_DAYS = 7 — it is documented as shared with the Rust tray's own purchase-offer
        threshold (license.ts:52's comment, license.rs:680, tray_offers_purchase at :760) and an
        existing test pins it to 7 (license.test.ts:98); changing it would silently move when the
        tray offers to sell, not just when the pill goes quiet. Add a twin-constant equality test
        and cargo test --lib license to this item's acceptance (today it runs vitest only).
      - Do NOT let a working countdown promise a purchase that cannot happen: as of this panel,
        the Stripe payment link is inactive and the license issuer (Forge) does not answer, so
        this item's countdown/urgent copy must say plainly that purchasing is not open yet
        instead of showing a Buy action that dead-ends — coordinate with LIC-A's Panel revision
        in 20-y2-trial-and-limits.mjs, which is the item that actually restores a working
        purchase path (via the Drivia Consulting app, per the product ledger).
      - The trial is 14 days (license.rs; memory project_yap_build_state YP2 "14-day full trial").
      - Y2-B (#186) draws the numeral but was built against the 7-day policy and was never
        visually QA'd (STATUS-yap.md row Y2-B).

    DO
      1. Policy: trial 14..8 -> show, tone "trial-quiet" (the numeral at reduced contrast, no
         hourglass animation); 7..1 -> the existing "trial"; last day -> "urgent"; licensed ->
         nothing (unchanged); problem / ended unchanged.
      2. The numeral is the days remaining the backend computed (days_left, rollback floor
         included) — never recomputed in the webview.
      3. Hover/accessible title: "Free trial — N days left". Never a price on the pill (Y2-D rule).
      4. Both characters, all three docks, through the shell.

    Tests: license.test.ts — every day 14..0, licensed, problem, ended; a snapshot of the
    accessible title per tone.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - THE USER SEAT FOUND A LIE: on day 1 of an "urgent" trial the user taps Buy and gets an INACTIVE Stripe link on the retired account
        (license.rs:170-176 PAYMENT_LINK_URL). No item owned a working Buy path. The ledger fixes the checkout CLOSED until launch day.
      - CHANGE 1, the Buy chip is truthful: it renders ONLY when the license status reports purchase_open = true, sourced from one Rust
        const PURCHASE_OPEN (default false, surfaced on the license status payload and read in pill/license.ts). While false the pill and
        the countdown copy carry no purchase action at all (trial numeral, "Free trial - N days left", last-day "Trial ends today"). Wilson
        flips PURCHASE_OPEN in the same release that opens checkout. Depends on yap-lic-04 landing (PURCHASE_URL replaces PAYMENT_LINK_URL;
        PR #232 carries those items), so the preflight holds this item until license.rs has pub const PURCHASE_URL. Note yap-lic-04 shows
        no separate "closed" state in its LICENSE PANEL (the web page says when purchases are closed); this gate is for the PILL chip only and
        does not add a closed state anywhere else.
      - CHANGE 2, no redraw: carry the numeral work from branch loop/y2-b-pill-trial-countdown-in-both-styles at 8d85cdf (cherry-pick its numeral
        commits, keep its tests). The "if #186 is still open" path above is dead; #186 is closed.
      - CHANGE 3, the missing gate: the earlier revision promised cargo test --lib license and a twin-constant equality test; the acceptance now
        runs it and greps PILL_QUIET_FROM_DAYS in BOTH license.rs and license.ts. PILL_SHOW_DAYS (license.rs:867, license.ts:59) is NOT repurposed.
      - CHANGE 4, someone could notice it: the acceptance runs node scripts/pill-matrix.mjs --only trial-quiet (6 PNGs: 3 docks x 2 characters)
        once yap24-UI3 has landed; attach the PNGs to the PR, never commit them.
      - Founding-price copy: the FOUNDING19 coupon expires 2026-11-08 (license.rs:168). No founding-price string ships on the pill (Y2-D rule: never
        a price on the pill); the date question belongs to Wilson, not this item.
  `,
  acceptance: `
    grep -q 'purchase_open' desktop/src/pill/license.ts
    grep -q 'PILL_QUIET_FROM_DAYS' desktop/src/pill/license.ts
    grep -q 'PILL_QUIET_FROM_DAYS' desktop/src-tauri/src/license.rs
    grep -q 'pub const PURCHASE_OPEN' desktop/src-tauri/src/license.rs
    O="$(mktemp -d)" && node scripts/pill-matrix.mjs --only trial-quiet --out "$O" && test "$(ls "$O"/*.png | wc -l)" -ge 6
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/pill/license.test.ts
    npm test
    npm run build
    cd src-tauri && cargo test --features custom-protocol --lib license
  `,
})

ITEMS.push({
  id: 'yap24-PILL2', prompt: 'yap24-PILL', branch: 'loop/yap24-pill2-backend-stage-events-for-polish-and-paste', gated: 'panel',
  title: 'The backend says which stage a take is in, so "polishing" and "pasting" finally appear on the pill',
  preflight: `
    ( if ! ( test -f scripts/pill-matrix.mjs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap24-UI3 (scripts/pill-matrix.mjs) has not landed; build and land UI3 first"; exit 0; fi; grep -q "TAKE_STAGE_EVENT" desktop/src-tauri/src/lib.rs && grep -q "take_stage" desktop/src/float-main.tsx && grep -q 'insert-copy-card' desktop/src/pill/live.ts )
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26.
    DEPENDS: Y5-C (merged #193 — the phase machine this feeds)

    EVIDENCE
      - pill/live.ts:411 "polishing" and :413 "pasting" are declared, owned by Y5-C / Y7-D, and
        never produced: reduceTakePhase (live.ts:1152-1196) has no event that yields either.
      - The backend emits recording, status, transcript, transcribe_progress, audio_level,
        license, license_required and a few UI events — no stage event (grep of .emit in
        src-tauri/src, 2026-09-26).
      - Result: the LLM polish (up to the Y4-E chunked deadline) and the paste receipt wait both
        read as "Transcribing".

    DO
      1. One event, TAKE_STAGE_EVENT = "take_stage", payload {stage, takeId, words?}, emitted at
         the real boundaries in the dictation pipeline: decode start, polish start (only when the
         polish stage actually runs), paste start, paste confirmed / not confirmed, done, empty,
         error. One emit helper; no stage is emitted from two places.
      2. reduceTakePhase gains {type:'stage'}; stage outranks the inferred status path, and the
         existing hold/timeout policy (PHASE_HOLD_MS) still bounds every working phase.
      3. The paste-not-confirmed receipt ("no app read the clipboard within 1500ms", seen in
         Wilson's log) becomes a visible one-line state, not only a log line.

    Tests: Rust — the dictation pipeline test harness asserts the stage sequence for
    polish-on and polish-off takes; TS — live.test.ts stage sequences incl. out-of-order status.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - HAND OFF, do not build a second surface: DO step 3 ("paste not confirmed becomes a visible one-line state") is VOID as a pill state. The ungated
        item yap-insert-bridge-2026-10-26 (00a) owns paste, the Accessibility probe and the Copy card (INSERT_COPY_CARD_EVENT "insert-copy-card"
        in paste.rs). A not-confirmed paste emits its stage and the pill hands off to that Copy card; the pill draws no extra sentence. The
        stage boundaries around paste live in paste.rs's single insertion chooser (choose_insert_path), so emit stage events from that one
        boundary and rebase after the insert-bridge PR; do not add a second emit site.
      - THE JOB THAT REMAINS: live.ts:411 "polishing" and :413 "pasting" are declared and never produced (reduceTakePhase, live.ts:1152-1196, has no
        event that yields them). Produce them from take_stage. The Rust test asserts the stage sequence for polish-on and polish-off takes and
        the acceptance now requires at least one test to RUN (grep for "test result: ok. N passed" with N >= 1); the old form could pass on
        an unmatched filter.
      - Someone could notice it: node scripts/pill-matrix.mjs --only polishing and --only pasting (6 PNGs each), PNGs attached to the PR.
  `,
  acceptance: `
    grep -q "TAKE_STAGE_EVENT" desktop/src-tauri/src/lib.rs
    grep -q 'insert-copy-card' desktop/src/pill/live.ts
    O="$(mktemp -d)" && node scripts/pill-matrix.mjs --only polishing --out "$O" && test "$(ls "$O"/*.png | wc -l)" -ge 6
    O="$(mktemp -d)" && node scripts/pill-matrix.mjs --only pasting --out "$O" && test "$(ls "$O"/*.png | wc -l)" -ge 6
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/pill
    npm run build
    cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib take_stage 2>&1 | grep -qE 'test result: ok\\. [1-9][0-9]* passed'
    cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-PILL3', prompt: 'yap24-PILL', branch: 'loop/yap24-pill3-blind-hotkey-and-blind-paste-states', gated: 'panel',
  title: 'When the hotkey cannot hear you, the pill says so: Secure Input, a disabled tap, and missing Accessibility are pill states',
  preflight: `
    ( if ! ( test -f scripts/pill-matrix.mjs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap24-UI3 (scripts/pill-matrix.mjs) has not landed; build and land UI3 first"; exit 0; fi; grep -q "secure_input" desktop/src/float-main.tsx && grep -q '"blind"' desktop/src/pill/live.ts && grep -q 'still works' desktop/src/pill/live.ts )
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26.
    DEPENDS: PERM-C (merged #161), Y1-A (merged #162)

    EVIDENCE
      - lib.rs:893-922 build_status already computes secure.blocked, the tap health message and
        accessibility. float-main.tsx:60-65 BackendStatus declares only recording, busy,
        engine_loading, last_error.

    Panel revisions 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, HIGH, GROUNDED):
      - Do NOT move the Secure Input check to key-down + a backstop (see the REJECTED plan noted
        in yap24-OS5's Panel revision) — the tap is blind under Secure Input and never sees that
        key-down. Keep secure_input.rs's 2 s poll as the source for this pill state.
      - Add a FOURTH blind-hotkey cause this item's original three (Secure Input, tap disabled,
        Accessibility missing) do not cover: the hotkey tap was never created because Input
        Monitoring was missing at launch (ptt_macos.rs logs this and returns without ever
        retrying). Wire OS3's grant-flip detection to call ptt_macos::start again, and give this
        pill state a distinct cause/copy ("hotkey not installed") with the Privacy_ListenEvent
        deep link, not just "disabled."
      - Add a dwell rule before showing "blind": ignore loginwindow as an owner of Secure Input
        (Wilson's log shows it firing most often at the lock screen, when the pill is not visible
        anyway) and require >=1 s of continuous Secure Input before the state appears, clearing
        immediately once it is gone — otherwise a brief Chrome password-field focus flickers the
        pill. Acceptance gains a no-flicker case (secure input for 500 ms -> no blind state) and
        keeps the existing <=2.1 s appearance bound.
      - Wilson's logs: well over 100 WARN lines "Secure Input ENABLED by loginwindow / Google
        Chrome — the fn PTT event tap is blind" across yap.log, yap.log.1 and yap.log.2.
      - The live gate reducer (live.ts reduceGatePhase) handles mic permission, recording and
        cancel only.

    DO
      1. A gate phase "blind" with three causes, each one sentence and one action:
         Secure Input on (name the owning app, which secure_input.rs already resolves);
         event tap disabled by macOS (re-armed per Y1-A — say so while it is down);
         Accessibility missing (paste will copy to the clipboard only — say that, deep link).
      2. It outranks idle/sleepy but not a take in progress; it clears the moment status clears.
      3. The pill stays click-through except the action chip.

    Tests: live.test.ts — status payloads -> phase for each cause, precedence against a take.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - PREDATES #249. hotkeys.rs:14-17: the Carbon chord (default Control-Option-Command-Space) needs no Accessibility, no Input Monitoring and is delivered
        under Secure Input; attempt_tap re-arms on every permission edge (:19-20). So "blind" is now FN ONLY: only the fn key is dead under Secure Input.
      - COPY (exact): "fn is blocked by <app>'s secure field - <chord_label> still works", where <chord_label> is hotkeys.rs::chord_label (:496) for the
        user's CURRENT chord (so a rebound chord reads truthfully) and <app> is the owner secure_input.rs already resolves. The old claim that the
        hotkey is dead would be a lie for anyone with the chord on. The acceptance greps "still works" in live.ts, and live.test.ts gains a
        render case asserting the chord hint appears.
      - DROPPED: the "hotkey not installed" cause (done by #249's re-arm retry) and the Accessibility cause (belongs to yap-insert-bridge, which owns
        the Copy card). Keep the dwell rule (ignore loginwindow, at least 1 s continuous Secure Input, 500 ms never flickers, clears at once) and
        the 2 s poll (see OS5: exempt by name).
      - Someone could notice it: node scripts/pill-matrix.mjs --only blind, PNGs attached to the PR.
  `,
  acceptance: `
    grep -q 'still works' desktop/src/pill/live.ts
    O="$(mktemp -d)" && node scripts/pill-matrix.mjs --only blind --out "$O" && test "$(ls "$O"/*.png | wc -l)" -ge 6
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/pill
    npm test
    npm run build
  `,
})

ITEMS.push({
  id: 'yap24-PILL4', prompt: 'yap24-PILL', branch: 'loop/yap24-pill4-hands-free-looks-different-from-hold', gated: 'panel',
  title: 'Hands-free looks different from hold: a lock mark and a stop hint that names the key the user actually bound, in both characters and all docks',
  preflight: `
    ( if ! ( test -f scripts/pill-matrix.mjs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap24-UI3 (scripts/pill-matrix.mjs) has not landed; build and land UI3 first"; exit 0; fi; grep -q "hands_free" desktop/src/float-main.tsx && grep -q "hands-free" desktop/src/pill/live.test.ts && grep -qE 'chord_label|chordLabel' desktop/src/pill/live.ts )
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26.
    DEPENDS: yap24-PILL2 (same reducer), Y5-K when merged (characters are data)

    EVIDENCE
      - lib.rs:896 status carries hands_free; the pill never reads it (float-main.tsx:60-65).
      - ptt_macos.rs:34 DOUBLE_TAP_MS = 450, :550 double-tap -> hands-free ON; Wilson's log shows
        dozens of hands-free takes, some several minutes long, with the same pill as a hold.

    DO: a listening variant "listening-locked" (phaseVisual label "Hands-free — tap fn⌃ to stop"),
    drawn by the shell as a small lock chip; the live commentary keeps running; Escape/cancel copy
    unchanged. Tests in live.test.ts.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - The hardcoded "Hands-free - tap fn-Control to stop" is WRONG now that chords are rebindable (#249). The label is built from the live binding:
        "Hands-free - " + chord_label (hotkeys.rs:496, carried on the status payload) + " to stop". A test in live.test.ts renders it with a REBOUND
        key and asserts the rebound label appears and the old literal does not; the acceptance fails if the string "tap fn" survives in non-test pill source.
      - Someone could notice it: node scripts/pill-matrix.mjs --only listening-locked, PNGs attached to the PR.
  `,
  acceptance: `
    grep -qE 'chord_label|chordLabel' desktop/src/pill/live.ts
    ! grep -rn --include='*.ts' --include='*.tsx' --exclude='*.test.ts' --exclude='*.test.tsx' 'tap fn' desktop/src/pill
    O="$(mktemp -d)" && node scripts/pill-matrix.mjs --only listening-locked --out "$O" && test "$(ls "$O"/*.png | wc -l)" -ge 6
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/pill
    npm run build
  `,
})

// ── 01-yap24-notetaker.mjs ────────────────────────────────────────────────
// yap24-NT — THE NOTETAKER CHAIN. Written 2026-09-26 by the architecture audit
// (docs/ARCHITECTURE-AUDIT-2026-09-26.md §2). Wilson, dictated 2026-09-26: "The notetaker is not
// even working and that's one of the most important things — every component of that."
//
// ROOT CAUSE, verified from code AND from Wilson's own install (read-only copy of the SQLite DB):
//   * Recording works: a stop writes the mic WAV and sets the row to `transcribing`
//     (desktop/src-tauri/src/meeting_control.rs:652-656, :710-712).
//   * NOTHING EVER TRANSCRIBES IT. The meeting ASR job (meeting_asr.rs:1718 `MeetingAsr`, :1767
//     `run`, :614 `WarmEngineChunkAsr`) is constructed ONLY in two integration tests
//     (tests/meeting_dictation_preempts_transcription.rs:145, tests/matrix_new_asr_chunk_timeout.rs:108);
//     the module opens with `#![allow(dead_code)]` (meeting_asr.rs:52), which is how the compiler was
//     kept quiet about it. `Database::append_meeting_segments` (db.rs:1652) has no caller in src/.
//     tests/meeting_manual_start_stop.rs:158-162 asserts `transcribing` and stops — "(YV93 finishes
//     it)" — so 105 lib meeting tests + 30 integration tests are green around a dead pipeline.
//   * The summary command then refuses: "that meeting has no transcript to summarize" (lib.rs:4216-4218).
//   * Diarization is never invoked: `diarize::pool()` has no caller (meeting_matrix.rs:143).
//   * Then the retention sweep deletes the audio anyway: `purge_meeting_audio` (db.rs:1936-1972)
//     selects on `started_at` alone, run every hygiene pass (lib.rs:2966-3001), 7 days
//     (meetings.rs:73). Wilson's DB: 4/4 meetings stuck in `transcribing`, processed_through 0.0,
//     audio_kept 0, both WAV paths NULL, meetings/ dir 0 B. The audio is gone.
//   * And the system-audio verdict marks silence as denial: 3/4 of those rows (one of them kind
//     `in_person`) carry LOOKS_DENIED_MESSAGE (syscapture.rs:2324-2327) because nothing played for
//     DENIAL_GRACE = 3 s (syscapture.rs:2243, :2334-2341).
//
// ORDER IS THE DEPENDENCY CHAIN: capture durability -> transcript -> notes -> UI. This file is ONE
// lane, sequential, on purpose. Pass 1 of the yap24 loop runs this file alone:
//   args: {mode:'build', only:['yap24-NT'], panelApproved:[...NT ids the panel approved]}
// Every item is gated:'panel' until the Senior Panel on the 2026-09-26 audit rules.
//
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs and docs/loop/HARNESS.md.
// Never touch the bundle identifier or the data directory name. Never sandbox. Headless only:
// no test in this file opens a window, and no acceptance command needs a microphone or a TCC grant.
//
// ACCEPTANCE HYGIENE (Panel round 2, 2026-09-26): the command runner executes every acceptance line
// ON ITS OWN, so a standalone `export` line sets nothing for the lines after it. Every cargo/bash
// line in this file therefore carries YAP_DATA_DIR="$(mktemp -d)/yap-state" INLINE — without it,
// cargo test on the loop machine writes into Wilson's real ~/Library/Application Support/WilsonVoice.

ITEMS.push({
  id: 'yap24-NT0', prompt: 'yap24-NT', branch: 'loop/yap24-nt0-provision-the-notetaker-eval-model-and-corpus', gated: 'panel',
  title: 'Provisioning, not product: the pinned parakeet model sits sha256-verified in the lane cache and the meeting eval corpus is materialised at its fixed path',
  notes: 'NEW item added by the Senior Panel round-2 verify 2026-09-26. Pure infrastructure (no app code), so it is in the pass-1 panelApproved list. yap24-NT2 (WER/RTF gate) and yap24-NT9 (the e2e proof) DEPEND on it: both used to be unable to run their model-backed gates on a throwaway YAP_DATA_DIR, which is how the e2e proof ended up specified as SKIP-exit-0.',
  preflight: `
    test -x scripts/provision-notetaker-eval.sh
    test "$(shasum -a 256 "$HOME/code/wilson-voice-loop/cache/models/parakeet-unified-en-0.6b-Q8_0.gguf" | cut -c1-64)" = 4b50b6dd862bf6e346929aaf4f5eaacec003bfa3f56462d6c874b41ef2f38795
    (R="$(git rev-parse --show-toplevel)" && cd "$HOME/yap-eval-corpus/meetings" && shasum -a 256 -c "$R/desktop/src-tauri/tests/fixtures/meeting_eval_manifest.sha256")
  `,
  spec: `
    Panel: APPROVED for pass 1 (round-2 verify 2026-09-26) — infrastructure only, no product surface.
    DEPENDS: none. yap24-NT2 and yap24-NT9 depend on THIS.

    WHY: a throwaway YAP_DATA_DIR never has a model (app_paths.rs:71 puts models under
    root.join("models")), so every model-backed gate in this chain either touched Wilson's real
    install or skipped. The fix is to provision the two inputs ONCE, at fixed paths, verified by
    hash, and have each gate point at them explicitly — never at the real install, never skip.

    THE TWO FIXED PATHS (acceptance lines in NT0, NT2 and NT9 name them literally):
      - model:  $HOME/code/wilson-voice-loop/cache/models/parakeet-unified-en-0.6b-Q8_0.gguf
                (the lane cache; Drain removes only the target-* dirs and the worktrees, never
                cache/). This directory is what YAP_MODEL_DIR means everywhere in this file.
      - corpus: $HOME/yap-eval-corpus/meetings (the durable location meeting_eval.rs already
                defaults to, CORPUS_HOME_RELATIVE). This is what YAP_EVAL_CORPUS means.

    DO
      1. scripts/provision-notetaker-eval.sh (bash, set -euo pipefail, executable, idempotent):
         a. Read the pin from desktop/src-tauri/src/catalog.json — the models[] entry with
            id handy-computer/parakeet-unified-en-0.6b-gguf, its revision, and the files[] row with
            quant Q8_0 (filename, size_bytes, sha256). The catalog is the one source of truth; do
            not hardcode a second copy of the hash in the script.
         b. If the cached file exists and its sha256 matches, print "model ok" and move on.
            Otherwise: if Wilson's installed copy exists at
            ~/Library/Application Support/WilsonVoice/models/<filename> AND its sha256 matches,
            clone it READ-ONLY into the cache (cp -c, APFS clone — never move, never modify the
            installed copy). Else download to <file>.partial from
            https://huggingface.co/<id>/resolve/<revision>/<filename>, falling back to each
            catalog mirror at <mirror>/<id>/<revision>/<filename> (the same order models.rs
            download_urls uses); verify sha256 BEFORE the atomic rename; a mismatch deletes the
            partial and exits non-zero.
         c. Corpus: if $HOME/yap-eval-corpus/meetings verifies with
            shasum -a 256 -c desktop/src-tauri/tests/fixtures/meeting_eval_manifest.sha256 (run from
            the corpus dir), print "corpus ok". Otherwise grow it with the SHIPPED generator
            (cargo test --features custom-protocol --test meeting_eval meeting_eval_generate_corpus
            -- --ignored --nocapture, with YAP_DATA_DIR="$(mktemp -d)/yap-state" inline and
            YAP_EVAL_CORPUS pointing at the fixed path) and verify again. A regrown corpus whose
            bytes disagree with the committed manifest (a different macOS say voice build) is a
            hard failure with that sentence — NEVER rewrite meeting_eval_manifest.* to match.
         d. Print the two paths and exit 0 only when both verified.
      2. docs/loop/HARNESS.md: a short "Notetaker eval inputs" note naming the two paths, the
         script, and YAP_MODEL_DIR / YAP_EVAL_CORPUS.

    NOT: no Rust or TypeScript change; no write into Wilson's installed data dir (read + clone only);
    no audio committed; no edit to catalog.json or the eval manifest.
  `,
  acceptance: `
    test -x scripts/provision-notetaker-eval.sh
    bash -n scripts/provision-notetaker-eval.sh
    YAP_DATA_DIR="$(mktemp -d)/yap-state" bash scripts/provision-notetaker-eval.sh
    grep -q 4b50b6dd862bf6e346929aaf4f5eaacec003bfa3f56462d6c874b41ef2f38795 desktop/src-tauri/src/catalog.json
    test -f "$HOME/code/wilson-voice-loop/cache/models/parakeet-unified-en-0.6b-Q8_0.gguf"
    test "$(shasum -a 256 "$HOME/code/wilson-voice-loop/cache/models/parakeet-unified-en-0.6b-Q8_0.gguf" | cut -c1-64)" = 4b50b6dd862bf6e346929aaf4f5eaacec003bfa3f56462d6c874b41ef2f38795
    test -f "$HOME/yap-eval-corpus/meetings/lecture-15min/audio.wav"
    (R="$(git rev-parse --show-toplevel)" && cd "$HOME/yap-eval-corpus/meetings" && shasum -a 256 -c "$R/desktop/src-tauri/tests/fixtures/meeting_eval_manifest.sha256")
  `,
})

ITEMS.push({
  id: 'yap24-NT1', prompt: 'yap24-NT', branch: 'loop/yap24-nt1-never-purge-untranscribed-meeting-audio', gated: 'panel',
  title: 'RESIDUAL: launch recovers a crashed meeting (recover_orphaned_meetings gets its first production caller) and non-complete audio has a 30-day window and a 5 GB ceiling',
  preflight: `
    grep -v '^[[:space:]]*//' desktop/src-tauri/src/lib.rs | grep -q 'recover_orphaned_meetings('
    grep -q 'FAILED_AUDIO_RETENTION_DAYS' desktop/src-tauri/src/meetings.rs
    grep -q 'FAILED_AUDIO_CEILING_BYTES' desktop/src-tauri/src/meetings.rs
    test -f desktop/src-tauri/tests/meeting_failed_audio_ceiling.rs
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26 as a RESIDUAL item: #218 (7399ec5) already shipped the state-aware purge test and reconcile_stranded_meetings; only what the last paragraph lists remains.
    DEPENDS: none (first item of the chain — it stops the data loss before anything else lands)

    Panel revisions 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, HIGH, GROUNDED):
      - Retention keys off a NEW pipeline_done_at column (set only once transcription, summary
        and diarization all reach a terminal state), never off started_at — db.rs:1945-1969's
        cutoff on started_at alone lets audio purge between "transcription complete" and
        NT5's diarize read or a Retry re-decode. Add MIGRATION_6_MEETING_NOTES in meetings.rs
        (SCHEMA_VERSION = 6) for this column (shared with NT4's meeting_items table below).
      - Failed/partial-with-audio meetings get a LONGER retention window (>=30 days), not
        indefinite: at ~230 MB/hour of two-track audio a backlog of failed meetings otherwise
        grows without bound. Add a disk ceiling (default 5 GB retained meeting audio); past it
        the oldest non-complete audio purges first, one sentence about it in Settings.
      - At launch, BEFORE reconcile, call meeting::recover_orphaned_meetings(meetings_dir) —
        it already exists (meeting.rs:1599) but has NO production caller today (only a unit
        test and meeting_matrix.rs reference it), so a crash mid-meeting leaves the row stuck
        in "recording" forever. Wire its FinalizedMeeting results into finish_meeting /
        set_meeting_sys_wav_path via wav_for_track (never a positional index), set state from
        FinalizedMeeting.state, then hand it to yap24-NT2's queue.
      - "Complete with zero segments" is a documented VALID outcome (a genuinely silent
        meeting) — do not special-case it out of purge eligibility; use pipeline_done_at, not
        segment count, as the purge criterion.
      - Delete the per-track ASR progress ledger (<id>.tN.asr-progress.json, see NT2) and the
        kept per-track host-time anchors (<id>.tN.index.jsonl, see NT2) together with the WAV
        whenever audio is purged or the meeting is deleted — today neither is referenced by
        purge_meeting_audio or the delete cascade.
      - Every cargo/bash acceptance command below carries YAP_DATA_DIR="$(mktemp -d)/yap-state"
        INLINE (round-2 verify: a standalone export line does not persist — the runner executes
        each line on its own). Without it, cargo test on this machine has already written into
        ~/Library/Application Support/WilsonVoice (a stray probe file from an earlier run is
        proof) — never touch Wilson's real install.

    EVIDENCE
      - db.rs:1936-1972  purge_meeting_audio selects every meeting with a WAV path and
        started_at < cutoff. It does not look at state, so a meeting still in transcribing, partial
        or failed loses the only copy of its audio after AUDIO_RETENTION_DAYS (meetings.rs:73 = 7).
      - lib.rs:2960-2965 says why the purge is time-based: "there is no summarize stage yet (YV97)".
        The stage exists now (summarize.rs), but no transcript ever reaches it (see yap24-NT2).
      - Wilson's install, read-only DB copy 2026-09-26: 4 meetings, all state=transcribing,
        processed_through_seconds 0.0, audio_kept 0, mic_wav_path NULL, sys_wav_path NULL;
        ~/Library/Application Support/WilsonVoice/meetings is empty. yap.log shows each of them
        "stopped ... -> transcribing" and nothing afterwards.

    DO
      1. [SHIPPED by #218; the "at least one segment" clause is now the rule for the 7-day class only, see the last paragraph] Retention is STATE-AWARE: the 7-day purge applies only to a complete meeting that has at least one meeting_segments row. [SUPERSEDED 2026-10-26: any other state keeps its audio 30 days, bounded by the 5 GiB ceiling (R2 in the last paragraph), and the Meetings view (yap24-NT6) says so.] Both tracks age out together (the YV106 promise stays).
      2. reconcile_stranded_meetings(), run once at launch after the DB opens and BEFORE the first
         hygiene sweep:
           - state recording with no live capture  -> partial (crash mid-meeting; the capture
             journal already recovers what it can — reuse it, do not duplicate it);
           - state transcribing/summarizing whose WAV exists -> left as is and handed to the
             pipeline queue yap24-NT2 creates (until NT2 lands: left as is, logged once);
           - state transcribing/summarizing whose WAV is gone -> failed, error = one honest
             sentence ("The audio for this meeting was removed before Yap transcribed it."),
             audio_kept 0. Never silently complete.
      3. One log line per reconciled row with id, old state, new state.

    NOT
      - Do not change AUDIO_RETENTION_DAYS. Do not delete rows. Do not touch dictation retention.
      - Do not rename the data directory or any column.

    Tests (new file tests/meeting_retention_keeps_untranscribed_audio.rs):
      - a transcribing meeting older than the window keeps both WAV paths after purge;
      - a complete meeting WITH segments older than the window loses both;
      - a complete meeting with ZERO segments keeps its audio;
      - reconcile marks a transcribing row whose WAV is missing as failed with the sentence,
        and leaves a transcribing row whose WAV exists untouched.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - WHY A RESIDUAL: this item's old preflight passed on main (the #218 test file and reconcile_stranded_meetings exist), so the harness
        would have marked it already-done and every revision above that #218 did not ship would have vanished. The preflight now keys on
        the residual only.
      - RESIDUAL DO (names are contract; the acceptance greps them):
          R1. meeting::recover_orphaned_meetings (meeting.rs:1599) has only test callers today. Call it ONCE at launch, after the DB opens and
              BEFORE reconcile_stranded_meetings; wire each FinalizedMeeting into finish_meeting / set_meeting_sys_wav_path via wav_for_track
              (never a positional index) and set the row state from FinalizedMeeting.state, then hand the row to the NT2 queue.
          R2. purge_meeting_audio (db.rs:2052+) selects only state='complete' AND EXISTS segments, so every failed, partial or
              complete-with-zero-segments row keeps its audio forever (~230 MB/hour of two tracks). Add FAILED_AUDIO_RETENTION_DAYS = 30
              and FAILED_AUDIO_CEILING_BYTES = 5 GiB in meetings.rs: a row retained for any reason other than (complete AND at least one
              segment) is purged after 30 days, and past the 5 GiB ceiling the OLDEST such rows purge first until under it. One plain
              sentence in Settings says so. Test file tests/meeting_failed_audio_ceiling.rs (ordering, ceiling, the 7-day class untouched).
          R3. Purge and delete also remove the per-track ledger files and the kept anchors (<id>.*.asr-progress.json, <id>.t*.index.jsonl) together
              with the WAVs. Tested in the same file.
          R4. (folded in from the retired NT3) kind in_person must plan TrackBPlan::MicOnly with no badge and no error. meeting_kind_branch
              must assert it; add the assertion if it is absent.
      - VOID: the pipeline_done_at column and MIGRATION_6_MEETING_NOTES for it (and the "use pipeline_done_at, not segment count" bullet). Its
        two readers are gone: diarization is deferred (NT5) and the notes job (#250) reads transcript rows, not audio. The acceptance fails
        if pipeline_done_at appears. No schema change in this item.
      - Conflict that was resolved: DO step 1's "at least one segment" text contradicted the revision's pipeline_done_at criterion. The shipped
        rule (complete AND at least one segment, 7 days) stays; everything else falls under R2.
    Audit 2026-10-26: confirmed on the INSTALLED 0.9.3-dev (a740614) that recover_orphaned_meetings (meeting.rs:1599) is still called only from its test (meeting.rs:4026). Storage is 16-bit WAV at 1.92 MB/min per track (60 MB per 31-min track, 320 MB kept); consider FLAC/Opus under the 5 GB ceiling. See docs/audits/meeting-notetaker-audit-2026-10-26.md issues #11 and #17.
  `,
  acceptance: `
    grep -v '^[[:space:]]*//' desktop/src-tauri/src/lib.rs | grep -q 'recover_orphaned_meetings('
    grep -q 'FAILED_AUDIO_RETENTION_DAYS' desktop/src-tauri/src/meetings.rs
    grep -q 'FAILED_AUDIO_CEILING_BYTES' desktop/src-tauri/src/meetings.rs
    ! grep -rn 'pipeline_done_at' desktop/src-tauri/src
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_failed_audio_ceiling
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_retention_keeps_untranscribed_audio
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_audio_retention
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_kind_branch
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib meeting
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-NT2', prompt: 'yap24-NT', branch: 'loop/yap24-nt2-wire-the-meeting-transcription-pipeline', gated: 'panel',
  title: 'RESIDUAL: the shipped meeting pipeline gets its guards: live recording never counts toward max_yield, a crash-loop guard, waiting_for_model, asr_model on the row, and a REQUIRED WER/RTF run',
  preflight: `
    ( if ! ( test -f "$HOME/code/wilson-voice-loop/cache/models/parakeet-unified-en-0.6b-Q8_0.gguf" ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap24-NT0 has not provisioned the lane model cache; build NT0 first"; exit 0; fi; grep -q 'YAP_EVAL_REQUIRE' desktop/src-tauri/tests/meeting_eval.rs && grep -q 'YAP_MODEL_DIR' desktop/src-tauri/tests/meeting_eval.rs && grep -q 'asr_model' desktop/src-tauri/src/meeting_pipeline.rs && grep -q 'waiting_for_model' desktop/src-tauri/src/meeting_pipeline.rs && grep -q 'MAX_UNPROGRESSED_ATTEMPTS' desktop/src-tauri/src/meeting_pipeline.rs && grep -q 'catch_unwind' desktop/src-tauri/src/meeting_pipeline.rs && test -f desktop/src-tauri/tests/meeting_pipeline_residual.rs )
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26 as a RESIDUAL item: #218 (7399ec5) shipped meeting_pipeline.rs and the wiring test; only the guards in the last paragraph remain.
    DEPENDS: yap24-NT0 (the pinned model + eval corpus its WER/RTF gate runs against),
    yap24-NT1 (audio must not be purged out from under a queued job)

    Panel round-2 verify 2026-09-26 (applied — acceptance now enforces what the revision below says):
      - The WER/RTF run is IN the acceptance, not prose: meeting_eval runs with
        YAP_EVAL_REQUIRE=1, YAP_EVAL_CORPUS=$HOME/yap-eval-corpus/meetings and
        YAP_MODEL_DIR=$HOME/code/wilson-voice-loop/cache/models (both provisioned and
        hash-verified by yap24-NT0). Implement in tests/meeting_eval.rs: (a) YAP_EVAL_REQUIRE=1
        turns the CORPUS_ABSENT skip into a panic, and a missing model into a panic — never a
        green skip; (b) when YAP_MODEL_DIR is set, the Decoder links (symlink or cp -c clone) the
        pinned parakeet Q8_0 file from it into <YAP_DATA_DIR>/models/ before the first decode, and
        refuses to run if YAP_DATA_DIR is unset (it must never seed or read the default root);
        (c) print WER, real-time factor and lecture-15min stop-to-notes wall clock, and commit
        the measured numbers to docs/BUDGETS.md.
      - The launch call site is pinned: lib.rs's start path calls meeting_pipeline::spawn (that
        exact path — the acceptance greps for it; "or the equivalent" is gone).

    Panel revisions 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, 3x BLOCKING + HIGH, GROUNDED):
      - BLOCKING — per-track ledger collision: JsonProgressStore keys ONLY on meeting_id
        (meeting_asr.rs:1597-1599), so the system track's run() resumes from the mic track's
        processed_through_seconds and dedups chunk.index against the mic track's chunks. Key
        the ledger per track, e.g. run(&format!("{id}.t{track}")), and add a
        meeting_pipeline_wired test with two tracks of different lengths asserting each track's
        own processed_through and no chunk-index collision.
      - [VOID 2026-10-26: #218 shipped per-chunk append plus delete-past-ledger (meeting_pipeline.rs:17-21, :517), which already makes a resume idempotent; no one-shot merge transaction is required here, the host-time merge is NT11-0.] BLOCKING — segments must NOT land per-chunk. append_meeting_segments (db.rs:1652) is a
        non-idempotent INSERT with a fresh row id every call: writing "as each chunk completes"
        (as this spec said) double-writes every seam and duplicates rows on any resume-after-quit.
        Instead: run BOTH tracks' MeetingAsr to completion (interrupted == false), call the
        shipped merge_two_tracks_by_host_time(ledger_a.chunks, ledger_b.chunks, anchors, epochs,
        kind) (meeting_asr.rs:1123), then in ONE transaction DELETE FROM meeting_segments WHERE
        meeting_id=? and INSERT the merged spans, set state complete. Drive progress EVENTS from
        the ledger's processed_through_seconds, not from segment-row writes.
      - BLOCKING — the two-track merge needs the per-track host-time anchors
        (<id>.tN.index.jsonl), which finalize_meeting_marker deletes today "for a consumer that
        does not exist" (meeting.rs:1794-1798, :1842-1845). NT2 IS that consumer: keep the
        anchor files (or persist records+epochs to a new meeting_anchors table) through
        finalize, and clean them up only via NT1's purge/delete path, never independently.
      - HIGH — max_yield: Duration::from_secs(120) (meeting_asr.rs:1675) breaks "dictation
        always wins": a hands-free take over ~2 minutes (routine in Wilson's logs) makes meeting
        ASR take the engine back mid-take. A live-recording demand must never count toward
        max_yield; the 120 s cap applies only to a stuck "busy, not recording" flag. Add a test:
        a 180 s simulated dictation never observes a meeting chunk take the engine.
      - HIGH — no crash-loop guard: the launch-time requeue of every stranded "transcribing" row
        runs in-process, on the SAME warm engine dictation uses (unlike polish/diarize, which are
        sidecars). Two unexplained SIGSEGVs already exist in the audit (0.8.0, worker threads
        39/40). Add an attempt counter per meeting; two crash-without-progress launches ->
        failed with an honest sentence, never a silent retry loop. Wrap the worker body in
        catch_unwind so one panic marks that row failed and keeps the FIFO worker alive. Start
        the queue ~30 s after launch, after the dictation engine's first warm-up.
      - HIGH — enqueue every row that has audio and zero segments in state {transcribing,
        partial}, not "transcribing" only — stop() and finalize both downgrade to partial on a
        capture hole or spliced silence, and those meetings currently never reach the pipeline
        and never free their disk under NT1's new state-aware retention.
      - "The compiler is the proof the pipeline is wired" is FALSE — meeting_asr.rs's items are
        pub in a lib crate (Cargo.toml crate-type includes rlib), so rustc's dead_code lint never
        fires on them regardless of wiring. Drop that claim from the acceptance's rationale; the
        real proof is behavioral: meeting_pipeline_wired must drive MeetingController::stop (not
        the worker directly) and observe segments > 0, plus a source-scan grep that
        MeetingAsr is constructed outside meeting_asr.rs AND that lib.rs's start path spawns the
        pipeline (grep -q 'meeting_pipeline::spawn' src/lib.rs or the equivalent call site).
      - Reuse the SHIPPED EngineDemand impl: TranscriptionManager already implements it
        (meeting_asr.rs:600-604, exercised by tests/meeting_dictation_preempts_transcription.rs)
        — do not invent a second AppState-based demand. Build the audio source with
        WavWindows::open (meeting_asr.rs:507), never MemoryWindows (690 MB for a 3-hour meeting).
      - Model gating: meeting_availability_for's NoModel/UnknownModel checks only that a model id
        is configured, never that its files are on disk (app_paths.rs:71 puts models under
        root.join("models"), which a fresh/offline install lacks). A refusal on a missing-but-
        configured model must produce a NEW waiting_for_model state that KEEPS the audio and
        auto-requeues on the model-download-complete event, not a permanent "failed" row.
      - Pin meeting ASR to the catalog's recommended model (parakeet, Q8_0), not "whatever
        engine happens to be loaded" — PKG2's interim whisper-tiny path must never become the
        model a meeting is permanently transcribed by; record asr_model on the row, and NT1 only
        purges audio transcribed by the recommended model.
      - Measurability: run the existing WER harness (meeting_eval.rs, WER_GATE 0.02) against
        ~/yap-eval-corpus/meetings inside this item's acceptance when the corpus is present, and
        FAIL (not silently pass) when it is absent from a machine expected to have it (gate via
        YAP_EVAL_REQUIRE=1). Print real-time factor and stop-to-notes wall clock for
        lecture-15min into docs/BUDGETS.md — nothing today measures either number.
      - Every cargo/bash acceptance command below carries YAP_DATA_DIR="$(mktemp -d)/yap-state"
        INLINE — see the NT1 revision for why.

    EVIDENCE — the whole reason "the notetaker is not even working":
      - meeting_control.rs:652-656 sets transcribing with the comment "YV93's transcription
        pipeline moves it on to complete". There is no such pipeline in src/.
      - meeting_asr.rs:52 #![allow(dead_code)] over the module. MeetingAsr (:1718), its run()
        (:1767) and WarmEngineChunkAsr (:614) are constructed only in
        tests/meeting_dictation_preempts_transcription.rs:145 and tests/matrix_new_asr_chunk_timeout.rs:108.
      - db.rs:1652 append_meeting_segments: no caller outside db.rs.
      - tests/meeting_manual_start_stop.rs:158-162 asserts transcribing and ends — no test drives
        stop -> segments -> complete.

    DO
      1. A new module meeting_pipeline.rs owning ONE background worker thread and a FIFO of meeting
         ids. Enqueue on: a successful MeetingController::stop whose state is transcribing; and at
         launch, every row reconcile_stranded_meetings (yap24-NT1) left in transcribing with audio.
      2. The worker builds the shipped job — do not write a second chunker:
           MeetingAsr { store: JsonProgressStore under <meetings dir>/<id>.progress.json,
                        asr: WarmEngineChunkAsr over the ONE warm engine (TranscriptionManager),
                        demand: an EngineDemand that answers true while a dictation is recording
                                or busy (AppState recording/busy) — dictation always wins,
                        quit: the exit-teardown flag, config: MeetingAsrConfig::default() }
         and runs it per track (mic track, then the system track when sys_wav_path is set, tagged
         MIC_TRACK / SYSTEM_TRACK exactly as meeting.rs defines them).
      3. Segments land through Database::append_meeting_segments as each chunk completes (the FTS
         index follows through its existing triggers), processed_through_seconds advances through
         the existing ledger, and the row moves transcribing -> complete (or partial with the
         error sentence when a track failed). The English-only gate (meeting_availability_for) is
         consulted first and a refusal is a failed row with its sentence, not a silent skip.
      4. Emit the existing meeting status event after every chunk and at the end, carrying
         {id, state, processedThroughSeconds, durationSeconds}, so yap24-NT6/NT7 can draw progress.
      5. Remove #![allow(dead_code)] from meeting_asr.rs. Anything still dead after the wiring is
         deleted or given a caller — the compiler is the proof that the pipeline is wired.
      6. [MOVED TO yap24-NT9, which now owns the flag in its own DO and preflight] A headless CLI entry beside --transcribe-file (cli.rs): --transcribe-meeting <mic.wav>
         [--sys <sys.wav>] that runs the same worker against a scratch state root
         (YAP_DATA_DIR required, like --smoke) and prints the segment count and state.
      7. Update meeting_matrix.rs rows whose PolicyOnly cell existed only because the pipeline had
         no caller — a row flips to Test only with a test that proves the call site.

    NOT
      - No second ASR engine, no new model load: meeting ASR shares the warm engine and yields.
      - Never block the main thread or the hotkey path (spawn a named thread, never the Tauri
        async runtime for the decode).
      - Do not touch diarization here (yap24-NT5).

    Tests (tests/meeting_pipeline_wired.rs), stub ChunkAsr, no model, no microphone:
      - controller start -> stop -> pipeline drain => segments > 0, state complete, both tracks
        present for a two-track meeting, processed_through_seconds == duration;
      - a dictation demand mid-meeting preempts at a chunk boundary and the job resumes;
      - a quit mid-job leaves the ledger; relaunch resumes and the transcript equals an
        uninterrupted run (reuse the existing resume-seam assertions);
      - source scan: MeetingAsr is constructed in src/ outside meeting_asr.rs.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - WHY A RESIDUAL: the old preflight (test -f meeting_pipeline_wired.rs, no allow(dead_code), MeetingAsr constructed outside
        meeting_asr.rs) passed on main after #218, so this item was silently already-done and its guards were never going to be built.
      - RESIDUAL DO (shipped and NOT to be redone: the worker module, FIFO, per-track ledger, WavWindows source, progress event, dead_code removal):
          R1. max_yield. lib.rs (~:6790) passes MeetingAsrConfig::default() (max_yield 120 s, meeting_asr.rs:1674, :1935), so a hands-free
              take over two minutes hands the engine back to meeting ASR mid-take. A LIVE-RECORDING demand never counts toward max_yield; the
              120 s cap applies only to a stuck "busy, not recording" flag. Test in tests/meeting_pipeline_residual.rs: a simulated 180 s
              dictation never observes a meeting chunk take the engine.
          R2. Crash-loop guard. Add MAX_UNPROGRESSED_ATTEMPTS (2) in meeting_pipeline.rs with a per-meeting attempt counter (persist it beside the
              progress ledger): two launches that crash without progress mark the row failed with one honest sentence, never a silent retry
              loop. Wrap the worker body in catch_unwind so one panic fails that row and keeps the FIFO alive. Start the queue about 30 s
              after launch, after the dictation engine's first warm-up. Enqueue every row that has audio and zero segments in
              {transcribing, partial}, not transcribing only (verify; add if absent).
          R3. waiting_for_model. meeting_availability_for checks only that a model id is configured. A refusal on a configured-but-missing
              model becomes a NEW state waiting_for_model that KEEPS the audio and auto-requeues on the model-download-complete event;
              never a permanent failed row.
          R4. asr_model. Record the model that transcribed the row in an asr_model column, via the next free versioned migration step (take
              the next number at build time; rebase after #250). Meeting ASR is pinned to the catalog's recommended model (parakeet Q8_0);
              no other engine may transcribe a meeting.
          R5. Measurability. tests/meeting_eval.rs: YAP_EVAL_REQUIRE=1 turns a missing corpus or model into a panic, never a green skip;
              YAP_MODEL_DIR links the pinned model into <YAP_DATA_DIR>/models/ and the run refuses an unset YAP_DATA_DIR. Print WER, real-time
              factor and lecture-15min stop-to-notes wall clock and commit the numbers to docs/BUDGETS.md. Record peak RSS during transcription.
      - The CLI flag --transcribe-meeting (old DO step 6) is no longer here: yap24-NT9 owns it.
    Audit 2026-10-26: a 51.8 s dictation spoken right after a meeting stop waited on the meeting decode: asr_decode_ms=5166 after 'cancelling the in-flight meeting chunk decode' (16:31:38Z), vs 338 ms for a 22.8 s take that morning. Measure take latency while a meeting job runs. ASR RTF on real meetings: 0.011-0.028 idle, 0.13 under loop CI load. Per-chunk timing is NEW yap-audit-meeting-timings. See docs/audits/meeting-notetaker-audit-2026-10-26.md issues #8 and #10.
  `,
  acceptance: `
    grep -q 'YAP_EVAL_REQUIRE' desktop/src-tauri/tests/meeting_eval.rs
    grep -q 'YAP_MODEL_DIR' desktop/src-tauri/tests/meeting_eval.rs
    grep -q 'asr_model' desktop/src-tauri/src/meeting_pipeline.rs
    grep -q 'waiting_for_model' desktop/src-tauri/src/meeting_pipeline.rs
    grep -q 'MAX_UNPROGRESSED_ATTEMPTS' desktop/src-tauri/src/meeting_pipeline.rs
    grep -q 'catch_unwind' desktop/src-tauri/src/meeting_pipeline.rs
    grep -q 'meeting_pipeline::spawn' desktop/src-tauri/src/lib.rs
    ! grep -n "allow(dead_code)" desktop/src-tauri/src/meeting_asr.rs
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_pipeline_residual
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_pipeline_wired
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_manual_start_stop
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_dictation_preempts_transcription
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib meeting
    YAP_DATA_DIR="$(mktemp -d)/yap-state" YAP_EVAL_REQUIRE=1 YAP_EVAL_CORPUS="$HOME/yap-eval-corpus/meetings" YAP_MODEL_DIR="$HOME/code/wilson-voice-loop/cache/models" cargo test --features custom-protocol --test meeting_eval -- --nocapture --test-threads=1
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-NT3', prompt: 'yap24-NT', branch: 'loop/yap24-nt3-silence-is-not-a-denied-permission', gated: 'panel',
  title: 'Silence is not a denial: an in-person meeting never attaches the system tap, and a quiet call is never told macOS refused it',
  preflight: `
    grep -q "output_was_running" desktop/src-tauri/src/syscapture.rs
    test -f desktop/src-tauri/tests/system_audio_verdict_silence_is_not_denial.rs
  `,
  spec: `
    Panel: pending
    DEPENDS: yap24-NT2 (the verdict is written on the same stop path the pipeline now owns)

    Panel revisions 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, HIGH, GROUNDED):
      - The code already has the right per-process probe field (TapEnvironment.
        system_output_active, syscapture.rs:680-683) and the right verdict function
        (TapLiveness::verdict, :501-518), but observe_environment (:2617-2619) has NO caller
        anywhere in src/, so env stays Default and the stop path falls back to the naive,
        device-level permission_verdict (:2334, :2628) this item was meant to replace. Implement
        the probe: kAudioHardwarePropertyProcessObjectList, then per process (skipping getpid())
        kAudioProcessPropertyPID and kAudioProcessPropertyIsRunningOutput; sample it on the pump
        tick and call observe_environment. Route BOTH the stop-path and pre-warm verdicts through
        TapLiveness::verdict and delete the LooksDenied arm of the old permission_verdict.
      - A muted call app (Zoom/Meet) or a mid-meeting default-output-device change (AirPods
        connect) still makes kAudioDevicePropertyDeviceIsRunningSomewhere true with no audible
        output. Evaluate over the WHOLE meeting, track every device that was default at any
        point during it, and require the tap to have delivered nothing for the entire meeting
        AND output to have run >=60s before writing LooksDenied; anything short of that is
        Unknown, and Unknown is never stored as an error. Never flip the setup row to LooksDenied
        from a single meeting — require two consecutive ones.
      - Fix the pre-flight: grep -q "output_was_running" is satisfied by a comment. Gate on the
        test file's existence and passing, not the string.

    EVIDENCE
      - syscapture.rs:2243 DENIAL_GRACE = 3 s; :2334-2341 permission_verdict returns LooksDenied
        whenever the tap delivered no non-zero sample for 3 s. Nothing playing looks identical.
      - syscapture.rs:2324-2327 LOOKS_DENIED_MESSAGE ("macOS has not granted System Audio
        Recording to Yap, and it will not ask again") is stored as the meeting error.
      - Wilson's DB: 3 of 4 meetings carry that sentence, including cc9f6153 whose kind is
        in_person — a room recording that never needed system audio at all.
      - settings_kv meeting_system_audio_setup_ack_v1 = "ran" (2026-08-20), so track_b_plan
        (syscapture.rs:2462-2485) attaches the tap on every meeting.
      - Apple: process taps are AudioHardwareCreateProcessTap, macOS 14.2+
        (https://developer.apple.com/documentation/coreaudio/audiohardwarecreateprocesstap(_:_:));
        the purpose string is NSAudioCaptureUsageDescription, macOS 14.2+
        (https://developer.apple.com/documentation/bundleresources/information-property-list/nsaudiocaptureusagedescription).
        There is no public API to read that grant back, so the verdict must stay a heuristic —
        make it an honest one.

    DO
      1. kind in_person => TrackBPlan::MicOnly with NO badge and NO error. The tap is for the
         can't-join-the-call case (Wilson 2026-08-10: IRL first).
      2. The verdict gains an input: was the default output device running during the window
         (kAudioDevicePropertyDeviceIsRunningSomewhere on the default output device,
         https://developer.apple.com/documentation/coreaudio/kaudiodevicepropertydeviceisrunningsomewhere).
         No delivery + output idle => Unknown ("No system audio played during this meeting."),
         never LooksDenied. LooksDenied only when output was running and the tap stayed silent.
      3. The meeting row stores the verdict sentence only when it is LooksDenied or Failed.
      4. The Settings pre-warm verdict follows the same rule, so an idle Mac cannot write a
         sticky LooksDenied into the setup row.

    Tests: pure verdict table (delivery x output_was_running x ran_for) in
    tests/system_audio_verdict_silence_is_not_denial.rs; an in_person kind yields MicOnly with
    no badge; existing meeting_track_b_wiring stays green.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - RETIRED. Not in panelApproved. Its preflight (output_was_running in syscapture.rs plus the verdict test file) passes on main
        (7399ec5), so the harness would report already-done anyway. The one residual line (kind in_person plans MicOnly, no badge, no error;
        unverified) is folded into yap24-NT1 as R4 with its test in meeting_kind_branch. Nothing else here remains to build.
    Audit 2026-10-26: on a real virtual call (445654ee) the tap went all_zero_buffers at 12:00, rebuild 2/3 timed out, and the call track ends at 690 s of 883 s. On a kind-unknown meeting (d8761579) no_system_audio_observed after 3 rebuilds, so the meeting was mic-only for 9 min. Retarget the tap on an output-route change instead of rebuilding the same tap. See docs/audits/meeting-notetaker-audit-2026-10-26.md issue #5.
  `,
  acceptance: `
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test system_audio_verdict_silence_is_not_denial
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_track_b_wiring
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_kind_branch
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib syscapture
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-NT4', prompt: 'yap24-NT', branch: 'loop/yap24-nt4-notes-summary-and-action-items-automatically', gated: 'panel',
  title: 'RESCOPED after #250: the default notes are MEASURED (action-item recall and precision at least 0.8 on a planted-commitment fixture) and installing the notes model leaves polish_model byte-unchanged',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/summary_jobs.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 250 (default meeting notes) has not landed; land it, then build NT4"; exit 0; fi; test -f desktop/src-tauri/tests/meeting_notes_recall_eval.rs && test -f desktop/src-tauri/tests/notes_model_install_keeps_polish_model.rs && test -f desktop/src-tauri/tests/notes_owner_calendar_name.rs && grep -q 'summary recall' docs/BUDGETS.md )
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26 and RESCOPED after PR #250 (notes as Markdown sections in meetings.summary, summary_jobs.rs, polish_model kept as the user override). The meeting_items table, the summary_model split and MIGRATION_6 in the text below are VOID.
    DEPENDS: yap24-NT2 (segments must exist), SEC-C (merged #184 — the polish model install path)

    Panel revisions 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, HIGH, GROUNDED):
      - Installing the summary model must NOT silently turn on LLM polish for dictation:
        summarize_meeting_blocking and the dictation-polish path both read state.settings.
        polish_model today (lib.rs:4225-4227, :320-325), and SEC-C's one-click install writes
        that same setting (lib.rs:4823-4826). Give summaries their OWN setting, summary_model,
        independent of dictation's polish_model. Add a test asserting polish_model is
        byte-unchanged after a summary-model install.
      - Schema: rename the meeting_actions table to meeting_items(kind CHECK IN ('action',
        'decision','question'), text, speaker_label, segment_id, start_seconds). Drop the
        'owner' column — summarize.rs:1026-1040 is explicit that the speaker is not a claim
        about who OWES the action. Map the positional seg_NNNN label to a real segment_id inside
        summarize before persisting (the mapping never otherwise leaves that process). Write the
        rows and meetings.summary in ONE transaction so the two stores cannot drift on a
        re-summarize. Add this table via the SAME MIGRATION_6 as NT1's pipeline_done_at, not an
        unversioned ALTER TABLE/CREATE TABLE IF NOT EXISTS — db.rs's own comment says shipped
        migration steps are immutable and its ad hoc let _ = conn.execute("ALTER TABLE ...")
        discards errors (db.rs:476-522, finding #26 cited at :480).
      - Give the summary job the SAME EngineDemand suspend-between-map-chunks behavior as
        MeetingAsr: on an 8 GB Mac, the ASR engine (warm 15 min), the dictation polish sidecar
        (warm for the process lifetime) and a second Qwen-1.5B summary sidecar together approach
        ~3 GB resident. Unload the ASR engine before summarizing when no dictation is pending, or
        route the summary through the already-warm polish sidecar on a low-priority queue.
        Record peak RSS (app + sidecars) during a summary into docs/BUDGETS.md with a ceiling
        (e.g. 2.5 GB on 8 GB machines) that falls back to summarizing on the next idle tick.
      - Add ONE measured eval: extend the synthetic corpus generator to plant K explicit
        commitments in a 15-minute fixture, run the REAL 1.5B sidecar (#[ignore] — the loop runs
        it only when the model is installed) and gate action-item recall >= 0.8 and precision
        >= 0.8, numbers committed to docs/BUDGETS.md. Shape/groundedness validators alone do not
        tell Wilson whether the notes are useful.

    EVIDENCE
      - lib.rs:4201-4260 summarize_meeting is a MANUAL command and refuses when there are no
        segments (:4216-4218) — which, before yap24-NT2, is every meeting.
      - The notetaker plan (Obsidian Notes/Yap-Notetaker-Epic-Plan-2026-08-10.md, schema
        meetings / meeting_segments / speaker_profiles / meeting_actions) is only partly real:
        Wilson's DB has no meeting_actions table (sqlite .tables, 2026-09-26).
      - The summary is a free-text column (meetings.summary). Action items with evidence links
        were the Read-AI-style promise ("transcript, summary, NEXT ACTIONS" — Wilson 2026-08-10).

    DO
      1. When the pipeline completes a meeting, enqueue a summarize job on the same worker (after
         transcription, never concurrently with it). It spawns its OWN sidecar session exactly as
         summarize_meeting_blocking does (lib.rs:4221-4223) so dictation is never starved.
      2. No local summary model installed => state stays complete, a new field
         summary_status = 'needs_model' is set, and the UI (yap24-NT6) offers the one-click
         install SEC-C already ships. Never an error toast.
      3. Migration: meeting_actions(id INTEGER PRIMARY KEY, meeting_id TEXT NOT NULL REFERENCES
         meetings(id) ON DELETE CASCADE, idx INTEGER NOT NULL, text TEXT NOT NULL, owner TEXT,
         evidence_segment_id INTEGER). The summarizer's structured output (summarize.rs, GBNF
         JSON) writes the action items there; each cites the segment it came from.
      4. Markdown export (the existing export path) gains Summary and Action items sections above
         the transcript.
      5. The manual Summarize command keeps working and re-runs the same job (idempotent: the
         empty-summary refusal at lib.rs:4250-4262 stays).

    Tests (tests/meeting_notes_auto_summary.rs, stub summary client): completion enqueues a
    summary; actions land as rows with evidence ids; no model => needs_model and no error;
    deleting a meeting cascades its actions (extend meeting_delete_cascade); export contains
    both sections.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - WHY: PR #250 (open, landing today) already owns the product half: default 1.5B notes model, notes stored as Markdown sections in
        meetings.summary, summary_jobs.rs, polish_model kept as the user's override. This item's old schema (meeting_items, a summary_model
        setting, MIGRATION_6) contradicts a binding ledger decision and duplicates #250, so it is VOID. Action items stay Markdown, not rows;
        rows are added only if a later item (X1, X3) is approved and needs them.
      - RESIDUAL DO, after #250 is on main (the preflight holds this item until summary_jobs.rs exists):
          R1. Provisioning: extend scripts/provision-notetaker-eval.sh to also verify (and APFS-clone read-only from the installed copy, or
              download per the polish catalog pin) the qwen2.5-1.5b Q4_K_M notes model into the lane cache next to the parakeet file. Same
              rules as NT0: hash from the catalog, never a second copy of the hash in the script.
          R2. Measured eval: extend the synthetic corpus generator to plant K explicit commitments in the 15-minute fixture and add
              tests/meeting_notes_recall_eval.rs, #[ignore], running the REAL 1.5B sidecar through summary_jobs. Gate action-item recall >= 0.8 and
              precision >= 0.8; under YAP_EVAL_REQUIRE=1 a missing model is a panic, not a skip. Commit both numbers to docs/BUDGETS.md
              under a heading containing "summary recall".
          R3. tests/notes_model_install_keeps_polish_model.rs: installing or selecting the notes model leaves the dictation polish_model setting
              byte-unchanged (read it before and after).
          R4. Memory: record peak RSS (app plus sidecars) during a summary in docs/BUDGETS.md against the 8 GB floor machine, and make the summary
              job run serialized with ASR, never concurrently with a live decode, falling back to the next idle tick above a stated ceiling.
      - Seat evidence: ASR warm (15 min) plus the 1.5B notes sidecar is about 2.6-3.0 GB resident, about 4 GB if the polish and notes
        sidecars do not share mmap'd weights; next to a live Zoom on 8 GB that swaps, which is why R4 is a gate and not advice.
    Audit 2026-10-26: all 3 meetings PR 250 summarized on Wilson's Mac grade 1/5. The Summary is 0.91-0.97 verbatim (4-gram containment), key points are raw lines (backfill_key_points), small talk is filed as actions, owners are wrong under echo, and there are up to 8 cross-section duplicates. Rejections per run: 17-38 v3_ungrounded + 3-17 v2_runaway. Add to this eval: Summary verbatim ratio <= 0.5 and owner accuracy. The first post-upgrade meeting auto-downloads 1.1 GB (allows_auto); make that visible before the run. Mechanical fixes are NEW yap-audit-notes-guards; strategy is NEW yap-audit-notes-strategy. See docs/audits/meeting-notetaker-audit-2026-10-26.md issues #2 and #18.

    Panel revisions 2026-10-26 (Senior Panel 3, two seats + synthesis; AI/models and PM seat; applied as gates):
      - R2 EVAL, RESCOPED. The planted fixture is a 15-minute synthetic (macOS say) meeting containing 6 explicit commitments ("I will send the deck by Friday"),
        4 implicit commitments ("I can get you the specs"), 2 decisions, 6 small-talk distractors that use "going to" or "'ll" (generic_commitment lets them through),
        and at least 2 commitments inside the last 20% of the meeting. It runs through summary_jobs alongside call-bleed-2voice (yap-audit-eval-call-bleed). Gates, all
        asserted by tests/meeting_notes_recall_eval.rs and committed to docs/BUDGETS.md under the heading containing "summary recall":
          action recall >= 0.8, action precision >= 0.8, decision recall >= 0.5, Summary 4-gram verbatim ratio <= 0.5, cross-section duplicates = 0,
          items per cited segment <= 2, owner accuracy >= 0.8 OR the owner left blank ("Someone here" counts as blank: it makes no claim).
        Explicit-only plants reach 0.9 recall while real meetings with implicit commitments and echo still fail; that is why the implicit, distractor and
        end-of-meeting plants are in the gate.
      - HUMAN HALF, numbers only: run scripts/eval/compare-reference.py --notes on a COPY of the 31-minute real meeting's DB and paste the verbatim ratio, duplicate
        count, max items per segment and owner counts in the PR body. Never paste text.
      - R5 (the v1 remote-name path, which also settles the deferred yap-audit-remote-speaker-names): a pure fn other_party_name(attendees, self) -> Option<String> that returns the
        display name when exactly one non-self calendar attendee exists and None for zero or two-plus. A Them-track item's owner in the NOTES uses that name when it is Some,
        otherwise reads "Them". Transcript labels stay Me / Them. It reads the attendees yap-notetaker-detect stores from the calendar match (opt-in); if that item stores
        none when this builds, ship the pure fn and its test and leave the wiring to that item (say so in the PR body). No new permission, no Accessibility read.
        tests/notes_owner_calendar_name.rs: one attendee -> name; none -> None; three -> None; the self address is excluded.
      - The owner-claim rules (mic-track "Someone here", echo owner) live in yap-audit-notes-guards; arms E and C are measured by yap-audit-notes-strategy, which also owns the honest
        fallback copy. This item owns the eval and its numbers only.
  `,
  acceptance: `
    test -f desktop/src-tauri/src/summary_jobs.rs
    test -f desktop/src-tauri/tests/notes_owner_calendar_name.rs
    ! grep -rn 'meeting_items' desktop/src-tauri/src
    ! grep -rn 'summary_model' desktop/src-tauri/src
    grep -q 'summary recall' docs/BUDGETS.md
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test notes_model_install_keeps_polish_model
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test notes_owner_calendar_name
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_notes_recall_eval
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test summary_default_notes
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib summarize
    YAP_DATA_DIR="$(mktemp -d)/yap-state" YAP_EVAL_REQUIRE=1 YAP_MODEL_DIR="$HOME/code/wilson-voice-loop/cache/models" cargo test --features custom-protocol --test meeting_notes_recall_eval -- --ignored --nocapture --test-threads=1
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-NT5', prompt: 'yap24-NT', branch: 'loop/yap24-nt5-diarization-joins-the-pipeline', gated: 'panel',
  title: 'Who said what: in-person meetings run the shipped diarization sidecar after transcription, with the honest accuracy framing',
  preflight: `
    grep -rln "diarize::pool()" desktop/src-tauri/src | grep -vE "diarize\\.rs|meeting_matrix\\.rs" | grep -q .
    test -f desktop/src-tauri/tests/meeting_pipeline_diarizes_in_person.rs
  `,
  spec: `
    Panel: DEFERRED 2026-09-26T17:35:00Z — see docs/loop/DEFERRED.md. Excluded from pass-1 panelApproved.
    Re-approve only after Y11-A..Y11-E are merged AND Y11-F's real-voice DER/FAR is measured
    against a stated threshold. DEPENDS is prose the harness does NOT enforce (no 'DEPENDS'
    handling exists in template.mjs/build.mjs — grep confirms it), and pass 1 runs
    only:['yap24-NT'], which hard-skips every Y11-* id, so an approved NT5 would build speaker
    enrollment on a path independently measured at FAR 1.000 (Y11-C) with DER 0.34-0.45 on clean
    synthetic voices (Y11 audit). Meeting notes that confidently attribute words to the wrong
    person are worse than notes with no speaker labels — 3 of 5 panel seats named this the single
    item to kill from pass 1. yap24-NT6/NT7 already render labels only when present, so leaving
    NT5 out costs nothing else in the chain.
    DEPENDS: yap24-NT2, Y11-A..Y11-E (the six parked diarization defects — FAR 1.000 on the shipped
    enrollment path, split_partition seeding, the pinned-digest skip — must be fixed first or this
    item ships known-wrong labels)

    EVIDENCE
      - meeting_matrix.rs:141-145: "diarize::pool() has no caller, no meeting is ever handed to a
        sidecar".
      - yap23 shipped the sidecar (yap-diarize, sherpa-onnx CAM++ 192-dim) and clustering (#141),
        and parked six items (#142-#146, #149, issues #150-#155).
      - Eval numbers are floors on a synthetic say-voice corpus (DER 0.34-0.45) — Y11-F gates the
        real-voice corpus.

    DO
      1. For kind in_person (and unknown), after transcription completes, hand the mic track to
         diarize::pool() and attribute clusters to segments with the shipped
         attribute_clusters / rank_and_floor path. kind call keeps track labels (You / Them).
      2. New or unknown voice => the "who is this?" prompt the plan specified (one question per
         cluster, never per segment), stored in speaker_profiles.
      3. UI copy states the accuracy honestly: "Speaker labels are a best guess — tap to fix."
      4. A diarize failure never fails the meeting: transcript and notes stand, labels are absent,
         one sentence says why.

    Tests: tests/meeting_pipeline_diarizes_in_person.rs with a stub DiarizeClient; a failing
    sidecar leaves the meeting complete with no labels.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - DEFER: after the first 100 users (not in panelApproved). Y11-A..E are not on main and speaker labels measured at FAR 1.000.
      - Regex bug fixed even though deferred: the preflight used grep -v with a BRE alternation, where | is a literal, so the comment at
        meeting_matrix.rs:143 satisfied line 1. It is grep -vE now, with escaped dots.
      - Single owner: the ungated yap-notetaker-notes item no longer wires diarize::pool() (its step was struck today), so this item is the
        ONLY owner of diarization in the meeting pipeline.
    Audit 2026-10-26: on a 25-min in-person meeting (ba4dac81) every one of 47 rows reads 'Speaker' and cluster_index is NULL on every segment in the DB; yap-diarize is bundled in /Applications/Yap.app but never called. On the 31-min call a second local voice (75% of its words in the mic track) is labelled 'Me'. See docs/audits/meeting-notetaker-audit-2026-10-26.md issues #6 and section 10b.
  `,
  acceptance: `
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_pipeline_diarizes_in_person
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_cluster_attribution
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib diarize
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test -p yap-diarize --release
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-NT6', prompt: 'yap24-NT', branch: 'loop/yap24-nt6-meetings-view-shows-every-state', gated: 'panel',
  title: 'The Meetings view tells the truth about every meeting: progress, notes, failure with Retry, audio kept or expired',
  preflight: `
    ( if ! ( test -f desktop/src/meetings/NotesView.tsx ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 250 (NotesView) has not landed; land it, then build NT6"; exit 0; fi; grep -qE 'canRetry\\([^)]*audioKept' desktop/src/meetings/progress.ts && ! grep -q 'canRetry(m.state)' desktop/src/views/Meetings.tsx && ! test -e desktop/src/meetings/viewState.ts )
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26.
    DEPENDS: yap24-NT2, yap24-NT4 (states and fields it renders)

    EVIDENCE
      - views/Meetings.tsx:339-347: a transcribing meeting with no segments renders "Yap is still
        working through the audio." — forever, because nothing was working through it.
      - No progress, no failed/partial rendering with a way out, no Retry, no summary-model state,
        no action items (the summary panel at :332-337 is one paragraph).
      - notetaker_status (lib.rs, the 14.4 gate sentence) reaches only the Settings setup step
        (meeting_matrix.rs row 12b).

    DO
      1. [VOID 2026-10-26: no viewState.ts; extend desktop/src/meetings/progress.ts, see the last paragraph] A pure module desktop/src/meetings/viewState.ts: meetingViewState(meeting, segmentsCount)
         -> { badge, headline, detail, progress: 0..1 | null, actions: ('retry'|'summarize'|
         'install-model'|'export'|'delete')[] } covering recording, transcribing (with
         processedThroughSeconds / durationSeconds), summarizing, complete, complete +
         needs_model, partial, failed, audio expired. Every state has exactly one headline.
      2. Meetings.tsx renders it: a progress bar that moves with the pipeline event, the notes
         (summary + action items, each item linking to its transcript segment), Retry for
         failed/partial (re-enqueues through a new retry_meeting command), Export Markdown.
         Panel revision 2026-09-26T17:35:00Z: Retry is offered ONLY when a WAV path exists and
         audio_kept = 1 — NT1 marks old rows failed with their audio already gone, and a Retry
         button that can never work is worse than none. meetingViewState needs a test for the
         no-audio failed case (actions: delete/export only).
      3. The empty state offers "Start a meeting" with the kind picker (in person / call) and
         carries notetaker_status when system audio is unavailable.
      4. Follow the token layer (Y5-A) and the empty/loading/error pattern (Y5-B) if merged;
         otherwise the existing App.css tokens — no new colours.

    Tests: [progress.test.ts, not viewState.test.ts] — one case per state, the progress arithmetic, and "no state renders
    an empty headline".

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - REAL DEFECT, LIVE ON MAIN: Meetings.tsx:289 calls canRetry(m.state) (progress.ts:58), which offers Retry on failed and partial rows even
        when audio_kept = 0 and no WAV exists, which is exactly what this item's Retry rule forbids.
      - CHANGE: extend progress.ts to canRetry(state, audioKept) and have Meetings.tsx (and #250's NotesView) pass the row's audio_kept; Retry is
        offered ONLY when a WAV path exists and audio_kept = 1. Do NOT create viewState.ts: it would duplicate #218's progress.ts and #246/#250's
        state mapping. progress.test.ts gains the no-audio failed case (actions: delete and export only) and a two-argument canRetry table.
      - The "N lines hidden as speaker echo - Show" line stays DEFERRED to the NT11 pass. Rebase on #246 and #250 before starting (the
        preflight holds this item until NotesView.tsx exists).
    Audit 2026-10-26 (installed 0a87249): the detail view prints the raw state 'complete' (Meetings.tsx:428), cards count 'segments' that are 30 s windows (:320-321), the detail page 'does not refresh itself' (:452), and Write/Retry notes exist only on the card (:339-350). Notes are render-only (NotesView.tsx). Rewrite is NEW yap-audit-rewrite-notes. See docs/audits/meeting-notetaker-audit-2026-10-26.md issue #12.
  `,
  acceptance: `
    grep -qE 'canRetry\\([^)]*audioKept' desktop/src/meetings/progress.ts
    ! grep -q 'canRetry(m.state)' desktop/src/views/Meetings.tsx
    ! test -e desktop/src/meetings/viewState.ts
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/meetings
    npm test
    npm run build
  `,
})

ITEMS.push({
  id: 'yap24-NT7', prompt: 'yap24-NT', branch: 'loop/yap24-nt7-pill-and-tray-follow-the-meeting-after-stop', gated: 'panel',
  title: 'The pill and the menu bar follow the meeting after stop: transcribing 42%, notes ready, or what went wrong',
  preflight: `
    grep -q "notes-ready" desktop/src/pill/meeting.ts
    grep -q "notes-ready" desktop/src/pill/meeting.test.ts
    grep -q "MEETING_PROGRESS_EVENT" desktop/src/pill/meeting.ts
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26.
    DEPENDS: yap24-NT2, yap24-NT6

    EVIDENCE
      - pill/MeetingBadge.tsx:2 "the pill's persistent recording state (YV95)" — the badge knows
        recording and nothing after it; the moment a meeting stops the pill goes quiet while the
        work (now real, yap24-NT2) happens.
      - The tray meeting item toggles start/stop only (meeting_control.rs:733-757 toggle).

    DO
      1. pill/meeting.ts gains post-stop phases: processing (with percent), notes-ready (holds
         until clicked or 10 s), failed (sentence). Driven by the pipeline event from NT2 — no
         polling.
      2. Both characters render them through the shell (Y5-K owner decision: the shell owns
         phases, a character only owns art).
      3. The tray shows "Meeting notes ready — Open" and a macOS notification (the notification
         plugin is already initialised) when a meeting completes while Yap is in the background.
      4. Clicking notes-ready opens that meeting in the Meetings view (navigate event).

    Tests: pill/meeting.test.ts — event sequence recording -> processing 0..100 -> notes-ready;
    failure path; a dictation during processing still shows the dictation phases (dictation
    outranks the badge).

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - This is the notetaker wow moment: "Meeting notes ready - Open" arrives while the user is doing something else. Today the pill goes quiet
        after stop (MeetingBadge knows recording and nothing after it; pill/meeting.ts has no post-stop phase).
      - CHANGE: consume the EXISTING meeting:progress event (MEETING_PROGRESS_EVENT, emitted by meeting_pipeline.rs:52 and typed in
        meetings/progress.ts:15). No new event, no polling; the acceptance fails on a setInterval in pill/meeting.ts. The notes-ready phase
        reads its state from the same payload (state complete) and #250's summary-ready state.
      - Pill matrix: once scripts/pill-matrix.mjs exists (yap24-UI3), run node scripts/pill-matrix.mjs --only processing and --only notes-ready
        and attach the PNGs to the PR (never commit them).
  `,
  acceptance: `
    grep -q "MEETING_PROGRESS_EVENT" desktop/src/pill/meeting.ts
    ! grep -n "setInterval" desktop/src/pill/meeting.ts
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/pill
    npm test
    npm run build
  `,
})

// yap24-NT8 — CUT 2026-10-26 by the Senior Panel plan-audit (object removed; the harness has no retired/status field, removal is the mechanism).
// Why: a duplicate of the ungated yap-notetaker-detect (same calendar_prompt_policy.rs, same EventKit prompt; NT8 default OFF, detect default ON)
// that also adds a permanent Calendars TCC permission and fixes nothing in stop -> transcript -> notes. One owner now: yap-notetaker-detect,
// whose calendar signal is opt-in and whose app signal needs no permission. The original spec is in git history (commit before 2026-10-26).

ITEMS.push({
  id: 'yap24-NT9', prompt: 'yap24-NT', branch: 'loop/yap24-nt9-notetaker-end-to-end-proof', gated: 'panel',
  title: 'Phase-closing proof: one headless command runs a two-track fixture through capture, transcript, notes and export',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/tests/meeting_pipeline_residual.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap24-NT2 residual (guards) has not landed; run the review pass, then build NT9"; exit 0; fi; grep -q 'transcribe-meeting' desktop/src-tauri/src/cli.rs && test -x scripts/notetaker-e2e.sh && grep -q "yap24" docs/MEETING-DEMO.md )
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26.
    DEPENDS: yap24-NT0 (the provisioned model + corpus the proof runs against), yap24-NT1-NT4,
    yap24-NT6, yap24-NT7 (NT5 and NT8 are DEFERRED — see their own Panel revisions
    above; NT9 does not wait on either)

    Panel revisions 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, BLOCKING, GROUNDED — 4 of 5 seats):
      - The phase-closing proof must actually run. Acceptance today is test -x + bash -n +
        two cargo tests — notetaker-e2e.sh itself is NEVER executed, and even when it is, it
        SKIPs (exit 0) on ANY throwaway YAP_DATA_DIR because a fresh scratch root never has a
        model (app_paths.rs:71 puts models under root.join("models")). "Reuse the repo's meeting
        fixtures, do not add audio" cannot be honored either: tests/fixtures has one 2.5 s mono
        WAV — no two-track meeting audio exists in the repo. This repeats the exact
        "verification that verifies nothing" failure this whole chain exists to end.
      - FIX: acceptance runs YAP_E2E_REQUIRE=1 bash scripts/notetaker-e2e.sh for real (added
        below). The script takes YAP_MODEL_DIR / YAP_EVAL_CORPUS overrides pointing at a
        lane-cached, sha256-verified parakeet model and a two-track fixture; when
        YAP_E2E_REQUIRE=1 is set, SKIP becomes a hard failure (non-zero exit) instead of exit 0.
        (Round-2 verify: SKIP-as-pass is removed entirely — see DO step 1.)
      - Commit ONE small generated two-track fixture under
        tests/fixtures/meeting-two-track/ (macOS say -> afconvert to 16 kHz mono, a few
        seconds per track — the same recipe the repo already used for quick-brown-fox-16k.wav),
        so the wired-path assertions (segments > 0 on BOTH tracks) have something to run against
        with no network and no owner-gated corpus.

    WHY: the 2026-08 loops closed the notetaker phases with 135 green meeting tests while the
    pipeline had no caller (see the header of this file). A phase is closed by the behaviour, not
    by the mechanisms.

    DO (step 1 rewritten by the Panel round-2 verify 2026-09-26 — the gate EXECUTES, it never skips)
      1. scripts/notetaker-e2e.sh (set -euo pipefail): REQUIRES YAP_DATA_DIR (a throwaway root —
         refuses the default root, like --smoke), YAP_MODEL_DIR and YAP_EVAL_CORPUS; a missing or
         unset one is a non-zero exit with the sentence naming it and the command that provisions
         it (scripts/provision-notetaker-eval.sh, yap24-NT0). It links the pinned parakeet Q8_0
         file from YAP_MODEL_DIR into <YAP_DATA_DIR>/models/ (symlink or cp -c clone, never a
         write into YAP_MODEL_DIR), then runs the release binary with --transcribe-meeting on the
         committed two-track fixture tests/fixtures/meeting-two-track/ (generated per the Panel
         revision above: say -> afconvert, 16 kHz mono, a few seconds per track), and also on
         YAP_EVAL_CORPUS/two-track-ordering. It asserts: row state complete, segments > 0 on BOTH
         tracks, summary present when the summary model is installed or summary_status
         needs_model when it is not, Markdown export contains Summary / Action items /
         transcript. Any miss is a non-zero exit. There is NO skip path (the script never prints
         SKIP — the acceptance greps for it) and no exit-0-on-SKIP branch: under YAP_E2E_REQUIRE=1 (the loop always sets it) a missing input is a failure,
         and without it the script still fails — it only adds the provisioning hint.
      2. docs/MEETING-DEMO.md gains a "yap24 — what a meeting does now" section with the
         command and a pasted run.
      3. meeting_matrix.rs: every row whose call site now exists is Test, with its test named.

    NOT: no microphone, no TCC, no window. This is the headless half; the human half (one real
    meeting on Wilson's Mac) is listed in the PR body as the remaining manual check.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - THIS ITEM NOW OWNS THE FLAG: the proof runs the release binary with --transcribe-meeting, which is absent from cli.rs (grep finds 0), and
        its former owner (NT2 DO step 6) is skipped as already-done. Step 0 of the DO: add --transcribe-meeting <mic.wav> [--sys <sys.wav>]
        beside --transcribe-file in cli.rs, running the same pipeline worker against a scratch state root (YAP_DATA_DIR required, like
        --smoke) and printing the segment count and the final state. The preflight and acceptance both grep it.
      - The summary assertion follows #250's states: with the notes model linked from YAP_MODEL_DIR the Markdown export contains the notes
        sections; without it, whatever pending state #250 defines, never an error and never the retired needs_model string.
      - Depends on NT0, NT1, NT2 (residual), #250 (the preflight holds on the NT2 residual test file).
    Audit 2026-10-26: the audio box is offline-first. The CLI should run NEW yap-audit-eval-call-bleed's call-bleed-2voice fixture and print its metrics (echo_dup_rate, mid-sentence ends, median segment s, notes owner accuracy, Summary verbatim ratio). See docs/audits/meeting-notetaker-audit-2026-10-26.md section 9.
  `,
  acceptance: `
    grep -q 'transcribe-meeting' desktop/src-tauri/src/cli.rs
    test -x scripts/notetaker-e2e.sh
    bash -n scripts/notetaker-e2e.sh
    ! grep -q 'SKIP' scripts/notetaker-e2e.sh
    YAP_DATA_DIR="$(mktemp -d)/yap-state" YAP_MODEL_DIR="$HOME/code/wilson-voice-loop/cache/models" YAP_EVAL_CORPUS="$HOME/yap-eval-corpus/meetings" YAP_E2E_REQUIRE=1 bash scripts/notetaker-e2e.sh
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib meeting_matrix
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_pipeline_wired
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_pipeline_residual
  `,
})

ITEMS.push({
  id: 'yap24-NT10', prompt: 'yap24-NT', branch: 'loop/yap24-nt10-meeting-survives-sleep-and-lid-close', gated: 'panel',
  title: 'A meeting survives a lid close or sleep: IOKit holds the sleep, the journal finalizes, and the row ends partial with one plain sentence instead of staying recording forever',
  notes: 'NEW item added by Senior Panel synthesis 2026-09-26T17:35:00Z (User/wildcard seat, BLOCKING — grounded). The audit counted Y1-B (register NSWorkspaceWillSleepNotification / IORegisterForSystemPower) as already-done, but its own pre-flight only matches a COMMENT in power.rs, not a real observer call site (meeting_matrix.rs row 16 still reads PolicyOnly{absent_call_site:"NSWorkspaceWillSleepNotification"}; grep -c for that literal returns 4 on main today). only:[\'yap24-NT\'] never reaches Y1-B, so without this item a lid-close mid-meeting (walking between rooms, closing the laptop) leaves the row stuck in "recording" with no finalize, and yap24-NT1\'s reconcile only handles a crash, not a clean sleep. Corrects docs/ARCHITECTURE-AUDIT-2026-09-26.md §10\'s already-done count from 2 to 1 (Y4-D only).',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/tests/matrix_row16_sleep_wake.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 239 (sleep/wake observer) has not landed; land it, then build NT10"; exit 0; fi; grep -v '^[[:space:]]*//' desktop/src-tauri/src/power.rs | grep -q 'IORegisterForSystemPower' && test -f desktop/src-tauri/tests/meeting_survives_sleep.rs )
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26.
    DEPENDS: yap24-NT1 (reuses its capture-journal recovery path; this item is the sleep-triggered
    twin of NT1's crash-triggered reconcile)

    EVIDENCE
      - power.rs mentions NSWorkspaceWillSleepNotification only in a comment (no registration
        call site exists in src/); a grep for WillSleep across power.rs and permissions.rs
        matches comments only.
      - meeting_matrix.rs row 16: PolicyOnly { absent_call_site: "NSWorkspaceWillSleepNotification" }.
      - meeting::recover_orphaned_meetings (meeting.rs:1599) already recovers a crash-abandoned
        journal — this item registers the real observer and calls the SAME recovery path on a
        clean sleep, rather than duplicating it.

    DO
      1. Register NSWorkspaceWillSleepNotification (and the IORegisterForSystemPower fallback
         already scoped in Y1-B) in power.rs. On fire, if a meeting is recording: finalize the
         capture journal exactly as recover_orphaned_meetings does for a crash, and mark the row
         paused_by_sleep (a new, honest state distinct from partial — the recording was stopped
         cleanly by the OS, not lost).
      2. On wake (NSWorkspaceDidWakeNotification), a paused_by_sleep meeting is left for the user
         to resume (Start again, producing a second meeting) or stop-and-finalize; it is NEVER
         silently resumed into the old row.
      3. yap24-NT6 renders paused_by_sleep with a one-sentence explanation and the same Retry/
         Export affordances as a normal complete meeting once transcribed.
      4. Update meeting_matrix.rs row 16 to Test once the call site and its test exist.

    NOT: no change to dictation's own sleep handling (already shipped/separate). No new TCC
    permission — NSWorkspace notifications need none.

    Tests (tests/meeting_survives_sleep.rs): simulate WillSleep mid-recording -> row finalizes to
    paused_by_sleep with both WAV paths intact; WillSleep with no active meeting is a no-op.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - API CHOICE: NSWorkspaceWillSleepNotification does NOT delay sleep; IORegisterForSystemPower with kIOMessageSystemWillSleep and an
        IOAllowPowerChange acknowledgement does. The IOKit path is now PRIMARY (the old DO had it as a fallback). Acknowledge only after the
        journal is finalized. Per Apple's IOKit power-management docs; not re-fetched this run, so the builder reads the current docs first.
      - KEEP PR #239's NSWorkspace observer registration (and its SleepWakeHub); this item subscribes to the same hub and adds the IOKit hold. The
        preflight holds this item until #239's tests/matrix_row16_sleep_wake.rs exists. Dep: #239 landed.
      - NO NEW STATE: the paused_by_sleep state in DO 1, 2, 3 is VOID. Finalize the row to partial with the sentence "Stopped when the Mac went to
        sleep." and let the existing pipeline enqueue partial rows (NT2 R2 enqueues {transcribing, partial}). Wake never silently resumes the old row.
        The acceptance fails if the string paused_by_sleep appears anywhere.
      - Test (tests/meeting_survives_sleep.rs): inject kIOMessageSystemWillSleep mid-recording and assert the row ends partial, both WAV paths intact,
        the pipeline enqueued it, and the acknowledgement happened after finalize; with no active meeting it is a no-op that still acknowledges.
  `,
  acceptance: `
    grep -v '^[[:space:]]*//' desktop/src-tauri/src/power.rs | grep -q 'IORegisterForSystemPower'
    grep -q 'IOAllowPowerChange' desktop/src-tauri/src/power.rs
    ! grep -rn 'paused_by_sleep' desktop/src desktop/src-tauri/src
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_survives_sleep
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test matrix_row16_sleep_wake
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib meeting_matrix
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo clippy --all-targets --features custom-protocol
  `,
})

// yap24-NT11-0 REMOVED by Senior Panel 3 2026-10-26 (both seats said fold or kill). Its question ("is the bleed real, at a stable lag?") was answered by data on
// disk: text containment 0.71-0.89 on 3 calls, and acoustically a constant lag (155 of 157 confident windows on c6187806, 144 within 30 ms; GCC-PHAT lag 7.1 / 0.6 / -1.7 ms
// on the three real calls, no drift). Its PASS gate (envelope peak_ncc >= 0.5) was an unvalidated threshold on an unbuilt metric, its human half put the top CRITICAL
// behind a calendar dependency, and the mach-epoch persistence is not needed (the WAV sample index is the working shared clock, measured). Its one keep-line (finalize
// keeps <id>.tN.index.jsonl) now lives in yap24-NT11 DO 5. The original spec is in git history. Do not add it back to panelApproved.

ITEMS.push({
  id: 'yap24-NT11', prompt: 'yap24-NT', branch: 'loop/yap24-nt11-aec3-prepass-on-the-mic-track', gated: 'panel',
  title: 'A call on built-in speakers is transcribed once: an AEC3 pre-pass cancels the far side out of the mic track (derived file, raw WAV untouched), with a per-span Speaker label and a 3-word residue rule as the fallback',
  notes: 'REWRITTEN by Senior Panel 3 2026-10-26 (seat A built aec3 0.4.0 in scratch and ran it on the real stored WAVs; seat B the PM/User/AI seat). Earlier revisions (text-dedupe primary, bleed_score gate, persisted mach epochs, the output-route grep) are GONE, not superseded in place. yap24-NT11-0 is folded here and removed. The v1 blocker for the first-session walk: every remote line appears twice (Me and Them) until this lands. Build early.',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/tests/meeting_failed_audio_ceiling.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap24-NT1 residual (the purge test file meeting_failed_audio_ceiling.rs, whose purge must also remove the derived .aec.wav) has not landed; run the review pass, then build NT11"; exit 0; fi; test -f desktop/src-tauri/tests/meeting_cross_track_bleed.rs && grep -q 'fn aec_prepass' desktop/src-tauri/src/meeting_aec.rs && grep -q 'suppressed_as_echo' desktop/src-tauri/src/db.rs )
  `,
  spec: `
    Panel 3 2026-10-26: REWRITTEN, APPROVED. Class: STANDARD (not SEC). Why: it reads and writes audio files in the meetings directory only; it adds no permission, no network, no capture-path
    change and no IPC surface. The one supply-chain change (a new crate) is covered by the existing supply_chain.rs pin test, which this item extends. Re-classify as SEC only if the build touches
    permissions.rs, entitlements, or the capture code in syscapture.rs / record.rs.
    DEPENDS: yap24-NT1 (purge and delete own the derived file), yap24-NT2 (the pipeline whose track-0 windows read the cleaned WAV).

    WHY (audit issue #1, CRITICAL). On built-in speakers the far side is transcribed twice: 3 of 3 real virtual calls show 0.71-0.89 of the call track's word trigrams inside the mic track's
    same window; on c6187806 echo_dup_rate is 0.88 and the rendered both-track WER against a cloud reference is 0.838. The notes then give the other side's commitments to Me.

    DECISION (duel settled with evidence): the primary fix is an AEC3 pre-pass, NOT a text dedupe. Seat A MEASURED the pure-Rust aec3 crate 0.4.0 offline over the stored 16 kHz mono WAVs
    of c6187806 (mic = t0, system tap = t1), NS and AGC off: call-playing windows attenuated 25.5 dB at the median (p25 7.4 dB); local-only windows lose 0.17 dB (p90 0.20); the two stored
    WAVs are already aligned (lag 7.1 / 0.6 / -1.7 ms on the three real calls, constant across the file, no drift); 12.1 s for 31 minutes on an M4 Pro (about 20-25 s expected on M1 8 GB),
    streaming state tens of MB; it builds on arm64; MIT OR BSD-3-Clause; its dependencies (rustfft, num-complex, crossbeam) are already in desktop/Cargo.lock, so it adds ONE crate.
    A text dedupe alone cannot fix ASR of mixed double-talk and would delete real words; energy ducking kills double-talk; the webrtc-audio-processing crate is a second native C library and is REJECTED.
    VPIO (capture-time voice processing) stays rejected: it ducks the call's own playback, is irreversible, and replaces the shared cpal mic path; the offline system-track reference does the
    same job non-destructively.

    DO
      0. PRESERVE THE EVIDENCE FIRST. The audio of c6187806 is purged about 7 days after 2026-10-26 (complete meeting with segments). Before anything else copy its two WAVs (read-only cp,
         never move) into the private eval corpus ~/yap-eval-corpus/private/call-c6187806/ (outside the repo, never committed, never uploaded) and record only that path in the PR body.
      1. Cargo.toml: aec3 = "=0.4.0" (default features), exact pin like llama-cpp-2. NOTICE: add the aec3 entry (MIT OR BSD-3-Clause). tests/supply_chain.rs: add aec3 to the reachable-from-app
         expected set and assert the exact x.y.z pin with the existing helper. No other new crate may appear in Cargo.lock (the test diffs the lockfile package set against the committed one).
      2. New module src/meeting_aec.rs, everything behind two fns: fn aec_coupling(mic, sys) -> Option<lag_ms> (GCC-PHAT over the call-playing windows; Some only when a confident lag is found
         in at least 30% of the windows where the system track is playing) and fn aec_prepass(mic_wav, sys_wav, out_wav) -> Result (streaming, bounded memory, never a whole-meeting f32 buffer).
         The reference is shifted 20 ms earlier so a lag of -1.7 ms stays causal. It writes <audio>.t0.aec.wav ONCE (idempotent: a second run is a no-op; written to a temp name then renamed);
         the raw WAV is never modified and a re-transcribe re-derives it.
      3. MeetingAsr reads <audio>.t0.aec.wav for track 0 (instead of the raw mic) only when kind is virtual AND a t1 track exists AND aec_coupling is Some. Headphones (no coupling) and in-person
         meetings read the raw mic unchanged. The derived file is purged and deleted WITH the raw WAVs by yap24-NT1's purge and the meeting-delete cascade (extend both; test it); it is never kept alone.
      4. FALLBACK, after AEC, for what it cannot fix: (a) where the system track is all-zero or missing for a span (a tap hole, a reopen gap; on the real calls: 4456 after 690 s and 61d7, whose far
         side is mostly NOT in the tap, so no AEC or dedupe can fix it), label the mic speech "Speaker" (unattributed) through meetings::speaker_label, never "Me", and exclude it from Me-owned
         notes items; (b) the residue rule: a run of 3 or more CONSECUTIVE folded mic words that matches a system run within +-1.5 s is echo residue; an isolated single-word match is always kept;
         for segment spans, remove the matched run from the mic span and drop the span only if the residue has 0 or 1 non-stopword tokens, else keep the residue flagged mixed. Text agreement is a
         drop condition only where AEC ran or the span has no system audio; it never deletes on a single word. Each suppressed mic span is persisted as its own row flagged
         suppressed_as_echo=1 (excluded from render, FTS, export and notes input) via the NEXT free versioned migration step, never an ad hoc ALTER; the Transcript shows one line "N lines hidden as
         speaker echo - Show" revealing them inline (a one-line TS change in the transcript list). Record route/reopen boundaries against each track's finalized-sample position, never raw host_ns.
         Tests assert labels through speaker_label itself, never through hardcoded "Me"/"You" literals.
      5. Finalize keeps <id>.tN.index.jsonl (it no longer deletes the per-track anchors); cleanup only through NT1's purge and the delete cascade. (Carried from the removed NT11-0.)
      6. DEPENDENT (only when the Rewrite notes button from yap-audit-rewrite-notes is on main at build time; otherwise that item carries this step when it lands): when audio_kept=1 offer
         "Re-transcribe and rewrite", so an old echo transcript can be re-derived through the AEC pre-pass and its notes rewritten.
      7. Create scripts/eval/aec-real-call.sh (executable; described under ACCEPTANCE ON REAL AUDIO below). Its output is numbers only, never transcript text.
      8. NUMBERS to docs/BUDGETS.md under "AEC pre-pass (real call)" and "AEC pre-pass (synthetic)": attenuation p50 and p90, local-only loss p90, lag, wall time and peak RSS for the pre-pass.
    NOT: no capture-path change, no new TCC permission, no change to dictation, no second native C library, no VPIO, no mach-epoch persistence (the WAV sample index is the shared clock, measured), no
    merge of the two tracks' transcripts here (ordering by start time belongs to yap-audit-utterance-segments), no NT5 diarization (NEXT-RELEASE note: NT5 on the call track would give Them A / Them B).
    Known limit, stated honestly: when two people share the laptop mic, AEC correctly keeps both voices on the mic track; their owners are handled by yap-audit-notes-guards ("Someone here"), not here.

    ACCEPTANCE ON REAL AUDIO (numbers only, run from the audio on disk in a mktemp dir, never the data dir): scripts/eval/aec-real-call.sh copies the two WAVs of c6187806 from
    ~/yap-eval-corpus/private/call-c6187806/ into a mktemp dir, runs the AEC pre-pass, runs wilson-voice --transcribe-file <aec.wav> --timed windowed at 30 s (NT9's --transcribe-meeting when it lands),
    and prints ONLY numbers via scripts/eval/compare-reference.py --tracks against the cloud reference (YAP_AEC_REAL_REFERENCE). With YAP_EVAL_REQUIRE=1 a missing WAV or reference is a failure, not a skip.
    Gates:
      (a) echo_dup_rate <= 0.10 (today 0.88);
      (b) owner (reference Speaker B) share in the Me track >= 0.60 (was 0.64) and local Speaker D >= 0.70 (was 0.75);
      (c) remote Speakers A and C share in the Me track <= 0.10 (was 0.66 and 0.75);
      (d) user-visible (both tracks rendered) WER <= 0.25 (today 0.838);
      (e) acoustic guard with no ASR: local-only windows attenuation p90 <= 1.0 dB loss, and call-playing windows p50 >= 15 dB.
    Tier A (no model) also runs on the synthetic call-bleed-2voice fixture and on a headphones control: attenuation p50 >= 15 dB on call windows, nothing removed on the control, plus the unit
    cases of step 4 (isolated "yeah" kept; 7-word mic commitment inside a 25-word overlapping system sentence keeps its residue; tap-hole span labelled via speaker_label; route change with a mic
    reopen drops nothing within +-1 s of the seam without evidence; every word that exists only on the mic track survives).
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/meeting_cross_track_bleed.rs
    grep -q 'fn aec_prepass' desktop/src-tauri/src/meeting_aec.rs
    grep -q 'fn aec_coupling' desktop/src-tauri/src/meeting_aec.rs
    grep -q 'aec3 = "=0.4.0"' desktop/src-tauri/Cargo.toml
    grep -q 'aec3' NOTICE
    test -x scripts/eval/aec-real-call.sh
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test supply_chain
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_cross_track_bleed
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_failed_audio_ceiling
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_pipeline_wired
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib meeting
    YAP_DATA_DIR="$(mktemp -d)/yap-state" YAP_EVAL_REQUIRE=1 bash ../../scripts/eval/aec-real-call.sh
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo clippy --all-targets --features custom-protocol
  `,
})

// ── 03-yap24-hotkeys-permissions-os.mjs ───────────────────────────────────
// yap24-OS — FASTER HOTKEY, TRUER PERMISSIONS, BETTER OS CITIZEN. Written 2026-09-26 by the
// architecture audit (docs/ARCHITECTURE-AUDIT-2026-09-26.md §4 and §6). Wilson, 2026-09-26: "the
// app should work better with the operating system, the kernel, the permissioning system; the
// hotkeys faster and smoother; way better than Wispr Flow."
//
// Rules that bind every item here: app-sandbox stays false (memory
// feedback_never_sandbox_utility_apps); the bundle id and the data dir are never renamed (TCC and
// history); no new vendor; headless tests only.
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap24-OS1', prompt: 'yap24-OS', branch: 'loop/yap24-os1-arm-capture-on-key-down-zero-lost-words', gated: 'panel',
  title: 'The first word is never lost and the start is 280 ms sooner: capture arms on key-down and a tap throws the pre-roll away',
  preflight: `
    grep -q "speculative" desktop/src-tauri/src/ptt_macos.rs
    test -f desktop/src-tauri/tests/ptt_speculative_arm.rs
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26, DEFERRED to the next release (not in panelApproved).
    DEPENDS: none

    Panel revisions 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, HIGH, GROUNDED, 2+ seats):
      - Arm speculatively ONLY when the persistent stream is already warm (inside IDLE_CLOSE).
        On a COLD stream, key-down handling is fn/Globe first (flags-changed), and whether this
        is a tap, hold or chord (fn+arrow, fn+Delete, Globe for emoji/input-switch) is only known
        once the gesture resolves — arming on every cold key-down would open the mic, light the
        orange indicator and force a Bluetooth headset into its HFP call profile on plain text
        editing and input-source switching, dozens of times an hour. On a cold stream: begin the
        device open at key-down but do not start delivering audio until the gesture resolves;
        add an explicit Interrupted -> discard branch (today only "released before TAP_MAX_MS"
        discards) and tests "fn+arrow opens no stream" / "a tap on a cold stream leaves it closed".
      - Measure BOTH the warm-stream and cold-stream press-to-first-sample paths separately in
        docs/BUDGETS.md — they are not the same number.

    EVIDENCE
      - ptt_macos.rs:32 HOLD_ARM_MS = 280; :488-509 a thread sleeps 280 ms after key-down and only
        then fires Start. The comment (YV38) is right that the gesture needs the window — but
        capture does not have to wait for the decision.
      - record.rs:390 arms the persistent capture worker only on Start; there is no pre-roll, so
        speech in the first ~280 ms after the press is not in the take.
      - record.rs:1316 IDLE_CLOSE = 60 s: after a minute idle the next take pays a cold stream
        open (bounded by ARM_TIMEOUT 3 s, record.rs:1312), and with a Bluetooth headset a profile
        switch.

    DO
      1. On fn / fn⌃ key-down: arm capture immediately in SPECULATIVE mode (buffering, pill still
         idle). At HOLD_ARM_MS: still held -> promote to a real take, keeping the buffered audio
         from key-down (the take starts at the press, not 280 ms later). Released before
         TAP_MAX_MS -> discard the buffer, then run the existing tap / double-tap logic.
      2. Measure (latency.rs) press->first-sample for warm and cold streams and record both in
         docs/BUDGETS.md; the pill's "listening" flips at promotion, unchanged.
      3. Do not keep the mic open longer than today: speculative arming reuses the same stream and
         the same IDLE_CLOSE; the orange mic indicator behaviour is unchanged.

    Tests (tests/ptt_speculative_arm.rs, pure state machine): hold -> promote with pre-roll
    retained; tap -> discard, no take; double-tap -> hands-free with no leftover buffer; a
    promote after a cold open still starts at the press timestamp.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - DEFER: next release. It touches ptt_macos.rs right after #249 stabilised the hotkeys, and it needs a measured number, not a guess.
        Pre-mortem guard recorded for when it is built: a speculative arm that opens the mic on every fn press lights the orange dot dozens of
        times an hour and flips AirPods to HFP, so warm-stream only plus a soak on Wilson's Mac before landing.
      - STALE REFERENCES FIXED: after #249, HOLD_ARM_MS is at ptt_macos.rs:142 and TAP_MAX_MS at :146 (the evidence cites :32); a start goes through
        HotkeyArbiter (hotkeys.rs). The speculative arm applies to the fn SOURCE only: the Carbon chord needs no pre-roll speculation.
  `,
  acceptance: `
    cd desktop && npm ci && cd src-tauri
    cargo test --features custom-protocol --test ptt_speculative_arm
    cargo test --features custom-protocol --lib ptt
    cargo test --features custom-protocol --lib latency
    cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-OS2', prompt: 'yap24-OS', branch: 'loop/yap24-os2-launch-at-login-via-smappservice', gated: 'panel',
  title: 'Launch at login through SMAppService.mainApp, the way macOS 13+ expects, instead of a LaunchAgent plist',
  preflight: `
    grep -q "SMAppService" desktop/src-tauri/src/lib.rs desktop/src-tauri/src/*.rs
  `,
  spec: `
    Panel: pending
    DEPENDS: none

    EVIDENCE
      - Cargo.toml:32-34 tauri-plugin-autostart "macOS LaunchAgent"; lib.rs:382-387 the autostart
        setting drives it (default OFF, correct).
      - Apple: SMAppService is macOS 13.0+ ("An object the framework uses to control helper
        executables that live inside an app's main bundle",
        https://developer.apple.com/documentation/servicemanagement/smappservice), and
        SMAppService.mainApp is "the main application as a login item", macOS 13.0+
        (https://developer.apple.com/documentation/servicemanagement/smappservice/mainapp).
      - Yap's floor is macOS 12.0 (tauri.conf.json bundle.macOS.minimumSystemVersion).

    DO: on macOS 13+ register/unregister SMAppService.mainApp via objc2 (the crate family the app
    already uses) and read its status back into Settings (enabled / requires approval / not
    registered — the "requires approval" case gets a deep link to Login Items). On 12.x keep the
    plugin path. Migrating users: if a LaunchAgent from the plugin exists and the setting is on,
    register mainApp and remove the agent once. Tests: the pure status->copy mapping and the
    migration decision table.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - DEFER: after the first 100 users. The autostart plugin works; there is no user-visible bug. SMAppService needs an objc2-service-management
        crate that is not in Cargo.lock today, and macOS 12.x (the stated floor) still needs the plugin path, so it is a dual path for no visible gain.
  `,
  acceptance: `
    cd desktop && npm ci && cd src-tauri
    cargo test --features custom-protocol --lib autostart
    cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-OS3', prompt: 'yap24-OS', branch: 'loop/yap24-os3-tcc-truth-pass-input-monitoring-and-copy', gated: 'panel',
  title: 'Permission truth: the hotkey asks for Input Monitoring by name, the purpose strings describe today\'s app, and a grant is noticed on focus',
  preflight: `
    grep -q "CGPreflightListenEventAccess" desktop/src-tauri/src/permissions.rs desktop/src-tauri/src/ptt_macos.rs
    ! grep -q "with Whisper" desktop/src-tauri/Info.plist
  `,
  spec: `
    Panel: pending
    DEPENDS: PERM-E (merged #167)

    EVIDENCE
      - ptt_macos.rs:314-323 creates a LISTEN-ONLY tap at the HID location and on failure logs
        "CGEventTapCreate failed — enable Accessibility". A listen-only tap is gated by Input
        Monitoring (permissions.rs:403 says as much); the copy sends people to the wrong pane.
      - Apple: CGPreflightListenEventAccess / CGRequestListenEventAccess, macOS 10.15+
        (https://developer.apple.com/documentation/coregraphics/cgpreflightlisteneventaccess(),
        https://developer.apple.com/documentation/coregraphics/cgrequestlisteneventaccess()).
      - Info.plist NSMicrophoneUsageDescription: "…transcribe dictation with Whisper" — the
        default engine is Parakeet (src/catalog.json), and meetings also use the mic.
      - permissions.rs:536-542 revocation watch polls every 45 s (30 s floor). A grant made in
        System Settings is not seen until the next tick.

    DO
      1. Input Monitoring: preflight with CGPreflightListenEventAccess, request with
         CGRequestListenEventAccess from the onboarding permission step, and route the tap-failure
         copy + deep link (Privacy_ListenEvent, permissions.rs:811) to it.
      2. Purpose strings: mic = "Yap listens only while you hold the dictation key or record a
         meeting, and transcribes on this Mac." (final copy: panel). No engine names.
      3. Re-check every grant on app activation and on every hotkey press (cheap, read-only
         calls), keeping the 45 s poll as the backstop.
    Tests: permission copy table; the activation re-check path is a pure function over grant
    snapshots.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - RESIDUAL ONLY: of the three DO steps two already shipped (Input Monitoring preflight/request and the activation re-check). What remains is
        the first TCC prompt: Info.plist:16 NSMicrophoneUsageDescription still reads "... to transcribe dictation with Whisper", which names the wrong
        engine at the user's first impression. Replace it with the panel copy: "Yap listens only while you hold the dictation key or record a
        meeting, and transcribes on this Mac." The acceptance greps both the absence of the old string and the presence of the new one.
        Ship this line even if everything else slips.
  `,
  acceptance: `
    ! grep -q "with Whisper" desktop/src-tauri/Info.plist
    grep -q 'transcribes on this Mac' desktop/src-tauri/Info.plist
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd src-tauri
    cargo test --features custom-protocol --lib permissions
    cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-OS4', prompt: 'yap24-OS', branch: 'loop/yap24-os4-hardened-runtime-entitlement-diet', gated: 'panel',
  title: 'Hardened-runtime diet: prove whether allow-jit and allow-unsigned-executable-memory are needed, and drop what is not',
  preflight: `
    ( if ! ( test -x scripts/qa/release-smoke.sh ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 242 (release smoke in sign-local.sh) has not landed; land it, then build OS4"; exit 0; fi; ! grep -q "com.apple.security.cs.allow-jit" desktop/src-tauri/Entitlements.plist && test -x scripts/qa/entitlement-diet.sh )
  `,
  spec: `
    Panel: pending (SECURITY-class: the signing path)
    DEPENDS: SEC-A (merged #165 — stable signing identity)

    Panel revisions 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, HIGH, GROUNDED, 2 seats):
      - The experiment as specified cannot fail: sign-local.sh's codesign call has no
        "--options runtime", so the hardened runtime is OFF for every variant and cs.allow-jit /
        cs.allow-unsigned-executable-memory are inert either way — every removal passes
        vacuously, which could strip something the NOTARIZED (runtime-enabled) release actually
        needs. Sign each variant with "--options runtime --timestamp" under the real Developer ID
        identity and assert codesign -d --entitlements shows flags=0x10000(runtime) before
        trusting a pass/fail. Run the SIGNED binary's real work per variant (in-process
        transcribe-cpp Metal decode via --transcribe-file, plus --transcribe-meeting once NT2
        exists), against a scratch YAP_DATA_DIR with the lane's cached model linked in — an empty
        scratch root never reaches Metal/ASR at all. Give the yap-polish / yap-diarize sidecars
        their own entitlement files and drop --deep from sign-local.sh.

    EVIDENCE
      - Entitlements.plist carries device.audio-input, app-sandbox=false, cs.allow-jit and
        cs.allow-unsigned-executable-memory. The last two weaken the hardened runtime for the whole
        process. Whether ggml-Metal, llama.cpp (in the yap-polish sidecar, a separate binary) or
        WKWebView (JIT runs in the WebContent process) needs them in the APP binary is UNVERIFIED.

    DO: build a signed (Apple Development, sign-local.sh) .app with each entitlement removed in
    turn; run the headless smoke (--smoke with YAP_DATA_DIR, --transcribe-file on the fixture) and
    record pass/fail per variant in docs/RELEASE.md. Remove every entitlement whose removal passes.
    app-sandbox stays false. Never ad-hoc sign (resets TCC).

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - THE EXPERIMENT COULD NOT FAIL: its acceptance was greps plus --lib smoke, and sign-local.sh has no --options runtime, so the hardened runtime
        was off for every variant and every removal passed vacuously. THE GATES NOW: sign-local.sh carries --options runtime (grepped); a new
        scripts/qa/entitlement-diet.sh signs one variant per removed entitlement and, for EACH signed variant, (a) asserts
        codesign -dv shows flags=0x10000(runtime), exiting non-zero otherwise, and (b) runs --transcribe-file on that signed binary against a scratch
        YAP_DATA_DIR with the lane's cached model linked in (an empty scratch root never reaches Metal). The acceptance greps both strings in the
        script AND in docs/RELEASE.md, and runs the script. Give the yap-polish and yap-diarize sidecars their own entitlement files and drop --deep.
      - Expectation to test, not assume (CTO seat): WKWebView JIT runs in the com.apple.WebKit.WebContent XPC process and llama.cpp/ggml Metal compiles
        shaders through MTLDevice, not JIT pages, so both entitlements are probably removable. Unproven until the signed-runtime run passes.
      - Dep: PR #242 (it edits sign-local.sh); the preflight holds this item until scripts/qa/release-smoke.sh exists.
  `,
  acceptance: `
    grep -q "<false/>" desktop/src-tauri/Entitlements.plist
    grep -q "com.apple.security.app-sandbox" desktop/src-tauri/Entitlements.plist
    grep -q "Entitlement audit" docs/RELEASE.md
    grep -q 'flags=0x10000(runtime)' docs/RELEASE.md
    grep -q -- '--options runtime' scripts/sign-local.sh
    test -x scripts/qa/entitlement-diet.sh
    bash -n scripts/qa/entitlement-diet.sh
    grep -q 'flags=0x10000(runtime)' scripts/qa/entitlement-diet.sh
    grep -q -- '--transcribe-file' scripts/qa/entitlement-diet.sh
    YAP_DATA_DIR="$(mktemp -d)/yap-state" YAP_MODEL_DIR="$HOME/code/wilson-voice-loop/cache/models" bash scripts/qa/entitlement-diet.sh
    cd desktop && npm ci && cd src-tauri
    cargo test --features custom-protocol --lib smoke
  `,
})

ITEMS.push({
  id: 'yap24-OS5', prompt: 'yap24-OS', branch: 'loop/yap24-os5-idle-wakeups-event-driven', gated: 'panel',
  title: 'Idle means idle: the fixed polls (Secure Input 2 s, pill space-keeper, permission watch) are consolidated and measured',
  preflight: `
    test -f desktop/src-tauri/tests/idle_timer_budget.rs
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26, DEFERRED to the next release (not in panelApproved).
    DEPENDS: yap24-PILL3 (Secure Input state consumer), Y10-F (idle RAM/CPU publication)

    Panel revisions 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, HIGH — REJECTS part of
    yap24-PILL3's original plan, GROUNDED):
      - Do NOT move the Secure Input check to key-down + a >=10 s backstop, which yap24-PILL3
        proposed and which this item's "consolidate the polls" framing would otherwise adopt:
        under Secure Input the fn PTT event tap is BLIND and never receives that key-down, so the
        only signal left would be the slow backstop — worsening detection from ~4 s to as much as
        ~20 s, in both directions (on and off). KEEP the 2 s poll; it is one Carbon flag read.
        Exempt it BY NAME from this item's "every wakeup needs a reason" table, citing
        secure_input.rs:41's own comment on why 2 s is the interval. Spend the consolidation
        effort on the ioreg owner-PID spawn (secure_input.rs:176) instead — run it only on a
        false-to-true transition, not on every poll tick.

    EVIDENCE
      - secure_input.rs:48 POLL_INTERVAL = 2 s; its own doc (secure_input.rs:41) says macOS
        publishes no notification for Secure Input, so a poll is legitimate — but it runs even
        when no hotkey press is pending.
      - float_pill.rs:47 a space-keeper tick re-asserts the panel's dock and level.
      - permissions.rs:542 WATCH_INTERVAL 45 s; lib.rs:5283 hygiene telemetry interval.
      - Yap idles at ~135 MB RSS (yap23 log, pid 87890) — good; wakeups were never measured.

    DO: one table of every periodic timer in the app (name, interval, why, what event could
    replace it) in docs/BUDGETS.md; [STRUCK 2026-10-26: this sentence contradicted the Panel revision above] secure_input POLL_INTERVAL stays 2 s and is exempt BY NAME in idle_timer_budget; the ioreg owner-PID spawn (secure_input.rs:176) runs on a false-to-true edge only; the space-keeper runs on NSWorkspace active-space-change and
    screen-change notifications instead of a tick where the API allows; a test enumerates the
    registered timers and fails if any idle-time interval below 10 s is not on the named exempt list (secure_input::POLL_INTERVAL is the only entry).

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - The DO text still said to check Secure Input on key-down plus a >= 10 s backstop and to fail any interval below 10 s, against this item's own
        revision (keep the 2 s poll). Fixed in place above and gated: the acceptance requires the 2 s constant to stay and the exempt entry to exist by
        name, and requires a test named ioreg_spawns_only_on_rising_edge to run. DEFER: next release (idle-CPU hygiene, no user-visible wow).
  `,
  acceptance: `
    grep -q 'secure_input::POLL_INTERVAL' desktop/src-tauri/tests/idle_timer_budget.rs
    grep -q 'POLL_INTERVAL: Duration = Duration::from_secs(2)' desktop/src-tauri/src/secure_input.rs
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test idle_timer_budget
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib secure_input::tests::ioreg_spawns_only_on_rising_edge 2>&1 | grep -qE 'test result: ok\\. [1-9][0-9]* passed'
    cargo clippy --all-targets --features custom-protocol
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
