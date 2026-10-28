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
  name: "yap-overhaul-all-part-04",
  description:
    "Part 04 of the Yap (wilson-voice) overhaul, two or three builder lanes (args.lanes): land whatever is already gate-green, pre-flight on unmodified main (already-done items are skipped), build, open a labelled PR. The adversarial review, the independent second-Opus gate and the merge bar run afterwards in the same script with args {mode:'review'}. 53 items (Y4-A..Y8-D), 4 awaiting the Senior Panel. The gate is local (Actions is disabled by the account spending limit); the DMG is not in it.",
  phases: [
    { title: "Recon", detail: "two lane worktrees, one npm ci + one warm cargo build each, the loop-build label, ci-mode measured" },
    { title: "Y4-A", detail: "Formatting is on for a fresh install: the shipped default reaches the formatting stage" },
    { title: "Y4-I", detail: "Measure the polish stage against the real weights before anything is built on its envelope" },
    { title: "SEC-C", detail: "The polish model gets an install path, so the LLM stage can exist on a real machine" },
    { title: "Y4-C", detail: "Paragraphing: long speech becomes paragraphs by rule, not one wall of text" },
    { title: "Y4-D", detail: "Lists, nesting and spoken punctuation proven at the level the product ships, with the gaps filled" },
    { title: "Y4-E", detail: "Long-form gets polished: the 400-word cliff becomes a chunked pass that keeps the deadline" },
    { title: "Y4-F", detail: "App-aware formatting: the six modes change the output, proven per app, and auto mode picks correctly" },
    { title: "Y4-G", detail: "The user can see what formatting did and undo it in one key — the trust mechanism" },
    { title: "Y4-H", detail: "The formatting settings stop being engineer words, and every one of them persists" },
    { title: "yap-audit-vad-boundaries", detail: "Meeting chunks cut at pauses, not on a 30 s clock: the shipped pipeline passes the warm Silero VAD to MeetingAsr (it is hardwired to vad: None)" },
    { title: "yap-audit-utterance-segments", detail: "A stored meeting segment is an utterance (token-span pauses merged into 3-12 s same-track turns), not a whole 30 s window, so Me/Them turns can be read" },
    { title: "yap-audit-notes-guards", detail: "Notes never ship a re-transcript or a wrong owner: refuse a verbatim Summary, strip Me:/Them: prefixes, one section per item, at most two items per cited segment, no padded Key points, no echoed or mic-track owner claims, a repetition penalty on notes decoding, a Summary input that covers the whole meeting, and a title that is not a transcript fragment" },
    { title: "yap-audit-notes-strategy", detail: "DECISION and SHIPPED, measured: Yap writes notes items-first and composes the Summary last from the surviving items; the arms are measured on the NT4 eval over AEC-cleaned input" },
    { title: "yap-audit-rewrite-notes", detail: "Rewrite notes on any finished meeting, from the card and the detail view, through the retry command PR 250 already ships; the old notes stay until the new run succeeds" },
    { title: "yap-audit-asr-unload-before-notes", detail: "The ASR engine is unloaded before the notes sidecar starts (unless a dictation ran in the last 60 s), so the post-meeting peak is one model, not two" },
    { title: "yap-audit-meeting-timings", detail: "The log explains a slow meeting: decode ms per chunk, one RTF line per track, notes phase timings and rejection counts by reason, and a true retune line" },
    { title: "yap-audit-eval-call-bleed", detail: "The audio box, offline: a synthetic two-voice call-bleed fixture with planted commitments, echo-dup and notes-owner metrics in meeting_eval, and a numbers-only reference comparison script" },
    { title: "yap-audit-notes-quit-cancel", detail: "Quitting during notes interrupts the job with one line (\"resumes next launch\") instead of N failed MAP passes" },
    { title: "yap-audit-remote-speaker-names", detail: "DEFERRED to the next release: name remote speakers locally from the call app's active-speaker indicator (Accessibility); v1 uses the calendar 1:1 name inside yap24-NT4 instead. SPIKE, or record NO-GO" },
    { title: "Y3-A", detail: "Capture stops growing two unbounded Vecs — a long take spills to disk with a measured memory ceiling" },
    { title: "Y3-B", detail: "Long takes decode in windows with seam dedupe, reusing the meeting chunker instead of one 120s-capped call" },
    { title: "Y3-C", detail: "The pill reports real progress on a long take instead of an honest-looking lie" },
    { title: "Y3-D", detail: "Cancel works mid-decode, not just mid-recording — and a cancel never loses the audio" },
    { title: "DB-B", detail: "A crash or quit mid-long-take loses nothing — the take resumes or is offered back on next launch" },
    { title: "Y3-F", detail: "A declared maximum session length with a warning before it, instead of an undeclared cliff" },
    { title: "Y3-G", detail: "A measured latency and energy budget for long takes, published as a test that fails on regression" },
    { title: "Y2-A", detail: "The float window subscribes to license status — the wiring that does not exist" },
    { title: "Y2-B", detail: "A quiet trial numeral on the pill in both pill styles, in the 30px side dock too" },
    { title: "Y2-C", detail: "A refused hotkey press produces a pill state that explains itself, instead of a throttled notification" },
    { title: "Y2-F", detail: "A paying customer whose key stops verifying is never shown a price" },
    { title: "Y2-D", detail: "One click from the pill to purchase, reusing the existing Payment Link — no new money surface" },
    { title: "Y2-E", detail: "The menu-bar item says the same thing as the pill and the settings card, from one source" },
    { title: "SEC-B", detail: "The trial state machine gets the adversarial tests its own doc comment promises" },
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
    { title: "Y8-A", detail: "The hotkey suite: hands-free, cancel, copy-last, paste-last, scratchpad, with real validation rules" },
    { title: "DB-D", detail: "Scratchpad: a second window on a hotkey, dictate-into-note, versions — the half-built feature finished" },
    { title: "Y8-B", detail: "The pill becomes a bar: five affordance slots, each with a tooltip and a vertical-dock layout" },
    { title: "Y8-C", detail: "Optional earcons for start, stop, paste and achievement — off by default, Yappy-voiced" },
    { title: "Y8-D", detail: "Coaching nudges: the bar teaches the app, in Yappy's voice, without becoming nagware" },
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
const LOG = '/Users/wilsonguenther/Obsidian/Wilson-Brain/Projects/Loop-Logs/2026-09-12-yap-part04.md'
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
const PART = 'part-04'
/**
 * Stamped by build.mjs: item id -> the item FILE's ordinal (informational) or a pinned lane letter
 * ('A'|'B'|'C', item field `lane`). Only the PIN is read: since 2026-10-27 lanes are assigned PER PART
 * at run time (splitLanes below) — the old per-file round-robin left lane C idle for a whole part.
 */
const LANE_BY_ID = {"Y4-A":16,"Y4-I":16,"SEC-C":16,"Y4-C":16,"Y4-D":16,"Y4-E":16,"Y4-F":16,"Y4-G":16,"Y4-H":16,"yap-audit-vad-boundaries":17,"yap-audit-utterance-segments":17,"yap-audit-notes-guards":17,"yap-audit-notes-strategy":17,"yap-audit-rewrite-notes":17,"yap-audit-asr-unload-before-notes":17,"yap-audit-meeting-timings":17,"yap-audit-eval-call-bleed":17,"yap-audit-notes-quit-cancel":17,"yap-audit-remote-speaker-names":17,"Y3-A":18,"Y3-B":18,"Y3-C":18,"Y3-D":18,"DB-B":18,"Y3-F":18,"Y3-G":18,"Y2-A":19,"Y2-B":19,"Y2-C":19,"Y2-F":19,"Y2-D":19,"Y2-E":19,"SEC-B":19,"UPD-A":20,"UPD-B":20,"Y6-A":20,"PRIV-A":20,"Y6-B":20,"Y6-C":20,"DB-C":20,"PRIV-B":20,"Y6-D":20,"Y6-E":20,"Y7-A":21,"Y7-B":21,"Y7-C":21,"Y7-D":21,"Y7-E":21,"Y8-A":22,"DB-D":22,"Y8-B":22,"Y8-C":22,"Y8-D":22}
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

// ── 10-yap-meeting-audit-2026-10-26.mjs ───────────────────────────────────
// yap-audit — MEETING NOTETAKER AUDIT 2026-10-26 (stamp 2026-10-26T16:44:50Z).
// Source: docs/audits/meeting-notetaker-audit-2026-10-26.md (read it first; every item cites its issue #).
// Evidence came from Wilson's installed 0.9.3-dev (main a740614 + PR 250 head 0a87249) and a read-only copy
// of his meetings DB: 12 meetings, 4 over 10 min, plus a cloud-notetaker reference transcript of the 31-min one.
// Findings that existing items already own were appended to those items as "Audit 2026-10-26:" lines
// (NT1, NT2, NT3, NT4, NT5, NT6, NT9, NT11-0, NT11, yap-notetaker-notes, yap-lic-00-kill-forge-fetch).
// SENIOR PANEL 3 (2026-10-26T17:00Z, two seats + synthesis): approved utterance-segments, notes-strategy, rewrite-notes; extended notes-guards
// (F2 padded key points, F3 echo owner, F4 no-summary copy, F5 prefix clamp, mic-track owner, repetition penalty); remote-speaker-names DEFERRED
// (next release; calendar 1:1 name lives in yap24-NT4 DO R5); yap24-NT11 rescoped to an AEC3 pre-pass and yap24-NT11-0 folded into it and removed.
// This file holds ONLY the fixes no item owned. PRIVACY: no fixture, test or log may contain text from Wilson's
// real meetings; fixtures are synthetic (macOS say) and planted.
// Every cargo line carries YAP_DATA_DIR="$(mktemp -d)/yap-state" INLINE (never the real data dir).

ITEMS.push({
  id: 'yap-audit-vad-boundaries', prompt: 'yap-audit', branch: 'loop/yap-audit-vad-boundaries-wire-silence-cuts', gated: null, builder: 'sonnet',
  title: 'Meeting chunks cut at pauses, not on a 30 s clock: the shipped pipeline passes the warm Silero VAD to MeetingAsr (it is hardwired to vad: None)',
  preflight: `
    ! grep -n 'vad: None' desktop/src-tauri/src/meeting_pipeline.rs
    test -f desktop/src-tauri/tests/meeting_pipeline_vad_boundaries.rs
  `,
  spec: `
    Audit issue #3 (HIGH). meeting_pipeline.rs:522 (main a740614) builds MeetingAsr with vad: None, so plan_windows
    (meeting_asr.rs:379-440) always takes plan_windows_fixed. On Wilson's install all 414 chunks in the
    *.asr-progress.json files are fixed_clock/edge, zero silence boundaries, and 15-47% of segments end mid-sentence.
    DO
      1. Hand the app's warm Silero session (crate::vad::WarmVad, already implements VoiceActivity at
         meeting_asr.rs:578) to MeetingAsr in the production pipeline. A VAD load failure keeps today's behaviour
         (fixed clock) and logs one warn line; it never fails a meeting.
      2. Keep resume semantics: a resumed run re-plans from ResumePoint exactly as today.
      3. tests/meeting_pipeline_vad_boundaries.rs: drive the PRODUCTION pipeline entry (the function the worker
         calls, not plan_windows directly) over a synthetic WAV with pauses placed between 20 s and 40 s; assert at
         least one boundary kind is the silence kind and every silence cut lands inside a pause; a second case with
         a VAD stub that errors asserts fixed_clock fallback and a complete meeting.
    NOT: no change to the 30 s target / 2 s overlap constants; no UI change.
  `,
  acceptance: `
    ! grep -n 'vad: None' desktop/src-tauri/src/meeting_pipeline.rs
    (cd desktop/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_pipeline_vad_boundaries)
  `,
})

ITEMS.push({
  id: 'yap-audit-utterance-segments', prompt: 'yap-audit', branch: 'loop/yap-audit-utterance-segments-from-token-spans', gated: 'panel',
  title: 'A stored meeting segment is an utterance (token-span pauses merged into 3-12 s same-track turns), not a whole 30 s window, so Me/Them turns can be read',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/summary_jobs.rs && test -f desktop/src-tauri/src/meeting_aec.rs ) || ! grep -q 'call-bleed-2voice' desktop/src-tauri/tests/fixtures/meeting_eval_manifest.json; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 250 (summary_jobs.rs, which this touches via summarize.rs), yap24-NT11 (meeting_aec.rs, the AEC pre-pass this is measured on) or yap-audit-eval-call-bleed (the fixture) has not landed; land them, then build this"; exit 0; fi; test -f desktop/src-tauri/tests/meeting_utterance_segments.rs && grep -q 'fn split_utterances' desktop/src-tauri/src/meeting_asr.rs )
  `,
  spec: `
    Panel 3 2026-10-26: APPROVED-WITH-CHANGES (two seats, SOUND-WITH-CHANGES each). Unit and split rule are decided below.
    Audit issue #4 (HIGH). Average stored segment length is 29.4-30.1 s on every real meeting: a two-person call
    renders as alternating 30 s paragraphs, each with one label. Token spans already exist in 100% of non-empty chunks
    (timestampKind token; c6187806 track 0 had 62 of 62 chunks with spans) and are thrown away at insert.
    WHY THE RULE IS A MERGE: the raw gap rule alone (cut at a span gap >= 0.6 s, or a sentence end followed by a gap >= 0.25 s)
    produced 440 pieces with median 1.8 s on c6187806: too fine to read and too many MAP passes for the 1.5B notes budget.
    DO
      1. meeting_asr.rs: pub fn split_utterances(chunk spans) -> Vec<(start, end, text)>. Step one, raw cut: gap >= 0.6 s, or a sentence end
         followed by a gap >= 0.25 s. Step two, merge: consecutive pieces of the SAME track merge while the gap between them is < 1.2 s and the
         merged piece is <= 20 s; a piece under 3 s merges into its neighbour on the same track (when the result stays <= 20 s) and is kept
         alone only when it is the whole chunk. Chunks with timestampKind none stay whole. Never split inside a word span.
      2. The pipeline inserts the pieces as segments (same track, absolute times); the seam merge still runs first. Existing meetings keep
         their 30 s rows: NO migration, no re-split on open; only a re-transcribe (or the Re-transcribe and rewrite action) produces utterances.
      3. Track 0 reads the AEC-cleaned window from yap24-NT11 when that exists for the meeting, so the pieces are cut on the cleaned decode.
         This item does NOT depend on yap-audit-vad-boundaries.
      4. summarize.rs seg labels keep working (positional). Re-measure MAP chunk planning: every MAP chunk, INCLUDING its seg labels, is <= 1400
         tokens on a 31-minute two-track fixture, and the MAP pass count is <= 1.25x the whole-window-row count for the same meeting (a test
         computes both from the same fixture).
      5. tests/meeting_utterance_segments.rs: synthetic spans -> expected pieces (including the three thresholds above at their edges); at least one
         case reads token spans from a corpus asr-progress JSON of the NT0 synthetic corpus (timings only; never text from a real meeting); a
         two-track synthetic meeting yields interleaved turns ordered by start; meeting_utterance_segments asserts median_segment_seconds is
         between 3 and 12 on call-bleed-2voice after the AEC pre-pass.
    NOT: no change to the 30 s target / 2 s overlap constants; no UI change; no migration; no re-splitting of stored rows.
    Coordinate with yap-notetaker-notes (track turns UI).
  `,
  acceptance: `
    grep -q 'fn split_utterances' desktop/src-tauri/src/meeting_asr.rs
    (cd desktop/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_utterance_segments)
    (cd desktop/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test summary_default_notes)
  `,
})

ITEMS.push({
  id: 'yap-audit-notes-guards', prompt: 'yap-audit', branch: 'loop/yap-audit-notes-guards-no-retranscript', gated: null,
  title: 'Notes never ship a re-transcript or a wrong owner: refuse a verbatim Summary, strip Me:/Them: prefixes, one section per item, at most two items per cited segment, no padded Key points, no echoed or mic-track owner claims, a repetition penalty on notes decoding, a Summary input that covers the whole meeting, and a title that is not a transcript fragment',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/summary_jobs.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 250 (default meeting notes, summary_jobs.rs) has not landed; land it, then build this"; exit 0; fi; test -f desktop/src-tauri/tests/summary_audit_guards.rs && grep -q 'VERBATIM_NARRATIVE_MAX' desktop/src-tauri/src/summarize.rs && grep -q 'No summary this time: the notes model could only repeat the transcript.' desktop/src-tauri/src/summarize.rs && grep -q 'fn word_trigram_containment' desktop/src-tauri/src/meeting_echo.rs )
  `,
  spec: `
    Panel 3 2026-10-26: APPROVED (ungated; extended with F2, F3, F4, F5, the owner rule and the repetition penalty). PR 250 merged as-is ships grade-1 notes:
    guards 1-6 plus F2 and F3 must land before PR 250 reaches a release (posted on PR 250).
    Audit issue #2 (CRITICAL, mechanical half). On the three meetings PR 250 summarized on Wilson's Mac the Summary
    paragraph is 0.91-0.97 verbatim (4-gram containment in the transcript), one starts with a leaked "Me:" prefix,
    one segment produced 10 near-identical actions, the same line appears in up to 3 sections (8 duplicates on the
    31-min meeting), first-person narration ("It's really dope.") is filed as an action, and a failed headline falls
    back to the first 8 transcript words as the title (fallback_title, summarize.rs).
    Three of the failures are NOT the model: backfill_key_points fills Key points with raw transcript sentences by design; headline_source
    reads the MAP narratives clamped to a 750-word PREFIX so the end of a long meeting never reaches the Summary (the 31-min meeting's
    3 real commitments were at 27:30-29:10); and notes decoding is greedy with no repetition penalty (the runaway and the 10 actions from one segment).
    DO (summarize.rs and the polish sidecar, deterministic, no model change)
      1. VERBATIM_NARRATIVE_MAX = 0.6: a narrative whose word-4-gram containment in the source exceeds it is rejected
         (reason v8_verbatim). If no narrative survives, render NO Summary paragraph (never the MAP fallback text) and the Summary section reads
         exactly: "No summary this time: the notes model could only repeat the transcript."
      2. Strip a leading speaker tag (Me:, Them:, Speaker, Speaker N:) from narrative, key points and items.
      3. One item lives in exactly one section: actions over decisions over open questions (same_item match).
      4. At most 2 items may cite the same seg label; keep the two with the strongest commitment score.
      5. An action needs a commitment (owner_commitment / imperative addressed to a person / "I'll" + verb);
         first-person feelings and descriptions ("I'm excited", "It's really dope") are dropped (reason v9_not_action).
         A "decision" needs a decision marker; a statement without "?" is not an open question.
      6. fallback_title: never raw transcript words; use "Meeting, <weekday> <HH:MM>" from started_at when the
         headline title fails.
      7. tests/summary_audit_guards.rs: synthetic transcripts reproducing each failure above (no real meeting
         text); assert every guard, and that a clean planted fixture still yields its planted actions.
      8. (F2) backfill_key_points never pads with transcript sentences: fewer than MIN_KEY_POINTS validated points render as they are; zero
         validated points renders NO Key points section.
      9. (F3) Echo-owner rule: an item citing a Me-track segment whose word trigrams are >= 30% contained in a Them-track segment within +-1.5 s is
         attributed to Them; if neither track is clear the owner is left blank. Use the SAME pure fn as echo_dup_rate:
         meeting_echo::word_trigram_containment (src/meeting_echo.rs, created by yap-audit-eval-call-bleed; if it is not on main when this builds,
         create it there with that name and let yap-audit-eval-call-bleed import it). The AEC pre-pass (yap24-NT11) will reduce how often this
         fires; the guard stays, because AEC cannot fix tap holes or a far side that is mostly absent from the tap.
     10. Owner on the mic track: an action whose cited segment is on the mic track is owned "Me" only when the meeting is known to have a single
         voice on that mic; otherwise the owner reads "Someone here". v1 has no voice-count source (diarization, NT5, is deferred until after
         100 users), so the pure fn mic_is_single_voice(meeting) returns false until NT5 supplies it: every mic-track action reads
         "Someone here" unless the item text itself names a person. This is a deliberate v1 loss, taken over a wrong "Me": on c6187806 the
         3 real commitments came from a second local voice that AEC correctly keeps, and they were filed under Me. A Them-track action reads "Them".
     11. Repetition penalty on NOTES decoding only (yap-polish main.rs sampler plan, near line 548): add a llama penalties sampler (repeat 1.1 over the
         last 64 tokens) to the chain AHEAD of greedy for notes requests. Dictation polish stays byte-for-byte greedy (its golden fixtures must
         not move); a test asserts the dictation plan is unchanged.
     12. (F5) headline_source: the Summary input covers EVERY MAP chunk (never a prefix clamp of 750 words). When over budget, shrink each
         chunk's contribution evenly, never truncate from the end. A test plants a commitment in the LAST chunk of a long synthetic transcript and
         finds it in the Summary input.
    NOT: no change to GROUNDEDNESS_FLOOR or the model (that is yap-audit-notes-strategy, gated). No diarization.
  `,
  acceptance: `
    grep -q 'VERBATIM_NARRATIVE_MAX' desktop/src-tauri/src/summarize.rs
    grep -q 'v8_verbatim' desktop/src-tauri/src/summarize.rs
    grep -q 'No summary this time: the notes model could only repeat the transcript.' desktop/src-tauri/src/summarize.rs
    grep -q 'Someone here' desktop/src-tauri/src/summarize.rs
    grep -q 'fn word_trigram_containment' desktop/src-tauri/src/meeting_echo.rs
    grep -q 'fn mic_is_single_voice' desktop/src-tauri/src/summarize.rs
    (cd desktop/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test summary_audit_guards)
    (cd desktop/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test summary_default_notes)
    (cd desktop && cargo test -p yap-polish)
  `,
})

ITEMS.push({
  id: 'yap-audit-notes-strategy', prompt: 'yap-audit', branch: 'loop/yap-audit-notes-strategy-measured-decision', gated: 'panel',
  title: 'DECISION and SHIPPED, measured: Yap writes notes items-first and composes the Summary last from the surviving items; the arms are measured on the NT4 eval over AEC-cleaned input',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/summary_jobs.rs && test -f desktop/src-tauri/tests/meeting_notes_recall_eval.rs && test -f desktop/src-tauri/src/meeting_aec.rs ) || ! grep -q 'call-bleed-2voice' desktop/src-tauri/tests/fixtures/meeting_eval_manifest.json || ! grep -q 'VERBATIM_NARRATIVE_MAX' desktop/src-tauri/src/summarize.rs; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 250 (summary_jobs.rs), yap24-NT4 (the recall eval test), yap24-NT11 (AEC-cleaned input), yap-audit-eval-call-bleed (fixture) or yap-audit-notes-guards (VERBATIM_NARRATIVE_MAX) has not landed; land them, then build this"; exit 0; fi; test -f docs/decisions/notes-strategy-2026-10.md && grep -q '^DECISION:' docs/decisions/notes-strategy-2026-10.md && grep -q 'summary recall' docs/BUDGETS.md )
  `,
  spec: `
    Panel 3 2026-10-26: APPROVED-WITH-CHANGES. The strategy is chosen as advisory below; the numbers in this item confirm or overturn it, and the chosen arm SHIPS here.
    Audit issue #2 (CRITICAL, strategy half). summarize.rs GROUNDEDNESS_FLOOR = 0.80 over content words (common_gates, polish.rs content_words: raw >=4-char
    tokens, no stemming) rejects paraphrase ("discussed", "agreed", "planning" never appear in the transcript; 38 / 15 / 22 v3_ungrounded rejections per
    real meeting) and keeps verbatim copies. A 1.5B model classifies short spans reliably under a grammar; it cannot reliably write abstractive prose
    over 1,400 noisy tokens. The mechanical non-model failures are fixed first in yap-audit-notes-guards (padded Key points, prefix clamp, no repetition
    penalty), so this item measures the model honestly.
    ARMS (each runs on the AEC-cleaned fixture input, never on echo-polluted text, which would measure echo and not strategy):
      A. today (1.5B + 0.80 floor) + the guards: the baseline.
      B. 1.5B with a paraphrase-tolerant grounding check alone (digits, names and contacts stay hard-gated; lemma-level match for the rest). Necessary, not sufficient.
      E. ITEMS FIRST, SUMMARY LAST (recommended): keep the MAP grammar items (actions, decisions, questions, each tied to a segment id); drop
         narrative from MAP and drop REDUCE; add a 3-8 word topic label per MAP chunk (timestamped); write the Summary in ONE pass whose input is only the
         surviving items plus the topic labels (about 300-600 tokens, covering the whole meeting); replace the 0.80 floor with hard digit/name/contact
         gates, item grounding against the CITED segment +-1, and a Summary 4-gram verbatim ceiling <= 0.5.
      C. Qwen2.5-3B q4, MEASURED ONLY (about a 2 GB download and about 2x wall time, UNVERIFIED): reports peak RSS with the ASR engine unloaded; must stay <= 2.2 GB.
      The BYOM arm is REMOVED: the yap-byom-* items own BYOM and it is not a notes strategy.
    Each arm reports: action recall and precision, decision recall, owner accuracy (or blank), Summary 4-gram verbatim ratio, cross-section duplicates,
    items per segment, wall time and peak RSS, on call-bleed-2voice and the planted 15-minute fixture of yap24-NT4 (R2). The eval gates are the table in NT4:
    recall >= 0.8, precision >= 0.8, decision recall >= 0.5, verbatim ratio <= 0.5, cross-section dups 0, items per segment <= 2, owner accuracy >= 0.8 or blank.
    HONEST FALLBACK COPY (ships with arm E): no Summary passes -> no paragraph, and the section shows "Topics" (the timestamped labels); items fail the
    gates -> a section "Lines that sounded like commitments" (quoted, with time and track), never filed under "Action items"; every notes card ends with
    the footer "Written on this Mac by a small model. Check owners against the transcript."
    DO
      1. Implement arm E behind a const NOTES_STRATEGY in summarize.rs (E default) so A, B and E are runnable by the eval; arm C is a model-path env override used only by the eval.
      2. Extend tests/meeting_notes_recall_eval.rs (NT4 owns the file) to run each arm and print one numeric table row per arm.
      3. docs/decisions/notes-strategy-2026-10.md: the table (rows labelled A, B, C, E, numbers with a decimal point), a line starting "DECISION:" naming the
         shipped arm, and the follow-up item ids. Commit the same numbers to docs/BUDGETS.md under a heading containing "summary recall".
      4. The chosen arm is the default in the shipped code and the fallback copy above is in the UI path (a test asserts each of the three fallback renderings).
      5. The Summary input covers every chunk (the guards' last-chunk test stays green).
    NOT: no second local model by default (C is measurement only); no hosted model; no change to the polish_model setting.
    Human half (numbers only, never text): run the notes metrics against a COPY of the 31-minute meeting through scripts/eval/compare-reference.py --notes and paste the
    verbatim ratio, duplicate count, max items per segment and owner counts in the PR body.
    DEPENDS: yap24-NT4 eval, yap-audit-eval-call-bleed fixture, yap-audit-notes-guards, yap24-NT11 (AEC), PR 250.
  `,
  acceptance: `
    test -f docs/decisions/notes-strategy-2026-10.md
    grep -q '^DECISION:' docs/decisions/notes-strategy-2026-10.md
    grep -q 'summary recall' docs/BUDGETS.md
    [ "$(grep -cE '^\| [ABCE] \|.*[0-9]\.[0-9]' docs/decisions/notes-strategy-2026-10.md)" -ge 3 ]
    grep -q 'NOTES_STRATEGY' desktop/src-tauri/src/summarize.rs
    (cd desktop/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" YAP_EVAL_REQUIRE=1 YAP_MODEL_DIR="$HOME/code/wilson-voice-loop/cache/models" cargo test --features custom-protocol --test meeting_notes_recall_eval -- --ignored --nocapture --test-threads=1)
    (cd desktop/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test summary_default_notes)
    (cd desktop/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test summary_audit_guards)
  `,
})

ITEMS.push({
  id: 'yap-audit-rewrite-notes', prompt: 'yap-audit', branch: 'loop/yap-audit-rewrite-notes-any-meeting', gated: 'panel',
  title: 'Rewrite notes on any finished meeting, from the card and the detail view, through the retry command PR 250 already ships; the old notes stay until the new run succeeds',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/summary_jobs.rs ) || [ "$(gh pr view 246 --json state -q .state 2>/dev/null)" = OPEN ] || ! grep -q 'VERBATIM_NARRATIVE_MAX' desktop/src-tauri/src/summarize.rs; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 250 (summary_jobs.rs) has not landed, PR 246 (Meetings.tsx, progress.ts) is still open, or yap-audit-notes-guards (VERBATIM_NARRATIVE_MAX) has not landed (a rewrite before the guards returns the same grade-1 notes); land them, then build this"; exit 0; fi; grep -q 'Rewrite notes' desktop/src/views/Meetings.tsx && test -f desktop/src/meetings/rewriteNotes.test.tsx )
  `,
  spec: `
    Panel 3 2026-10-26: APPROVED-WITH-CHANGES (two seats). The backend already exists; this is a small UI-and-gate item.
    Audit issue #7 (HIGH) and #12. Two 10+ min meetings carry pre-PR-250 notes that are a 5x repetition loop cut
    mid-sentence and a 56-word excerpt; the backlog banner only offers meetings WITHOUT notes and the card shows
    "Write notes" only when there is no summary. The detail view has no notes action.
    EVIDENCE: PR 250 already ships summarize_meeting (lib.rs) and retry_meeting_summary, which submit whether or not a summary exists, and
    summarize_meeting_blocking refuses an empty overwrite ("the existing summary was kept"), so the old notes survive a failed run. The ONLY gate is
    canRetryNotes(..., hasSummary) in desktop/src/meetings/progress.ts (about lines 104-112). A new rewrite_notes command would duplicate all of that.
    DO
      1. Change canRetryNotes to allow a complete or partial meeting that already has a summary. No new Rust command: the button invokes retry_meeting_summary.
      2. A "Rewrite notes" button on the card and a notes action row (Copy, Rewrite notes) in the detail view.
      3. While it runs the notes area reads exactly: "Rewriting. Your current notes stay until the new ones are ready." No "older version" hint is shown.
      4. If the notes model is not installed, show the one-line download size (about 1.1 GB) and the existing consent BEFORE the run; the rewrite never starts a silent
         fetch (run_one -> fetch_model must not run without the existing consent).
      5. desktop/src/meetings/rewriteNotes.test.tsx covers both placements, the keep-old-until-success rule, the copy above, and the model-missing line.
      6. DEPENDENT (only when src/meeting_aec.rs from yap24-NT11 is on main at build time; otherwise yap24-NT11 carries this step when it lands): when the meeting has
         audio_kept=1, offer "Re-transcribe and rewrite", because rewriting over an echo transcript re-files the same wrong owners. It re-runs the transcription
         (so the AEC pre-pass applies) and then the notes job.
    NOT: no fn rewrite_notes, no new Tauri command, no notes history UI.
  `,
  acceptance: `
    grep -q 'Rewrite notes' desktop/src/views/Meetings.tsx
    grep -q 'retry_meeting_summary' desktop/src/views/Meetings.tsx
    grep -q 'Your current notes stay until the new ones are ready.' desktop/src/views/Meetings.tsx
    ! grep -q 'fn rewrite_notes' desktop/src-tauri/src/lib.rs
    (cd desktop && npx vitest run src/meetings/rewriteNotes.test.tsx)
  `,
})

ITEMS.push({
  id: 'yap-audit-asr-unload-before-notes', prompt: 'yap-audit', branch: 'loop/yap-audit-asr-unload-before-notes', gated: null, builder: 'sonnet',
  title: 'The ASR engine is unloaded before the notes sidecar starts (unless a dictation ran in the last 60 s), so the post-meeting peak is one model, not two',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/summary_jobs.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 250 (summary_jobs.rs) has not landed; land it, then build this"; exit 0; fi; test -f desktop/src-tauri/tests/notes_unloads_asr_first.rs )
  `,
  spec: `
    Audit issue #9 (MEDIUM). On Wilson's Mac the ASR model (in-process, ~1.1 GB) stays loaded ~20 min after a meeting
    (unloads logged 21:17:43Z after a 20:57:48Z transcript; 16:49:05Z after 16:16:19Z) while the notes sidecar runs
    at 1.37 GB RSS: about 2.5 GB peak against the 8 GB M1 floor.
    DO: in the notes backend's start path, if no dictation take ended in the last 60 s, unload the ASR engine (the
    same call the idle unload uses) before spawning yap-polish; the next dictation reloads lazily (YV80 lazy default).
    A dictation that starts during notes reloads ASR as today; notes never block a take.
    tests/notes_unloads_asr_first.rs: with a stub engine and a stub sidecar launcher, assert the unload happens before
    spawn when idle, and does not happen when a take ended 10 s ago.
  `,
  acceptance: `
    (cd desktop/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test notes_unloads_asr_first)
  `,
})

ITEMS.push({
  id: 'yap-audit-meeting-timings', prompt: 'yap-audit', branch: 'loop/yap-audit-meeting-timings-in-the-log', gated: null,
  title: 'The log explains a slow meeting: decode ms per chunk, one RTF line per track, notes phase timings and rejection counts by reason, and a true retune line',
  preflight: `
    grep -q 'meeting rtf' desktop/src-tauri/src/meeting_pipeline.rs && test -f desktop/src-tauri/tests/meeting_timing_lines.rs
  `,
  spec: `
    Audit issues #10 and #16 (MEDIUM/LOW). The 31-min meeting took 4:01 to transcribe (RTF 0.13) vs 0:28 for 24 min
    three days earlier (RTF 0.019); the log cannot say why (no per-chunk timing; loop CI load and thermal fair were
    both present). Notes runs log one line per rejection but no phase timings. A mid-meeting input retune logged
    "0 samples captured so far" 30 minutes into a capture (the dictation fallback counter, not the meeting's).
    DO
      1. meeting_asr / pipeline: debug line per chunk (track, index, audio s, decode ms, boundary kind); one info
         line per track at completion: "meeting rtf track=N audio_s=.. wall_s=.. rtf=.. chunks=.. preempted=..".
      2. summarize: one info line per run with model load ms, MAP pass count and ms, headline ms, reduce ms, and
         rejection counts by reason (v2_runaway=.. v3_ungrounded=..) instead of one line per rejection (keep the
         per-rejection lines at debug).
      3. record.rs retune line: for a meeting capture report the meeting's captured seconds; add the retune (time,
         old/new format) to the meeting diagnostics JSON.
      4. tests/meeting_timing_lines.rs: format functions are pure and tested; no log scraping of real data.
  `,
  acceptance: `
    grep -q 'meeting rtf' desktop/src-tauri/src/meeting_pipeline.rs
    (cd desktop/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_timing_lines)
  `,
})

ITEMS.push({
  id: 'yap-audit-eval-call-bleed', prompt: 'yap-audit', branch: 'loop/yap-audit-eval-call-bleed-fixture-and-metrics', gated: null, builder: 'sonnet',
  title: 'The audio box, offline: a synthetic two-voice call-bleed fixture with planted commitments, echo-dup and notes-owner metrics in meeting_eval, and a numbers-only reference comparison script',
  preflight: `
    grep -q 'call-bleed-2voice' desktop/src-tauri/tests/fixtures/meeting_eval_manifest.json && test -x scripts/eval/compare-reference.py && grep -q 'fn word_trigram_containment' desktop/src-tauri/src/meeting_echo.rs
  `,
  spec: `
    Audit issue #13 (MEDIUM) and section 9. The NT0 corpus has no fixture where the far side leaks into the mic,
    no conversation between two voices and no planted owners, so issues #1-#4 and #2 cannot be regression-tested.
    DO
      1. Extend meeting_eval_generate_corpus (tests/meeting_eval.rs) with fixture call-bleed-2voice (~4 min, macOS
         say, two voices): track1.wav = far side; track0.wav = near side plus far side at -12 dB, delayed 60 ms,
         short reverb tail; turns of 2-8 s; a second near-side voice for 30 s; 6 planted commitments with owners and
         2 decisions in meta.json; reference.txt with speaker tags. Regenerate the manifest + sha256 the way NT0
         documents (never hand-edit hashes).
      2. Metrics (pure functions, unit-tested in tests/meeting_eval_metrics.rs): echo_dup_rate (share of call
         windows whose word trigrams are >= 30% contained in the mic window within +-1.5 s; it CALLS the production pure fn
         meeting_echo::word_trigram_containment in desktop/src-tauri/src/meeting_echo.rs, which this item creates as pub so the tests and the
         notes echo-owner guard (yap-audit-notes-guards) share ONE definition), mid_sentence_end_rate,
         median_segment_seconds, notes_owner_accuracy and summary_verbatim_ratio (word 4-gram containment).
         meeting_eval prints them for the fixture; gates are added by the items that fix them (NT11 sets echo_dup_rate
         <= 0.10, yap-audit-utterance-segments sets median < 12 s, NT4 sets owners).
      3. scripts/eval/compare-reference.py (python3 -I, executable): args = a COPY of wilson_voice.db, a meeting id, a
         reference transcript ("M:SS - Speaker" blocks). Prints ONLY numbers: offset, S/I/D and WER per track and as
         rendered, per-reference-speaker share in each track, echo rate, coverage. Never prints transcript text or
         names (speakers become A, B, C...). --self-test runs on a tiny synthetic pair and exits 0.
         Two extra inputs (panel 3, 2026-10-26): --tracks <t0 timed json> <t1 timed json> <reference> compares two timed-transcript files
         (the output of wilson-voice --transcribe-file <wav> --timed) with no DB, which is how the AEC pre-pass is measured on audio on disk;
         --notes prints, from a COPY of the DB, numbers only for one meeting's notes: Summary 4-gram verbatim ratio, duplicate lines across
         sections, max items per cited segment, and owner counts (Me / Them / Someone here / blank).
    DEPENDS: yap24-NT0 provisioning (corpus path); consumers are yap24-NT9 (CLI), yap24-NT4, yap24-NT11.
  `,
  acceptance: `
    grep -q 'call-bleed-2voice' desktop/src-tauri/tests/fixtures/meeting_eval_manifest.json
    test -x scripts/eval/compare-reference.py
    python3 -I scripts/eval/compare-reference.py --self-test
    grep -q 'fn word_trigram_containment' desktop/src-tauri/src/meeting_echo.rs
    (cd desktop/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_eval_metrics)
  `,
})

ITEMS.push({
  id: 'yap-audit-notes-quit-cancel', prompt: 'yap-audit', branch: 'loop/yap-audit-notes-quit-cancel-one-line', gated: null, builder: 'sonnet',
  title: 'Quitting during notes interrupts the job with one line ("resumes next launch") instead of N failed MAP passes',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/summary_jobs.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 250 (summary_jobs.rs) has not landed; land it, then build this"; exit 0; fi; test -f desktop/src-tauri/tests/notes_quit_interrupts.rs )
  `,
  spec: `
    Audit issue #14 (LOW). On the live 31-min meeting the app was quit 16 s into notes: the log shows 8 warn lines
    "a MAP pass failed (unavailable) - continuing" and "every one of 8 MAP pass(es) failed - refusing to overwrite"
    in 1 s. The launch resume then worked (27 s after relaunch). Wrong signal: it reads like a model failure.
    DO: when the app is quitting, the notes client returns Cancelled-by-quit (not Unavailable) so summarize stops at
    the next pass with one info line "notes for <id> interrupted by quit - resumes next launch"; the ledger is not
    marked cancelled (the resume must still run). tests/notes_quit_interrupts.rs with a stub client.
  `,
  acceptance: `
    (cd desktop/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test notes_quit_interrupts)
  `,
})

ITEMS.push({
  id: 'yap-audit-remote-speaker-names', prompt: 'yap-audit', branch: 'loop/yap-audit-remote-speaker-names-active-speaker', gated: 'panel',
  title: 'DEFERRED to the next release: name remote speakers locally from the call app\'s active-speaker indicator (Accessibility); v1 uses the calendar 1:1 name inside yap24-NT4 instead. SPIKE, or record NO-GO',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/call_signals.rs && test -f desktop/src-tauri/src/meeting_aec.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-call-0 (call_signals.rs, call-app catalog) or yap24-NT11 (meeting_aec.rs; a name on Them leaves the echoed half in Me until echo_dup_rate <= 0.10) has not landed; land them, then build this"; exit 0; fi; head -1 docs/loop/evidence/yap-audit-remote-speaker-names.md | grep -qE '^SPIKE-VERDICT yap-audit-remote-speaker-names (GO|NO-GO)' )
  `,
  spec: `
    Panel 3 2026-10-26: DEFERRED to the next release (both seats: RETHINK). NOT in panelApproved. Kept here, gated, so nothing ages out.
    WHY DEFERRED: it reads another app's participant tree every second (a new SECURITY-class surface beyond yap-call-2's title-only Accessibility read, same review
    as yap-call-2); Zoom's grid is custom-drawn with no Accessibility speaker flag, and Meet needs Chrome's renderer accessibility forced on (CPU); indicator lag
    plus echo puts a wrong name on a line, which is worse than "Them". Today a name on Them leaves the echoed half in Me, so this needs the AEC pre-pass first.
    V1 ALTERNATIVE (owned by yap24-NT4, DO R5): for a 1:1 call the one other calendar attendee is an honest name with no new OS surface; the notes owner of a
    Them-track item uses that name when exactly one non-self attendee exists, otherwise stays "Them".
    NEXT-RELEASE NOTE (not v1): once diarization (yap24-NT5, deferred until after 100 users) runs on the call track it gives Them A / Them B with NO new
    permission; that is the second alternative to evaluate before any Accessibility grid read.
    Audit section 10 (biggest gap vs the cloud notetaker). The reference product on this Mac ships remote-speaker attribution from the meeting app's own UI
    (diarization/remote_speaker.rs, active_speaker_heuristics/odd_one_out.rs), sampled per second. Yap labels the whole call track "Them".
    SPIKE DO: for Zoom desktop and Google Meet in Chrome only, read the active-speaker element per second during a test call (synthetic voices), map seconds to
    call-track utterances, report precision of the name per utterance; write docs/loop/evidence/yap-audit-remote-speaker-names.md whose first line is
    "SPIKE-VERDICT yap-audit-remote-speaker-names GO" or "... NO-GO" with the numbers. No product code ships from the spike.
    GO BAR (all required): per-utterance name precision >= 0.9 on Zoom AND on Meet; sampler CPU <= 3%; no new TCC prompt beyond the one yap-call-2 already shows; and
    NT11 echo_dup_rate <= 0.10 on call-bleed-2voice. Anything else is NO-GO.
    SECURITY NOTE (seat A): participant names are other people's data read from another app; the read is local-only, never logged with names, never stored beyond the
    meeting row, and needs its own consent line; class SEC.
    DEPENDS: yap-call-0, yap24-NT11, yap-audit-utterance-segments.
  `,
  acceptance: `
    head -1 docs/loop/evidence/yap-audit-remote-speaker-names.md | grep -qE '^SPIKE-VERDICT yap-audit-remote-speaker-names (GO|NO-GO)'
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

// LIC-A — DELETED 2026-10-26 by the Senior Panel plan-audit (object removed; the harness has no retired/status field, removal is the mechanism).
// It was RETIRED 2026-10-19 (superseded by yap-lic-00-kill-forge-fetch and yap-lic-01..06) but still counted as a panel-gated item and its preflight
// pointed at supabase/functions, against the ledger (licensing lives on the consulting database #2). Original spec is in git history.

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

// DB-A — KILLED 2026-10-26 by the Senior Panel plan-audit (object removed; the harness has no retired/status field, removal is the mechanism).
// Why: usage metering and a "limit reached" surface for a limit the ledger rules out ($29 lifetime, no metered tier, no trial-nag surfaces). It shipped a
// dark LIMIT_ENABLED=false gate plus a second rollup beside get_insights: infrastructure for its own sake. Y10-E's reference to DB-A's rollup was
// repointed to the existing get_insights day-series. Original spec is in git history.

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
  notes: `STATUS 2026-09-26: BUILT, PR #197 OPEN and stale (main moved 30+ times under it), labelled needs-human. Rebase that PR onto main and finish it; do not open a second PR. AMENDED 2026-09-26: the premise "points at a dead URL" is FALSE while the repo is public — releases/latest/download/latest.json returns HTTP 200 and serves 0.8.0 (darwin-aarch64). The item is still required before the repo is privatized at app-completion (Wilson 2026-08-11); until then it is not urgent. Wilson's log also shows "update check failed: error sending request" on launches without network — honest, keep it quiet.`,
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
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop. 2026-09-26: lane A was in pre-flight at the 2026-09-15 stop and wrote nothing.`,
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
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
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
  notes: `SUPERSEDED IN PART 2026-10-19 by yap-lic-00-kill-forge-fetch (07b, SECURITY): the revocation list refresh is DELETED, so Yap has TWO network call sites (the model download and the updater check), not three. The allowlist test pins exactly those two and must fail on any revocation or licensing fetch anywhere in the crate; PRIVACY.md names two. Never assert, document or restore a revocation refresh. The preflight holds this item (dependency not on main) until yap-lic-00 has landed. PRIOR NOTES: STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
  title: 'Crash reporting is local, complete and provably offline — no Sentry, no PostHog, ever',
  preflight: `
    ( M=0; git grep -qnE 'REVOCATION_URL|ISSUER_HOST|fetch_revocations|spawn_revocation_refresh|revoked\\.json' -- desktop/src-tauri/src && M=1; grep -q 'launch_license_init_makes_no_network_call' desktop/src-tauri/src/license.rs || M=1; if [ "$M" = 1 ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-lic-00-kill-forge-fetch has not landed (the revocation refresh is still in desktop/src-tauri/src, or license.rs lacks launch_license_init_makes_no_network_call) — PRIV-A must not pin a call that item deletes; run the review pass to land it, then the build pass again"; exit 0; fi; test -f desktop/src-tauri/tests/no_outbound_on_the_dictation_path.rs && grep -q 'only_two_network_call_sites_exist' desktop/src-tauri/tests/no_outbound_on_the_dictation_path.rs && cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test no_outbound_on_the_dictation_path )
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
          the model download and the updater check — an explicit allowlist by
          file and function that a new call site forces you to update. The
          revocation list refresh was DELETED by yap-lic-00-kill-forge-fetch
          (SECURITY); it is NOT on the allowlist, and the sweep covers the WHOLE
          crate (every file under desktop/src-tauri/src, not only license.rs),
          so a revocation or licensing fetch re-added in lib.rs or in a new
          module fails this test. Licensing returns to the network only through
          the yap-lic-01..06 items, which update this allowlist themselves.
      * \`crash.rs\` completeness: a crash report must never contain transcript
        text, a file path inside the user's home beyond the app's data dir, or a
        license key. Extend the redaction tests with a report synthesized to
        contain all three and assert all three are gone.
      * Nothing in crash.rs may upload. The support bundle is produced for the
        USER to send; assert there is no send path.
      * Update PRIVACY.md to state the claim in the exact words the test proves,
        and add the test's name to the doc so the claim is traceable. Do not
        soften the claim and do not overstate it: the model download and the
        updater DO talk to the network, and PRIVACY.md must say which two and
        that neither carries audio or text. The licensing path makes NO network
        call (yap-lic-00 deleted the revocation refresh); never describe, assert
        or restore one.

    What NOT to do:
      - Do NOT add Sentry, PostHog, Segment or any analytics SDK.
      - Do NOT add an opt-in telemetry toggle. There is no telemetry to toggle.
      - Do NOT claim in PRIVACY.md that Yap makes no network calls at all. Two
        calls exist and naming them is what makes the rest credible.
      - Do NOT re-add, allowlist or document a revocation list refresh or any
        licensing network call. yap-lic-00 removed it as a SECURITY fix.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/no_outbound_on_the_dictation_path.rs
    grep -q 'only_two_network_call_sites_exist' desktop/src-tauri/tests/no_outbound_on_the_dictation_path.rs
    ! git grep -nE 'REVOCATION_URL|ISSUER_HOST|fetch_revocations|spawn_revocation_refresh|revoked\\.json' -- desktop/src-tauri/src desktop/src
    ! grep -niE 'revocation (check|list|refresh|download)|refunded or charged back|three network' PRIVACY.md
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
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
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
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
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
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
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
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
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
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
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
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop. 2026-09-26 evidence: ARCHITECTURE.md:5 still says "OpenAI Whisper weights via MLX on-device" and its decision table says Whisper; the engine is transcribe-cpp (Parakeet default, src/catalog.json). docs/ARCHITECTURE-AUDIT-2026-09-26.md §8 lists the rest.`,
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
        the license model, and the two network calls PRIV-A names (the model
        download and the updater check; the licensing path makes none since
        yap-lic-00).
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
  id: 'Y7-A', prompt: 'Y7', branch: 'loop/y7-a-headless-smoke-against-the-built-binary', gated: null, builder: 'sonnet',
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
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
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop. 2026-09-26: despite the title, scripts/smoke-windowed.mjs drives Chrome with --headless=new against vite preview — it never opens a visible window, so it complies with the headless-only rule.`,
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
  id: 'Y7-C', prompt: 'Y7', branch: 'loop/y7-c-non-vacuous-mutation-proof-for-the-new-tests', gated: null, builder: 'sonnet',
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
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
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
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
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
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
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop. 2026-09-26 evidence: paste-last already ships (yap.log "⌃⌘V paste-last registered" on every launch) and cancel mid-decode shipped in Y3-D (#176); the pre-flight must retire those parts and the builder owns only what is missing (copy-last, scratchpad hotkey, validation rules).`,
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
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
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
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
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
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
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
  notes: `STATUS 2026-09-26: NOT STARTED as of the 2026-09-15 stop.`,
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
