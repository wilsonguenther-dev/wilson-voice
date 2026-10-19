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
  name: "yap-overhaul-all-part-01",
  description:
    "Part 01 of the Yap (wilson-voice) overhaul, two builder lanes: land whatever is already gate-green, pre-flight on unmodified main (already-done items are skipped), build, open a labelled PR. The adversarial review, the independent second-Opus gate and the merge bar run afterwards in the same script with args {mode:'review'}. 47 items (Y0-A..loop-dependency-marker-stdout-only), 24 awaiting the Senior Panel. The gate is local (Actions is disabled by the account spending limit); the DMG is not in it.",
  phases: [
    { title: "Recon", detail: "two lane worktrees, one npm ci + one warm cargo build each, the loop-build label, ci-mode measured" },
    { title: "Y0-A", detail: "Clippy becomes a blocking CI gate instead of `|| true` decoration (rustfmt stays informational — ledger)" },
    { title: "Y0-D", detail: "One documented state root override and a --smoke mode, so two lanes and Wilson's own install stop sharing one history and one hotkey" },
    { title: "Y0-E", detail: "The instrument that can see \"looks broken\" ships BEFORE the nine UI items it judges — and it fails on today's tree" },
    { title: "Y0-B", detail: "One command proves a fresh clone builds, tests and stages both sidecars — no host-found dependencies" },
    { title: "Y0-C", detail: "A test asserts the SHIPPED defaults, so a feature that is off by default can never be called tested again" },
    { title: "yap24-NT0", detail: "Provisioning, not product: the pinned parakeet model sits sha256-verified in the lane cache and the meeting eval corpus is materialised at its fixed path" },
    { title: "yap24-NT1", detail: "Meeting audio is never deleted before it has been transcribed, and stranded \"transcribing\" rows are reconciled on launch" },
    { title: "yap24-NT2", detail: "The meeting transcript exists: stop hands the audio to the shipped MeetingAsr job, which writes segments and completes the row" },
    { title: "yap24-NT3", detail: "Silence is not a denial: an in-person meeting never attaches the system tap, and a quiet call is never told macOS refused it" },
    { title: "yap24-NT4", detail: "Notes appear on their own: a completed transcript is summarized locally, action items are stored as rows, and \"no model\" is a state, not an error" },
    { title: "yap24-NT5", detail: "Who said what: in-person meetings run the shipped diarization sidecar after transcription, with the honest accuracy framing" },
    { title: "yap24-NT6", detail: "The Meetings view tells the truth about every meeting: progress, notes, failure with Retry, audio kept or expired" },
    { title: "yap24-NT7", detail: "The pill and the menu bar follow the meeting after stop: transcribing 42%, notes ready, or what went wrong" },
    { title: "yap24-NT8", detail: "Calendar-aware: an upcoming meeting or class raises a \"Record?\" prompt on the pill — opt-in, EventKit, no OAuth" },
    { title: "yap24-NT9", detail: "Phase-closing proof: one headless command runs a two-track fixture through capture, transcript, notes and export" },
    { title: "yap24-NT10", detail: "A meeting survives a lid close or sleep: the journal finalizes on WillSleep and the row is marked paused_by_sleep, not left recording forever" },
    { title: "yap24-NT11", detail: "A call on built-in speakers is transcribed once: mic segments that re-hear the system track are dropped, and the dedupe is skipped on headphones" },
    { title: "yap24-PILL1", detail: "The trial is visible from day 14 to day 1: a quiet numeral for the first week, the Y2-B treatment for the last" },
    { title: "yap24-PILL2", detail: "The backend says which stage a take is in, so \"polishing\" and \"pasting\" finally appear on the pill" },
    { title: "yap24-PILL3", detail: "When the hotkey cannot hear you, the pill says so: Secure Input, a disabled tap, and missing Accessibility are pill states" },
    { title: "yap24-PILL4", detail: "Hands-free looks different from hold: a lock mark and \"tap fn⌃ to stop\", in both characters and all docks" },
    { title: "yap24-OS1", detail: "The first word is never lost and the start is 280 ms sooner: capture arms on key-down and a tap throws the pre-roll away" },
    { title: "yap24-OS2", detail: "Launch at login through SMAppService.mainApp, the way macOS 13+ expects, instead of a LaunchAgent plist" },
    { title: "yap24-OS3", detail: "Permission truth: the hotkey asks for Input Monitoring by name, the purpose strings describe today's app, and a grant is noticed on focus" },
    { title: "yap24-OS4", detail: "Hardened-runtime diet: prove whether allow-jit and allow-unsigned-executable-memory are needed, and drop what is not" },
    { title: "yap24-OS5", detail: "Idle means idle: the fixed polls (Secure Input 2 s, pill space-keeper, permission watch) are consolidated and measured" },
    { title: "yap24-UI1", detail: "Every sprite pixel lands on whole device pixels at every dock size and backing scale — no half-pixel blur" },
    { title: "yap24-UI2", detail: "One animation clock for the pill and the habitat: frame-rate independent, parks at idle, honours Reduce Motion" },
    { title: "yap24-UI3", detail: "Pay the visual-QA debt: a headless screenshot matrix of every pill phase x dock x character, attached to the PR" },
    { title: "PERM-A", detail: "Ask macOS the actual question: AVCaptureDevice authorizationStatus + requestAccess, replacing the device probe that cannot see a denial" },
    { title: "PERM-B", detail: "A denied microphone gets its own screen with a working System Settings deep link, not a green check" },
    { title: "PERM-C", detail: "Every hotkey press re-checks the grant, and the pill shows a denied state instead of recording silence" },
    { title: "Y1-A", detail: "A tap macOS disabled is re-armed and reported — the other half of \"the hotkey is dead\"" },
    { title: "PERM-D", detail: "First run refuses to reach calibration without a real grant, and a silent take is diagnosed instead of pasted as nothing" },
    { title: "SEC-A", detail: "Stop ad-hoc signing local builds — the reason permissions \"reset\" on Wilson's own machine" },
    { title: "Y1-B", detail: "Register the sleep/wake observer two later items already claim exists" },
    { title: "PERM-E", detail: "One permission health surface, watched for revocation, covering mic + Accessibility + Input Monitoring + audio capture" },
    { title: "yap-flakes-2026-10-16", detail: "Main goes green: per_chunk_deadline_not_per_take and meeting_manual_start_stop stop asserting on the scheduler" },
    { title: "yap-lifecycle-2026-10-16", detail: "Quit means quit, reopen means a window: RunEvent::Reopen, one clear Quit, the pill gone at quit, bounded teardown, RSS flat under load" },
    { title: "yap-p0-hotkeys-no-permission", detail: "A take starts and stops with ZERO TCC grants: a fixed-chord dictation hotkey on Carbon RegisterEventHotKey is the primary path, the fn event tap is the extra, and a dead tap is shown as a degraded mode instead of a working hotkey" },
    { title: "yap-p0-perm-repair", detail: "One truthful grant probe per service (live API, never the Settings list) in perm_probe.rs, a detected \"stale identity\" state, and a one-click Repair: tccutil reset for Yap's own bundle id, open the exact pane, re-probe until granted" },
    { title: "yap-p0-insertion-bridge-fallback", detail: "Insertion never silently fails: AX insertion only when the probe says trusted, a clipboard + ⌘V bridge when PostEvent is granted, an instant Copy card when neither is, and one failing backend call can no longer blank the app as \"Bridge error\"" },
    { title: "yap-p0-release-smoke", detail: "Every DMG build runs a signed-app smoke after signing (codesign --strict on app + DMG with the pinned Developer ID team, a designated-requirement change gate, launch, probe every service, register hotkeys, insert into TextEdit) and the release FAILS when any step fails, including a stale-identity grant" },
    { title: "yap-qa-hotkey-walk-binding", detail: "hotkey-walk presses the CONFIGURED push-to-talk chord (fn, fn⌃, …) read from settings.json, has a dev-build flag, and the app shows a hint when fn is held without the binding's modifier" },
    { title: "yap-lic-00-kill-forge-fetch", detail: "SECURITY: delete the launch-time revocation GET to the dead Forge sslip.io host entirely; no licensing network call at all until the new client lands" },
    { title: "yap-dev-perm-probe-attribution", detail: "A dev build's permission probe says \"dev build: permissions are the host terminal's\" instead of showing the launching terminal's TCC grants as Yap's green rows" },
    { title: "loop-dependency-marker-stdout-only", detail: "The harness's dependency hold matches DEPENDENCY-NOT-ON-MAIN only as a line the EXECUTED preflight printed, never anywhere in the runner seat's JSON" },
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
  'desktop/src-tauri/capabilities/, desktop/src-tauri/tauri.conf.json, the updater/signing path, desktop/src-tauri/src/db or any migration, the polish_protocol/sidecar IPC, or any path or diff hunk matching updater|sign|notariz|keychain|permission|accessibility|microphone|network|http|upload|telemetry, or the loop harness itself (scripts/loop/, scripts/loop/review-gate.mjs included, scripts/cicd-loop-all.mjs) or .github/workflows/'
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
const LANE_BY_ID = {"Y0-A":0,"Y0-D":0,"Y0-E":0,"Y0-B":0,"Y0-C":0,"yap24-NT0":1,"yap24-NT1":1,"yap24-NT2":1,"yap24-NT3":1,"yap24-NT4":1,"yap24-NT5":1,"yap24-NT6":1,"yap24-NT7":1,"yap24-NT8":1,"yap24-NT9":1,"yap24-NT10":1,"yap24-NT11":1,"yap24-PILL1":0,"yap24-PILL2":0,"yap24-PILL3":0,"yap24-PILL4":0,"yap24-OS1":1,"yap24-OS2":1,"yap24-OS3":1,"yap24-OS4":1,"yap24-OS5":1,"yap24-UI1":0,"yap24-UI2":0,"yap24-UI3":0,"PERM-A":1,"PERM-B":1,"PERM-C":1,"Y1-A":1,"PERM-D":1,"SEC-A":1,"Y1-B":1,"PERM-E":1,"yap-flakes-2026-10-16":0,"yap-lifecycle-2026-10-16":1,"yap-p0-hotkeys-no-permission":0,"yap-p0-perm-repair":0,"yap-p0-insertion-bridge-fallback":0,"yap-p0-release-smoke":0,"yap-qa-hotkey-walk-binding":1,"yap-lic-00-kill-forge-fetch":1,"yap-dev-perm-probe-attribution":1,"loop-dependency-marker-stdout-only":1}
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
  title: 'Meeting audio is never deleted before it has been transcribed, and stranded "transcribing" rows are reconciled on launch',
  preflight: `
    test -f desktop/src-tauri/tests/meeting_retention_keeps_untranscribed_audio.rs
    grep -q "fn reconcile_stranded_meetings" desktop/src-tauri/src/meetings.rs desktop/src-tauri/src/lib.rs desktop/src-tauri/src/meeting_control.rs
  `,
  spec: `
    Panel: pending
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
      1. Retention becomes STATE-AWARE: audio is eligible for purge only when the meeting is
         complete AND it has at least one meeting_segments row AND the retention window has
         passed. Any other state keeps its audio indefinitely, and the Meetings view (yap24-NT6)
         says so. Both tracks age out together (the YV106 promise stays).
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
  `,
  acceptance: `
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_retention_keeps_untranscribed_audio
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_audio_retention
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib meeting
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-NT2', prompt: 'yap24-NT', branch: 'loop/yap24-nt2-wire-the-meeting-transcription-pipeline', gated: 'panel',
  title: 'The meeting transcript exists: stop hands the audio to the shipped MeetingAsr job, which writes segments and completes the row',
  preflight: `
    test -f desktop/src-tauri/tests/meeting_pipeline_wired.rs
    ! grep -n "allow(dead_code)" desktop/src-tauri/src/meeting_asr.rs
    grep -rln "MeetingAsr {" desktop/src-tauri/src | grep -v "meeting_asr.rs" | grep -q .
  `,
  spec: `
    Panel: pending
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
      - BLOCKING — segments must NOT land per-chunk. append_meeting_segments (db.rs:1652) is a
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
      6. A headless CLI entry beside --transcribe-file (cli.rs): --transcribe-meeting <mic.wav>
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
  `,
  acceptance: `
    ! grep -n "allow(dead_code)" desktop/src-tauri/src/meeting_asr.rs
    grep -rln "MeetingAsr {" desktop/src-tauri/src | grep -v "meeting_asr.rs" | grep -q .
    grep -q 'meeting_pipeline::spawn' desktop/src-tauri/src/lib.rs
    grep -q 'YAP_EVAL_REQUIRE' desktop/src-tauri/tests/meeting_eval.rs
    grep -q 'YAP_MODEL_DIR' desktop/src-tauri/tests/meeting_eval.rs
    cd desktop && npm ci && cd src-tauri
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
  title: 'Notes appear on their own: a completed transcript is summarized locally, action items are stored as rows, and "no model" is a state, not an error',
  preflight: `
    grep -q "CREATE TABLE IF NOT EXISTS meeting_actions" desktop/src-tauri/src/db.rs
    test -f desktop/src-tauri/tests/meeting_notes_auto_summary.rs
  `,
  spec: `
    Panel: pending
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
  `,
  acceptance: `
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_notes_auto_summary
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_delete_cascade
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_markdown_export
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib summarize
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-NT5', prompt: 'yap24-NT', branch: 'loop/yap24-nt5-diarization-joins-the-pipeline', gated: 'panel',
  title: 'Who said what: in-person meetings run the shipped diarization sidecar after transcription, with the honest accuracy framing',
  preflight: `
    grep -rln "diarize::pool()" desktop/src-tauri/src | grep -v "diarize.rs\|meeting_matrix.rs" | grep -q .
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
    test -f desktop/src/meetings/viewState.ts
    test -f desktop/src/meetings/viewState.test.ts
  `,
  spec: `
    Panel: pending
    DEPENDS: yap24-NT2, yap24-NT4 (states and fields it renders)

    EVIDENCE
      - views/Meetings.tsx:339-347: a transcribing meeting with no segments renders "Yap is still
        working through the audio." — forever, because nothing was working through it.
      - No progress, no failed/partial rendering with a way out, no Retry, no summary-model state,
        no action items (the summary panel at :332-337 is one paragraph).
      - notetaker_status (lib.rs, the 14.4 gate sentence) reaches only the Settings setup step
        (meeting_matrix.rs row 12b).

    DO
      1. A pure module desktop/src/meetings/viewState.ts: meetingViewState(meeting, segmentsCount)
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

    Tests: viewState.test.ts — one case per state, the progress arithmetic, and "no state renders
    an empty headline".
  `,
  acceptance: `
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
  `,
  spec: `
    Panel: pending
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
  `,
  acceptance: `
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/pill
    npm test
    npm run build
  `,
})

ITEMS.push({
  id: 'yap24-NT8', prompt: 'yap24-NT', branch: 'loop/yap24-nt8-calendar-aware-record-prompt', gated: 'panel',
  title: 'Calendar-aware: an upcoming meeting or class raises a "Record?" prompt on the pill — opt-in, EventKit, no OAuth',
  preflight: `
    grep -q "NSCalendarsFullAccessUsageDescription" desktop/src-tauri/Info.plist
    test -f desktop/src-tauri/tests/calendar_prompt_policy.rs
  `,
  spec: `
    Panel: DEFERRED 2026-09-26T17:35:00Z — see docs/loop/DEFERRED.md. Excluded from pass-1 panelApproved (and
    from the yap24-NT chain's intent). It adds a NEW permanent TCC permission (Calendars), a
    5-minute EventKit background poll, and a pill interruption — none of it fixes stop ->
    transcript -> notes, which is the whole mandate of this pass ("the notetaker is not even
    working"). Its own spec already requires the panel AND Wilson to bless it, and it is the
    only pass-1 item whose acceptance can prove nothing beyond a synthetic policy table. 2 of 5
    panel seats named this the single item to kill from pass 1. Reconsider it after yap24-NT9
    has passed on one real meeting on Wilson's own Mac, and only after his explicit Calendars
    yes (ledger: pricing/product identity decisions are his; a new permanent TCC grant belongs
    in the same bucket).
    DEPENDS: yap24-NT6, yap24-NT7

    EVIDENCE
      - Wilson 2026-08-10 (memory project_yap_build_state): "calendar-AWARE (knows when
        meetings/CLASSES are coming — students are a target — and proactively prompts Record?)";
        stack decided then: EventKit via objc2-event-kit, reads iCloud/Google/Outlook through
        Calendar.app, zero OAuth.
      - Wispr ships a meeting reminder pill ("In {{n}} min", Join + Start, snooze) — parity
        teardown Notes/Wispr-Full-Parity-Research-2026-08-09.md line 100.
      - Apple: requestFullAccessToEvents is macOS 14.0+
        (https://developer.apple.com/documentation/eventkit/ekeventstore/requestfullaccesstoevents(completion:));
        purpose string NSCalendarsFullAccessUsageDescription, macOS 14.0+
        (https://developer.apple.com/documentation/bundleresources/information-property-list/nscalendarsfullaccessusagedescription).

    DO
      1. Settings -> Meetings: "Remind me to record meetings and classes" (default OFF). Turning
         it on requests calendar access; below macOS 14 the toggle is disabled with the reason.
      2. A pure policy (calendar_prompt.rs): events with attendees or a video link, or on a
         calendar the user ticked as "classes", starting within 2 min -> one prompt; snooze 2 min;
         never while a dictation or meeting is running; never twice for one event.
      3. The pill shows the prompt (Record / Snooze / Not this one); Record starts the meeting
         with the event title and kind call when the event has a video link, else in_person.
      4. Calendar reads are an EventKit query every 5 minutes at most and on
         EKEventStoreChanged — never a tight poll. Nothing leaves the Mac.

    Tests: tests/calendar_prompt_policy.rs over synthetic events (no EventKit in the test).
  `,
  acceptance: `
    grep -q "NSCalendarsFullAccessUsageDescription" desktop/src-tauri/Info.plist
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test calendar_prompt_policy
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-NT9', prompt: 'yap24-NT', branch: 'loop/yap24-nt9-notetaker-end-to-end-proof', gated: 'panel',
  title: 'Phase-closing proof: one headless command runs a two-track fixture through capture, transcript, notes and export',
  preflight: `
    test -x scripts/notetaker-e2e.sh
    grep -q "yap24" docs/MEETING-DEMO.md
  `,
  spec: `
    Panel: pending
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
  `,
  acceptance: `
    test -x scripts/notetaker-e2e.sh
    bash -n scripts/notetaker-e2e.sh
    ! grep -q 'SKIP' scripts/notetaker-e2e.sh
    YAP_DATA_DIR="$(mktemp -d)/yap-state" YAP_MODEL_DIR="$HOME/code/wilson-voice-loop/cache/models" YAP_EVAL_CORPUS="$HOME/yap-eval-corpus/meetings" YAP_E2E_REQUIRE=1 bash scripts/notetaker-e2e.sh
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib meeting_matrix
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_pipeline_wired
  `,
})

ITEMS.push({
  id: 'yap24-NT10', prompt: 'yap24-NT', branch: 'loop/yap24-nt10-meeting-survives-sleep-and-lid-close', gated: 'panel',
  title: 'A meeting survives a lid close or sleep: the journal finalizes on WillSleep and the row is marked paused_by_sleep, not left recording forever',
  notes: 'NEW item added by Senior Panel synthesis 2026-09-26T17:35:00Z (User/wildcard seat, BLOCKING — grounded). The audit counted Y1-B (register NSWorkspaceWillSleepNotification / IORegisterForSystemPower) as already-done, but its own pre-flight only matches a COMMENT in power.rs, not a real observer call site (meeting_matrix.rs row 16 still reads PolicyOnly{absent_call_site:"NSWorkspaceWillSleepNotification"}; grep -c for that literal returns 4 on main today). only:[\'yap24-NT\'] never reaches Y1-B, so without this item a lid-close mid-meeting (walking between rooms, closing the laptop) leaves the row stuck in "recording" with no finalize, and yap24-NT1\'s reconcile only handles a crash, not a clean sleep. Corrects docs/ARCHITECTURE-AUDIT-2026-09-26.md §10\'s already-done count from 2 to 1 (Y4-D only).',
  preflight: `
    grep -qE "NSWorkspaceWillSleepNotification|IORegisterForSystemPower" desktop/src-tauri/src/power.rs
    test -f desktop/src-tauri/tests/meeting_survives_sleep.rs
    ! grep -q 'absent_call_site: "NSWorkspaceWillSleepNotification"' desktop/src-tauri/src/meeting_matrix.rs
  `,
  spec: `
    Panel: pending
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
  `,
  acceptance: `
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_survives_sleep
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib meeting_matrix
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-NT11', prompt: 'yap24-NT', branch: 'loop/yap24-nt11-speaker-bleed-mic-dedupe-with-route-detection', gated: 'panel',
  title: 'A call on built-in speakers is transcribed once: mic segments that re-hear the system track are dropped, and the dedupe is skipped on headphones',
  notes: 'NEW item added by the Senior Panel round-2 verify 2026-09-26 (THE USER seat, HIGH). NOT in pass 1 (gated panel, left out of the pass-1 panelApproved) — the destination is pass 2, see docs/loop/DEFERRED.md #12. It needs yap24-NT2 (the one-shot two-track merge) and yap24-NT3 (the output-device tracking) merged first. Panel round-3 audit 2026-09-26T23:42:00Z (synthesis of two independent seats — Senior macOS Audio/Rust Engineer and THE USER) revised the spec — see "Panel revisions" below. Still pass-2 only; put back into the pass-2 panelApproved array per Loop-Logs/YAP-RESUME-2026-09-26.md.',
  preflight: `
    test -f desktop/src-tauri/tests/meeting_cross_track_bleed.rs
    grep -q 'fn bleed_score' desktop/src-tauri/src/meeting_asr.rs
  `,
  spec: `
    Panel: audited 2026-09-26 — SOUND-WITH-CHANGES: approved for pass 2, revised (kill the
    output-route on/off switch, add an offline acoustic cross-correlation gate + real track-epoch
    persistence, redefine the dedupe unit as a residue rule, add per-span "Me" attribution) — see
    "Panel revisions" below. Two independent seats converged on the same BLOCKING finding and the
    same kill.
    DEPENDS: yap24-NT2 (the merge stage this dedupe lives in), yap24-NT3 (the whole-meeting output
    device tracking it reuses)

    ### PANEL REVISIONS 2026-09-26T23:42:00Z (Senior Panel synthesis, round 3 — applied,
    BLOCKING + 4x HIGH + 4x MEDIUM, GROUNDED; Senior macOS Audio/Rust Engineer seat and THE USER
    seat converged independently on the BLOCKING clock finding and on killOne):

      - BLOCKING — no shared clock zero exists between the two tracks, so the 500 ms overlap
        window and "same epoch as the track anchors" below have nothing to measure against: each
        track's host_ns is rebased to its OWN first callback (syscapture.rs's TapClock, and the
        mic's cpal base in record.rs), the absolute mach time is never persisted, and
        TrackEpochs::SHARED is, per meeting_asr.rs's own doc comment, "a lie the caller has to
        tell out loud" — no production path ever fills in a real offset (a source grep for
        TrackEpochs:: today finds only tests/two_track_merge_*.rs, nothing under src/). Add an
        NT11 step 0, before the dedupe: at every stream open/reopen, persist the absolute
        first-callback mach time (the tick TapClock already computes as its epoch, and the mic's
        cpal base next to record.rs's mach_absolute_time call) into the per-meeting anchor
        sidecar NT2 already keeps. Build TrackEpochs from those persisted values — never from
        SHARED — inside NT11's own code path (this item, not NT2's). A source-scan test asserts
        TrackEpochs::SHARED has no caller under src/. As a fallback when an epoch is missing
        (e.g. an anchor file written before this landed), estimate it once per meeting from the
        bleed_score correlation peak below, never from a guessed constant.

      - HIGH — the dedupe unit is undefined ("segment" spans word-level or higher depending on
        the ASR model), so text containment on WORD spans reduces to "drop any mic word that also
        appears on the system track nearby" — a real "yeah", "right", or Wilson reading back
        "Tuesday at three" gets deleted. On SEGMENT spans the >= 0.6 ratio cuts both ways: a
        7-word real commitment sitting inside someone else's overlapping 25-word sentence is
        0.78-contained and gets dropped whole (violating this item's own "no real mic word is
        lost" test), while below the ratio the other side's full 25 words survive labelled "Me".
        Replace the ratio rule: for TimedKind::Word, dedupe only a run of 3 or more CONSECUTIVE
        folded mic words matched to a system run inside the measured lag window — an isolated
        single-word match is always kept. For TimedKind::Segment (or higher), text containment
        becomes a residue rule, not a drop/keep binary: remove the matched system run's text from
        the mic span; if the residue has 0 or 1 non-stopword tokens, drop the span; otherwise KEEP
        the residue (word-level spans) or keep the whole span flagged mixed=true (segment-level)
        so NT4 never draws a "Me" action item from it. Text agreement is a NECESSARY condition for
        a drop, never SUFFICIENT on its own — see the acoustic gate below.

      - HIGH — the system WAV is the decisive evidence and this item as first written never uses
        it. Add a pure fn bleed_score(mic: &[f32], sys: &[f32], lag_range) -> (peak_ncc, lag_ms)
        in meeting_asr.rs (16 kHz, band-limited envelopes, windowed FFT cross-correlation — O(n),
        well under 1% of ASR time for a 3-hour meeting) run offline over the two already-finalized
        WAVs (WavWindows::open, never MemoryWindows — 690 MB for a 3-hour meeting). A candidate mic
        run/segment is dropped only when peak_ncc clears a committed threshold AND the text rule
        above agrees. This also satisfies the item's own "measured, not guessed": log the
        per-meeting median measured lag and commit it, next to each device's queried
        kAudioDevicePropertyLatency + safety offset, to docs/BUDGETS.md — "a few hundred ms ...
        through the room" conflated acoustic travel time (~3 ms/m) with this unmeasured clock
        offset; both numbers now get recorded honestly. Still never touches the capture path or
        the WAV files — a re-transcribe still re-derives everything.

      - HIGH — kill the output-route classifier as the dedupe on/off switch (killOne, both seats):
        classifying from the system DEFAULT output device is the wrong device for a call app that
        outputs elsewhere, misses Bluetooth/HDMI/AirPlay speakers under "unknown is treated as
        speakers", and misreads Wilson's own machine today (system_profiler SPAudioDataType shows
        "BH + Headphones: Transport: Unknown" and "BlackHole 2ch: Transport: Virtual"). Gate the
        dedupe instead on MEASURED COUPLING: it runs only where the bleed_score + text evidence
        above actually finds matched runs at a stable lag. On AirPods/headphones no runs match, so
        nothing is dropped, with no device taxonomy required. fn output_route(...) stays in
        syscapture.rs as a DIAGNOSTIC field only, logged next to the suppression count (see
        below) — it never again decides on/off. Do not use the grep for fn output_route as a
        stand-in for "the feature works" (dropped from preflight/acceptance above); the real gate
        is bleed_score plus the matched-run test below.

      - HIGH — per-span attribution: a route change or a tap rebuild leaves a stretch where the
        far side's speech exists ONLY on the mic (dead air across a reopen, or AirPods
        disconnecting back to speakers mid-call) — MicIsMe is decided once per MEETING today
        (diarization_target from whether any system spans exist at all), so that whole stretch is
        labelled "Me" and NT4 attributes their sentence to Wilson. Make the label per SPAN, not
        per meeting: where the route is speakers/unknown and the system track delivered nothing
        for that span (tap hole, reopen gap, or all-zero samples), label the mic speech "Speaker"
        (unattributed) through the existing meetings::speaker_label path, not "Me", and exclude it
        from "Me"-owned action items in NT4. Tests assert labels through speaker_label itself,
        never through hardcoded "You"/"Me"/"Them" string literals in a fixture (the DO text below
        says "You"; the shipped label is "Me").

      - MEDIUM — a route change mid-meeting is exactly when cross-track host time is least
        trustworthy: a reopened stream rebases host_ns to zero, and the segmented timeline cannot
        recover the dead air across the reopen. Record route and reopen boundaries against each
        track's finalized-sample position (the axis meeting.rs::finalized_positions already keeps
        continuous across a reopen), never against raw host_ns. The "speakers -> AirPods" fixture
        below must include a mic reopen at the boundary; assert nothing within +/-1 s of the seam
        is dropped unless the acoustic gate actually fires.

      - MEDIUM — VPIO / capture-time voice-processing echo cancellation is REJECTED as the layer
        for this fix, stated explicitly rather than silently bypassed: it requires both the input
        and output node in voice-processing mode, ducks the call's OWN playback on macOS 14+
        (voiceProcessingOtherAudioDuckingConfiguration), is irreversible (a re-transcribe could
        never recover the raw mic), and would replace the shared cpal mic path dictation also
        uses. The offline system-track reference (bleed_score above) does the same job
        non-destructively, entirely outside the capture path, and re-derives on every
        re-transcribe. Add this sentence to the NOT clause verbatim: "VPIO rejected: it ducks the
        call's own playback, is irreversible, and replaces the shared cpal mic path; the offline
        system-track cross-correlation does the same job non-destructively."

      - MEDIUM — the tests as first specified are text-only ("synthetic ChunkOutcome spans, no
        model"), and meeting_asr.rs's own header warns RNNT/TDT emission times are unstable
        across decodes and must never be used to match one decode's words against another's — yet
        this item matches mic-decode words to system-decode words by time. Add a real paired
        fixture to the NT0 corpus (5 minutes of a call on built-in speakers: mic.wav + sys.wav +
        anchors, with a committed room impulse response at -18 dB / 40 ms delay and a declared
        300 ms epoch offset) and gate on it with the real model via --transcribe-meeting headless:
        remaining duplicated far-side runs <= 5%, mic-only word recall >= 0.98, nothing dropped on
        the headphones fixture, and the "speakers -> AirPods" reopen fixture above all hold. Keep
        the synthetic-span unit tests as Tier A (no model); the paired-WAV run is Tier B, gated
        YAP_EVAL_REQUIRE=1 + YAP_MODEL_DIR like NT0/NT2. Commit both numbers to docs/BUDGETS.md.

      - MEDIUM — suppressed spans must be auditable, not a silent delete-and-count: persist each
        suppressed mic span as its own row flagged suppressed_as_echo=1 (excluded from render,
        FTS, export and NT4 input) instead of the "one diagnostics field" this item first
        specified, so a founder can see and recover a line he remembers saying without a
        re-transcribe. Add this via the NEXT AVAILABLE versioned migration step (never an ad hoc
        ALTER — NT4's revision already claims MIGRATION_6_MEETING_NOTES; this one takes the
        following number when both land). Route spans, now that they are diagnostic-only, live in
        the same per-meeting anchor/journal sidecar NT2 already keeps, cleaned up only through
        NT1's purge/delete path — never independently. yap24-NT6 (its own item, not built here)
        gets a one-line note: "N lines hidden as speaker echo · Show" revealing them inline — that
        UI line is DEFERRED to NT6's own pass.

    EVIDENCE (both seats, converged): syscapture.rs's TapClock rebases each stream's host_ns to
    its own first mHostTime and never persists the absolute tick; meeting_asr.rs's doc comment
    calls TrackEpochs::SHARED "a lie the caller has to tell out loud"; asr_engine.rs's TimedKind
    is None/Segment/Word/Token and meeting_asr.rs's own fixtures already exercise TimedKind::Word;
    meeting.rs::finalized_positions stays continuous across a reopen where host_ns does not;
    system_profiler SPAudioDataType on Wilson's own machine shows "BH + Headphones: Transport:
    Unknown" and "BlackHole 2ch: Transport: Virtual" — both misclassified by a device-taxonomy
    route switch.

    WHY (THE USER seat): a founder's real call is a laptop on a desk with the far side playing out
    of the built-in speakers. The system track (process tap) carries the far side cleanly; the mic
    track ALSO hears it, a few hundred ms later, through the room. Once NT2 transcribes both tracks
    and merges them by host time, every remote sentence appears twice — once as Them (system) and
    once as You (mic) — and NT4's summary then attributes the other side's commitments to Wilson.
    On headphones/AirPods there is no bleed and nothing must be removed.

    DO (original — SUPERSEDED where it conflicts with "Panel revisions" above; kept for context,
    read the revisions first)
      1. SUPERSEDED — output route detection is no longer the dedupe on/off switch (see the HIGH
         kill finding above). Still add fn output_route(...) in syscapture.rs, beside NT3's
         per-meeting device tracking, classifying the default output as speakers | headphones |
         unknown on the same triggers as before, but it now feeds a DIAGNOSTIC log field only —
         never a gate. The real gate is step 2 below.
      2. SUPERSEDED — the dedupe still runs INSIDE NT2's one-shot merge stage (after both tracks'
         ASR finished, before the single transactional write), but the drop decision is now: (a)
         epochs come from the persisted per-track anchors (BLOCKING finding above), never SHARED;
         (b) a candidate window is a text-matched run (3+ consecutive words, or the containment
         residue rule for segment spans — HIGH finding above), gated on peak_ncc from
         fn bleed_score clearing its committed threshold (HIGH finding above) — text agreement
         alone is never sufficient; (c) never trim words out of a kept span/residue.
      3. Audio is never touched — the dedupe and bleed_score both act read-only on the two
         finalized WAVs/segments, so a re-transcribe re-derives everything. SUPERSEDED: suppressed
         mic spans are now their own rows flagged suppressed_as_echo=1 (MEDIUM finding above), not
         a single diagnostics counter; still log how many and on which route per meeting.
      4. Headphones spans skip the dedupe entirely (no matched runs exist there in practice, so
         this now falls out of the measured-coupling gate rather than being special-cased); kind
         in_person meetings (MicOnly per NT3) never run it. NEW: where a speakers/unknown span has
         no system audio to compare against at all (tap hole / reopen gap), label that mic speech
         "Speaker" via meetings::speaker_label, not "Me" (HIGH per-span-attribution finding above).

    NOT: no echo cancellation / DSP on the capture path (VPIO explicitly rejected — see the added
    NOT sentence in the Panel revisions above), no new TCC permission, no change to dictation, no
    second ASR pass. Route classification is a diagnostic field only, never the on/off switch.

    Tests (tests/meeting_cross_track_bleed.rs) — Tier A, synthetic ChunkOutcome spans, no model:
      - speakers route, mic re-hears a system sentence 250 ms late, with a real persisted epoch
        (not SHARED) -> the mic copy is suppressed via a matched run + bleed_score, the system
        copy stays, the merged transcript has the sentence once;
      - the same audio spans on a headphones route (no matched run found) -> nothing suppressed;
      - double-talk: a 7-word mic commitment inside an overlapping 25-word system sentence ->
        the residue (the mic's own 7 words) survives, in both directions of overlap ratio;
      - an isolated single mic word ("yeah") that also appears in a nearby system utterance ->
        kept (never dropped on a single-word match);
      - mic speech with no overlapping system segment -> kept;
      - mic speech on a speakers/unknown route where the system track is silent for that span
        (tap hole) -> labelled "Speaker" via meetings::speaker_label, not "Me";
      - a route change mid-meeting (speakers -> AirPods) with a mic reopen at the boundary ->
        dedupe applies only where bleed_score/text evidence actually matches, and nothing within
        +/-1s of the seam is dropped without that evidence;
      - source scan: no caller of TrackEpochs::SHARED under src/;
      - no real mic word is lost: across the fixture, every word that exists only on the mic track
        survives (mirror meeting_eval's seam_dedupe_never_deletes_real_words posture).

    Tests — Tier B, real model, gated YAP_EVAL_REQUIRE=1 + YAP_MODEL_DIR (MEDIUM finding above):
      - a paired mic.wav/sys.wav fixture (room impulse response, -18 dB, 40 ms delay, declared
        300 ms epoch offset) run through --transcribe-meeting headless: duplicated far-side runs
        <= 5%, mic-only word recall >= 0.98, nothing dropped on the paired headphones fixture.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/meeting_cross_track_bleed.rs
    grep -q 'fn output_route' desktop/src-tauri/src/syscapture.rs
    grep -q 'fn bleed_score' desktop/src-tauri/src/meeting_asr.rs
    ! grep -rn 'TrackEpochs::SHARED' desktop/src-tauri/src
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_cross_track_bleed
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_pipeline_wired
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib meeting
    YAP_DATA_DIR="$(mktemp -d)/yap-state" YAP_EVAL_REQUIRE=1 YAP_EVAL_CORPUS="$HOME/yap-eval-corpus/meetings" YAP_MODEL_DIR="$HOME/code/wilson-voice-loop/cache/models" cargo test --features custom-protocol --test meeting_cross_track_bleed -- --ignored --nocapture --test-threads=1
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo clippy --all-targets --features custom-protocol
  `,
})

// ── 02-yap24-pill-and-trial.mjs ───────────────────────────────────────────
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
    grep -q "14d" desktop/src/pill/license.test.ts
    ! grep -q "trial, more than 7 days   → nothing" desktop/src/pill/license.ts
  `,
  spec: `
    Panel: pending (product copy/threshold — the panel may keep a quiet style, it may not keep silence)
    DEPENDS: Y2-B (open PR #186 — the numeral in both pill styles and the 30px side dock).
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
  `,
  acceptance: `
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/pill/license.test.ts
    npm test
    npm run build
  `,
})

ITEMS.push({
  id: 'yap24-PILL2', prompt: 'yap24-PILL', branch: 'loop/yap24-pill2-backend-stage-events-for-polish-and-paste', gated: 'panel',
  title: 'The backend says which stage a take is in, so "polishing" and "pasting" finally appear on the pill',
  preflight: `
    grep -q "TAKE_STAGE_EVENT" desktop/src-tauri/src/lib.rs
    grep -q "take_stage" desktop/src/float-main.tsx
  `,
  spec: `
    Panel: pending
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
  `,
  acceptance: `
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/pill
    npm run build
    cd src-tauri
    cargo test --features custom-protocol --lib take_stage
    cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-PILL3', prompt: 'yap24-PILL', branch: 'loop/yap24-pill3-blind-hotkey-and-blind-paste-states', gated: 'panel',
  title: 'When the hotkey cannot hear you, the pill says so: Secure Input, a disabled tap, and missing Accessibility are pill states',
  preflight: `
    grep -q "secure_input" desktop/src/float-main.tsx
    grep -q '"blind"' desktop/src/pill/live.ts
  `,
  spec: `
    Panel: pending
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
  `,
  acceptance: `
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/pill
    npm test
    npm run build
  `,
})

ITEMS.push({
  id: 'yap24-PILL4', prompt: 'yap24-PILL', branch: 'loop/yap24-pill4-hands-free-looks-different-from-hold', gated: 'panel',
  title: 'Hands-free looks different from hold: a lock mark and "tap fn⌃ to stop", in both characters and all docks',
  preflight: `
    grep -q "hands_free" desktop/src/float-main.tsx
    grep -q "hands-free" desktop/src/pill/live.test.ts
  `,
  spec: `
    Panel: pending
    DEPENDS: yap24-PILL2 (same reducer), Y5-K when merged (characters are data)

    EVIDENCE
      - lib.rs:896 status carries hands_free; the pill never reads it (float-main.tsx:60-65).
      - ptt_macos.rs:34 DOUBLE_TAP_MS = 450, :550 double-tap -> hands-free ON; Wilson's log shows
        dozens of hands-free takes, some several minutes long, with the same pill as a hold.

    DO: a listening variant "listening-locked" (phaseVisual label "Hands-free — tap fn⌃ to stop"),
    drawn by the shell as a small lock chip; the live commentary keeps running; Escape/cancel copy
    unchanged. Tests in live.test.ts.
  `,
  acceptance: `
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/pill
    npm run build
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
    Panel: pending
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
  `,
  acceptance: `
    ! grep -q "with Whisper" desktop/src-tauri/Info.plist
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
    ! grep -q "com.apple.security.cs.allow-jit" desktop/src-tauri/Entitlements.plist
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
  `,
  acceptance: `
    grep -q "<false/>" desktop/src-tauri/Entitlements.plist
    grep -q "com.apple.security.app-sandbox" desktop/src-tauri/Entitlements.plist
    grep -q "Entitlement audit" docs/RELEASE.md
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
    Panel: pending
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
    replace it) in docs/BUDGETS.md; Secure Input is checked on key-down and on a slow backstop
    (>= 10 s) instead of every 2 s; the space-keeper runs on NSWorkspace active-space-change and
    screen-change notifications instead of a tick where the API allows; a test enumerates the
    registered timers and fails if any idle-time interval drops below 10 s.
  `,
  acceptance: `
    cd desktop && npm ci && cd src-tauri
    cargo test --features custom-protocol --test idle_timer_budget
    cargo test --features custom-protocol --lib secure_input
    cargo clippy --all-targets --features custom-protocol
  `,
})

// ── 04-yap24-ui-pixels-motion.mjs ─────────────────────────────────────────
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
    Panel: pending
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
  `,
  spec: `
    Panel: pending
    DEPENDS: yap24-PILL1..PILL4, yap24-UI1, Y5-B/Y5-C/Y5-D

    EVIDENCE: the 2026-09-15 run's builders logged "NOT visually QA'd" on Y2-B, Y5-B, Y5-C, Y5-D
    (memory project_yap_loop_state_20260912; STATUS-yap.md). A green gate cannot see a pill.

    DO: scripts/pill-matrix.mjs reuses smoke-windowed.mjs's headless Chrome (--headless=new) and
    vite preview, renders float.html with each phase forced through a dev-only query parameter,
    captures PNGs for every phase x {bottom,left,right} x {classic,yappy}, and fails on the same
    structural conditions (empty text, clipped text, overflow). Output under
    docs/pr-screenshots/yap24-UI3/. Never launches the app, never opens a visible window.
  `,
  acceptance: `
    test -f scripts/pill-matrix.mjs
    node --check scripts/pill-matrix.mjs
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
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
  notes: `STATUS 2026-09-26: MERGED (PR #157) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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
  notes: `STATUS 2026-09-26: MERGED (PR #158) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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
  notes: `STATUS 2026-09-26: MERGED (PR #161) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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
  notes: `STATUS 2026-09-26: MERGED (PR #162) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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
  notes: `STATUS 2026-09-26: MERGED (PR #164) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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
  notes: `STATUS 2026-09-26: MERGED (PR #165) in run wf_f6cd9dfa-860. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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
  notes: `STATUS 2026-09-26: NOT ALREADY-DONE — Panel correction 2026-09-26T17:35:00Z (Senior Panel synthesis, BLOCKING, GROUNDED, User/wildcard seat): the earlier ALREADY-DONE call was wrong. The pre-flight's grep matches power.rs line 18, a doc comment ('//! ... matrix row #16's NSWorkspaceWillSleepNotification path'), not a real registration — confirmed firsthand: power.rs has zero real call sites, and meeting_matrix.rs still carries FOUR absent_call_site: "NSWorkspaceWillSleepNotification" entries today, which is this item's OWN acceptance criterion for done (test 0 -eq grep -c ...) and it reads 4, not 0. Correct docs/ARCHITECTURE-AUDIT-2026-09-26.md section 10's already-done count from 2 to 1 (Y4-D only). This item is real, unbuilt work. A scoped notetaker pass (only:['yap24-NT']) never reaches it by prefix, so yap24-NT10 (added to 01-yap24-notetaker.mjs by this same panel) ships a meetings-only subset of this fix inside pass 1; when Y1-B itself is later built, wire its one publisher into NT10's meeting consumer instead of registering a second NSWorkspace observer. PANEL ROUND-2 VERIFY 2026-09-26: the pre-flight is now this item's OWN acceptance lines (absent_call_site count == 0, the DidWake registration, the fan-out and unregister test names, the row-16 test) — the old doc-comment grep plus the already-passing policy-only matrix_row16 test retired it as done once and must not again. Note yap24-NT10 alone cannot satisfy it: NT10 flips the absent_call_site rows but never adds the dictation/permissions fan-out test.`,
  title: 'Register the sleep/wake observer two later items already claim exists',
  preflight: `
    test 0 -eq "$(grep -c 'absent_call_site: "NSWorkspaceWillSleepNotification"' desktop/src-tauri/src/meeting_matrix.rs)"
    grep -q 'NSWorkspaceDidWakeNotification' desktop/src-tauri/src/power.rs
    grep -q 'one_observer_fans_out_to_dictation_meetings_and_permissions' desktop/src-tauri/tests/matrix_row16_sleep_wake.rs
    grep -q 'the_observer_is_unregistered_on_shutdown' desktop/src-tauri/tests/matrix_row16_sleep_wake.rs
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
  notes: `STATUS 2026-09-26: MERGED (PR #167) in run wf_f6cd9dfa-860 — acceptance failed twice before merge (needs-human): the review pass re-checks it. Pre-flight on main should retire it; if pre-flight fails, the merged work regressed — say so, do not rebuild blind.`,
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

// ── 06-yap-flakes-2026-10-16.mjs ──────────────────────────────────────────
// yap-flakes-2026-10-16 — MAIN GOES GREEN. Written 2026-10-16 by the yap-prep seat.
// Sources: RESUME §11.7 ("fix it, assert payload elapsed_seconds, it has cost three investigations"),
// 10-06 context Fact 11 + open item 16. Main CI (workflow "CI", job "Tauri build (Rust + frontend)",
// step "cargo test") failed on the last three main runs: 36256562076 (63452bb), 36270319107
// (90d7861), 36343744919 (6f40697). Their failed-step logs were no longer retrievable on 2026-10-16
// (gh run view --log-failed returned nothing), so WHICH test failed on each run is UNVERIFIED; the
// two named tests are the recorded suspects. FIX, no panel (unambiguous intent).
// Runs before every other 2026-10-16 item: every later gate inherits a red main otherwise.
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap-flakes-2026-10-16', prompt: 'yap-flakes', branch: 'loop/yap-flakes-2026-10-16-main-green', gated: null,
  title: 'Main goes green: per_chunk_deadline_not_per_take and meeting_manual_start_stop stop asserting on the scheduler',
  preflight: `
    test -f docs/loop/FLAKES-2026-10-16.md
    test -f scripts/flake-soak.sh
    grep -q 'per_chunk_deadline_not_per_take' docs/loop/FLAKES-2026-10-16.md
    grep -q 'meeting_manual_start_stop' docs/loop/FLAKES-2026-10-16.md
  `,
  spec: `
    Panel: not required — FIX (unambiguous intent: main must be green before the loop lands anything)
    DEPENDS: none

    EVIDENCE
      - desktop/src-tauri/tests/dictation_chunked.rs:305 per_chunk_deadline_not_per_take: a 300 ms
        per-chunk budget against a fake decode that sleeps 200 ms per window, twelve windows. Each
        window has 100 ms of wall-clock headroom. A loaded box (CI, or two loop lanes compiling)
        that delays one sleep or one thread wake by more than 100 ms trips the per-chunk deadline,
        chunks_failed becomes 1, and the test fails while the code under test is right. Recorded
        red on main at 90d7861 (Obsidian Projects/Yap.md, 2026-09-27).
      - desktop/src-tauri/tests/meeting_manual_start_stop.rs: the YV111 tick-count window was
        already replaced by support::wait_until on the ticker's own output (around line 145), but
        RESUME §11.7 still names this test as the wall-clock flake, and asks for the assertion to
        sit on the PAYLOAD: every recording status's elapsed_seconds checked against the sink's
        receipt Instant (struct Emit, around line 39), not on how many emits a window caught.
      - Three consecutive red main runs on step "cargo test" (ids in the header).

    DO
      1. REPRODUCE FIRST, before any edit. Write scripts/flake-soak.sh <test-target> <filter> <runs>:
         it starts one busy loop per CPU (sysctl -n hw.ncpu; yes > /dev/null), records their PIDs,
         traps EXIT to kill exactly those PIDs, runs "cargo test --features custom-protocol --test
         <test-target> <filter>" <runs> times from desktop/src-tauri, reads every exit code bare,
         prints "<passes>/<runs>", and exits non-zero on any failure. Run it 30x for each test on
         unmodified main and write the counts to docs/loop/FLAKES-2026-10-16.md. If a test passes
         30/30 under load, write that down and harden only what the evidence supports.
      2. per_chunk_deadline_not_per_take: keep what it proves (the deadline is spent per WINDOW, not
         per take: the take's total decode time exceeds the budget while no single window does) and
         remove the scheduler dependence. Preferred: drive the deadline from an injectable clock
         (a test-only seam, no behaviour change) so elapsed time is simulated. Fallback: about 10x
         per-window headroom with more windows (for example 24 windows x 50 ms against a 1 s budget:
         1.2 s total exceeds the budget, each window keeps 950 ms of headroom).
      3. meeting_manual_start_stop: assert on the payload. For every recording status the sink
         received, elapsed_seconds never decreases, and it stays within 1 s plus one tick of
         (receipt Instant minus the start Instant). support::wait_until stays the only wait,
         TICK_DEADLINE stays the death detector, and no sleep-then-count shape exists in the file.
      4. Sweep desktop/src-tauri/tests/ for the same shape (a sleep followed by a count, or a
         wall-clock upper bound under 1 s) and list every hit in the doc. Fix only the two named
         tests here; file the rest as new numbered entries in docs/loop/DEFERRED.md.
      5. Never add #[ignore], never add a retry loop, never widen a bound past what the doc's
         numbers justify. The PR body carries the before and after soak counts.
  `,
  acceptance: `
    test -f docs/loop/FLAKES-2026-10-16.md
    test -x scripts/flake-soak.sh
    grep -q 'elapsed_seconds' desktop/src-tauri/tests/meeting_manual_start_stop.rs
    ! grep -qF '#[ignore]' desktop/src-tauri/tests/dictation_chunked.rs
    ! grep -qF '#[ignore]' desktop/src-tauri/tests/meeting_manual_start_stop.rs
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    bash scripts/flake-soak.sh dictation_chunked per_chunk_deadline_not_per_take 30
    bash scripts/flake-soak.sh meeting_manual_start_stop '' 30
  `,
})

// ── 07-yap-lifecycle-2026-10-16.mjs ───────────────────────────────────────
// yap-lifecycle-2026-10-16 — QUIT MEANS QUIT, REOPEN MEANS A WINDOW. Written 2026-10-16 by the
// yap-prep seat from the 10-06 prompt D6 and the 10-06 context Facts 12-14 / open item 1.
// Wilson, 2026-10-06: "the way it shuts down is wrong — I can't open the app but I can still see
// the sideways pill." FIX, no panel (unambiguous intent). Item #1 of the 10-06 new items.
// Headless only: no agent launches the GUI app (see HAZARD below). SHARED PREAMBLE + STANDARD
// GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap-lifecycle-2026-10-16', prompt: 'yap-lifecycle', branch: 'loop/yap-lifecycle-2026-10-16-quit-reopen-teardown', gated: null,
  title: 'Quit means quit, reopen means a window: RunEvent::Reopen, one clear Quit, the pill gone at quit, bounded teardown, RSS flat under load',
  preflight: `
    grep -q 'RunEvent::Reopen' desktop/src-tauri/src/lib.rs
    test -f desktop/src-tauri/tests/lifecycle.rs
    test -f scripts/lifecycle-soak.sh
  `,
  spec: `
    Panel: not required — FIX (10-06 prompt D6: this is item #1 of the new items)
    DEPENDS: yap-flakes-2026-10-16 (a green main). Independent of yap24-NT and yap24-PILL.

    EVIDENCE (10-06 context Facts 12-14; line numbers re-checked on main 6f40697, 2026-10-16)
      - lib.rs ~5605: the main window's CloseRequested handler calls prevent_close() and hide():
        the red X hides the window and never quits. The activation policy is Regular (~5586), so
        the Dock icon stays.
      - lib.rs ~6149: the .run() closure matches only RunEvent::Exit. Nothing in src/ handles
        RunEvent::Reopen, so a Dock click or a Finder double-click on a Yap whose window is hidden
        shows nothing. That is "I can't open the app".
      - lib.rs ~5342: the single-instance callback calls focus_main_window (~4535), yet yap.log shows
        "second instance launched" once (2026-09-12) in the whole log.
      - The pill is an NSPanel in the same process (float_pill.rs), so a visible pill means the
        process is alive. Tray Quit (~5837) stops any meeting, checkpoints the WAL, then app.exit(0).
      - Memory: Wilson's running Yap was 2.2 GB RSS after 2.5 days on 10-06 (pid 23686) and 1.4 GB
        after 2d15h on 2026-10-16 (pid 1640), against a ~928 MB lazy-load start. A leak is
        suspected and UNMEASURED.

    DO
      1. RunEvent::Reopen (has_visible_windows false) shows, unminimizes and focuses the main window
         through ONE function shared with the single-instance callback and the tray "Open Yap".
         Put the decision in a pure function (event, window state) -> action with unit tests; the
         run-loop arm only dispatches.
      2. ONE unambiguous Quit. Cmd-Q (app menu), tray "Quit Yap" and the Dock's Quit all reach the
         same quit path. The red X keeps hiding (Yap is a menu-bar utility), and the FIRST time it
         hides, the window's own surface says once where Yap went and how to quit. No modal.
      3. The pill is ordered out as the FIRST step of quit, before the meeting stop and the WAL
         checkpoint, so a slow teardown never leaves a pill on screen.
      4. Bounded teardown. Every quit step has a deadline, the whole quit finishes within 3 s, and a
         step that misses its deadline is logged and skipped before the process exits. Log one
         "exit teardown <ms>" line with per-step timings.
      5. Stale instance. If the single-instance handoff to a running instance gets no answer
         within 2 s, log it and say so instead of exiting silently.
      6. RSS flat under load. scripts/lifecycle-soak.sh <runs> runs the BUILT binary headless
         (YAP_DATA_DIR="$(mktemp -d)/yap-state", --transcribe-file
         tests/fixtures/quick-brown-fox-16k.wav, or a --smoke soak mode if the builder adds one) in
         a loop, samples RSS with ps every iteration, prints the series, and exits non-zero when
         RSS after the warm-up iterations grows more than 10 percent. Find and fix what it shows
         (idle model unload; tie to yap24-OS5 "idle means idle"). Record the numbers in the PR.
      7. HAZARD: a GUI launch of a second copy reaches the single-instance plugin and would FOCUS
         WILSON'S RUNNING YAP. No agent launches the GUI app. Every automated proof is a cargo test
         or the headless CLI path (cli::handle_args exits before the Tauri builder). Never kill or
         signal any /Applications/Yap.app process.
      8. Keep Tauri; app-sandbox stays false; never rename the bundle id or the data dir.

    WILSON'S WALK (after merge, on his installed build; NOT an agent step, the PR body lists it):
      scripts/rebuild_app.sh; close the window, click the Dock icon: the window shows; Cmd-Q: the
      process is gone AND the pill is gone; relaunch: the window shows; open -a Yap a second time:
      the first instance focuses.
  `,
  acceptance: `
    grep -q 'RunEvent::Reopen' desktop/src-tauri/src/lib.rs
    test -f desktop/src-tauri/tests/lifecycle.rs
    test -x scripts/lifecycle-soak.sh
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    (cd desktop/src-tauri && cargo test --features custom-protocol --test lifecycle)
    test "$(cd desktop/src-tauri && cargo test --features custom-protocol --test lifecycle -- --list 2>/dev/null | grep -c ': test$')" -ge 4
    (cd desktop/src-tauri && cargo build --release --features custom-protocol)
    bash scripts/lifecycle-soak.sh 40
  `,
})

// ── 07-yap-p0-broken-2026-10-17.mjs ───────────────────────────────────────
// yap-p0-broken-2026-10-17 — "NONE OF THE HOTKEYS ARE WORKING, PERMISSIONING, THE BRIDGES, NOTHING'S
// WORKING." Written 2026-10-17 (07:46Z) by the yap-p0-items seat from Wilson's report on Yap 0.9.0
// (the Developer-ID-signed build installed 2026-10-16). His observation is ground truth. FIX, no panel.
// One file = one lane, run in order: hotkeys first (a take must start with zero grants), then the
// truthful probe + Repair (the hotkey item's degraded UI calls it), then insertion, then the release
// smoke that would have stopped 0.9.0 shipping.
//
// LAUNCH SEQUENCE (fix round 2026-10-17, second reviewer B1). Each item builds on the previous item's
// code, every builder starts from origin/main, and build mode never lands an unreviewed PR. So items
// 2-4 carry a DEPENDENCY preflight: when the previous item's symbols are not on main, the preflight
// prints DEPENDENCY-NOT-ON-MAIN and exits 0, and the harness records "skipped: dependency not on main"
// (template.mjs runBuild) with no builder and no PR. The operator alternates, one item per round:
//   build {mode:'build', lanes:1, only:['yap-p0-']} -> review {mode:'review'} (lands item 1)
//   -> build (item 1 already-done, item 2 builds) -> review -> build (item 3) -> review -> build (item 4)
//   -> review. Stop when all four report already-done. Never build two of these in one pass.
//
// Review: Opus only (review-opus + review-opus2). No Codex, no Gemini, no third-party reviewer.
// HAZARD (same as 07/08): no agent launches the GUI app, posts events into, signals or quits Wilson's
// running /Applications/Yap.app, or writes TCC on this machine (no tccutil run by a seat, no sqlite
// writes). The APP may run `tccutil reset <Service> com.wilsonguenther.wilson-voice` for ITSELF when
// the user clicks Repair; a seat only unit-tests the argv it would build. Every automated proof is a
// cargo test, a vitest, or a script self-test. SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.
//
// NO GUESSING (binds every item below): before writing code, the builder PROVES the root cause with a
// probe it ran (a cargo test that fails on main, a script self-test, or a read-only log/codesign line)
// and pastes that output into the PR body under "Root cause, proven". A PR whose body has no probe
// output is incomplete and the review seat blocks it.
//
// EVIDENCE FILE (first reviewer advisory): the "Root cause, proven" probe output ALSO goes into
// docs/loop/evidence/<item id>.md, first line exactly "ROOT-CAUSE-PROBE <item id>", then the raw output.
// Each acceptance checks the file is non-empty and carries that item's probe marker lines.
//
// SYMBOLS ARE CONTRACT. The function, module, event and test names in each spec are what the later
// items' dependency preflights and this item's acceptance grep for. Do not rename them.
//
// DIAGNOSIS (main b853b83, read-only, 2026-10-17):
//   HOTKEYS. Dictation start/stop has exactly ONE keyboard path: a listen-only CGEventTap at
//     kCGHIDEventTap (ptt_macos.rs:822-848, CGEventTapCreate at :826). That path needs Input
//     Monitoring (ListenEvent) or Accessibility, and is blind under Secure Input (secure_input.rs:1-9).
//     The Carbon RegisterEventHotKey path (tauri-plugin-global-shortcut, shortcuts.rs:23, lib.rs:6933-6971)
//     needs NO TCC grant and is immune to Secure Input, but registers only ⌃⌘V paste-last, ⌃⌘M meeting,
//     ⌃⌘Z undo, and the ⌘⇧V dictation toggle, which is OFF by default (lib.rs:498 keep_cmd_shift_v:
//     false; gated at lib.rs:6965). So with a dead grant, or with Terminal's Secure Keyboard Entry on,
//     there is no keyboard way to start or stop a take. lib.rs:6800 only LOGS "Accessibility not
//     trusted" and starts the tap anyway; lib.rs:6888 sets hotkey_registered = true unconditionally,
//     before the tap thread has even tried (a failed install at ptt_macos.rs:859-863 sets RUNNING false
//     and never retries), so the status can claim a hotkey that does not exist.
//     yap.log 2026-10-17: 07:40:34Z "Accessibility not trusted"; fn double-tap did start takes at
//     07:40:37Z and 07:44:01Z; then 6x "tap disabled by kCGEventTapDisabledByUserInput" 07:44:31Z-07:45:48Z;
//     Wilson relaunched Yap 4 times 07:43:07Z-07:43:42Z; 07:47:47Z "Secure Input ENABLED by Terminal (pid
//     1407) — the fn PTT event tap is blind". secure_input.rs:20-27 says no Carbon fallback is possible
//     because Carbon cannot express fn. True for fn, but it is the reason a fixed non-fn chord must
//     exist, not a reason to have none.
//   PERMISSIONS. Grants are read live: AXIsProcessTrusted (permissions.rs:129-145), IOHIDCheckAccess
//     listen (permissions.rs:407-418). CGPreflightListenEventAccess / CGPreflightPostEventAccess are not
//     called anywhere in the app (only in scripts/qa/perm-helper.swift:16 and fn-post.swift:32). Stale
//     identity is detected ONLY from a ledger of the last signature a grant read TRUE under
//     (perm_truth.rs:128-149); no stored signature -> silent. The ledger shipped in #211 (d6cd36a),
//     AFTER release 0.9.0 (ecef342), so on Wilson's Mac it cannot fire for the dev->Developer ID switch.
//     There is no repair path: tccutil appears nowhere in the tree; the banner can only open the pane
//     (permissions.rs:855-891) and tell the user to remove and re-add by hand.
//   BRIDGES. The insertion path into the focused app: paste.rs:184-190 refuses to paste when
//     AXIsProcessTrusted is false (copy only, "Enable Accessibility for Yap to auto-paste"); otherwise
//     paste_tx.rs:252 publish_and_paste puts the text on the pasteboard and paste.rs:391-442 posts ⌘V
//     with CGEventPost at the HID tap; focus.rs:56/102 read the focused element through AXUIElement.
//     Every take in the 07:40Z-07:46Z window logged pasted=false. NOTE: CGEventPost of keystrokes is
//     itself gated by the PostEvent TCC service (listed under Accessibility in Settings): fn-post.swift:32
//     already preflights CGPreflightPostEventAccess for exactly that reason. So "⌘V without Accessibility"
//     is NOT assumed to work; the probe decides, and the zero-grant floor is the clipboard + a Copy card.
//     Separately, the frontend calls the Rust side the "backend bridge": appShell.ts:351-391 boots with ONE
//     Promise.all over ~12 invokes, and any single rejection replaces the status with "Bridge error: ..."
//     (App.tsx:36-38 "UI failed to load backend bridge"). Which of the two Wilson meant is unproven; item 3
//     must probe both.
//   QA HARNESS (#210): scripts/qa/perm-probe.sh (read-only identity/signature/duplicates, --compare
//     reports dr_change), scripts/qa/hotkey-walk.sh (posts real fn edges into the installed Yap, needs
//     YAP_QA_DRIVE_RUNNING=1; --self-test posts nothing), scripts/qa/perm-helper.swift, fn-post.swift.

ITEMS.push({
  id: 'yap-p0-hotkeys-no-permission', prompt: 'yap-p0-hotkeys-no-permission', branch: 'loop/yap-p0-hotkeys-carbon-primary-zero-tcc', gated: null,
  title: 'A take starts and stops with ZERO TCC grants: a fixed-chord dictation hotkey on Carbon RegisterEventHotKey is the primary path, the fn event tap is the extra, and a dead tap is shown as a degraded mode instead of a working hotkey',
  preflight: `
    test -f desktop/src-tauri/tests/hotkey_no_tcc.rs
    grep -q 'DICTATE_TOGGLE.shortcut()' desktop/src-tauri/src/lib.rs
    grep -q 'HotkeySource::Carbon' desktop/src-tauri/src/lib.rs
    grep -q 'set_fn_tap_status(' desktop/src-tauri/src/ptt_macos.rs
    grep -q 'hotkey-walk --chord' scripts/qa/hotkey-walk.sh
  `,
  spec: `
    Panel: not required — FIX (Wilson 2026-10-17: "none of the hotkeys are working")
    DEPENDS: a green main. Blocks: yap-p0-perm-repair (its dependency preflight greps this item's
      DICTATE_TOGGLE.shortcut() registration and HotkeySource::Carbon dispatch in lib.rs).

    REQUIRE (before any code — NO GUESSING)
      - Write tests/hotkey_no_tcc.rs FIRST and run it on unmodified main: with every grant denied, it must
        show that no registered chord starts a take (only the tap does) and that hotkey_registered reads
        true anyway. Paste that failing output into the PR body as "Root cause, proven" AND into
        docs/loop/evidence/yap-p0-hotkeys-no-permission.md (see EVIDENCE FILE above).
      - Quote the yap.log lines from the diagnosis above (read-only grep, never edit the log).
      - Prove the chosen chord is not a macOS system hotkey on this machine: paste
        \`defaults read com.apple.symbolichotkeys AppleSymbolicHotKeys\` evidence for it. Do not guess.

    DO (names are contract)
      1. shortcuts.rs: add DICTATE_TOGGLE, scope AlwaysGlobal, registered UNCONDITIONALLY at launch in
         lib.rs with h.global_shortcut().register(shortcuts::DICTATE_TOGGLE.shortcut()) (Carbon
         RegisterEventHotKey: no Accessibility, no Input Monitoring, immune to Secure Input). Default chord
         ⌃⌥⌘Space unless the REQUIRE probe shows a conflict (then the nearest free ⌃⌥⌘<key>, named in the
         PR). It passes tests/tray_hotkey_no_collision.rs. ⌘⇧V stays opt-in. Settings shows it as the
         "Works everywhere" hotkey and lets the user rebind it (Code + modifiers only).
      2. ONE dispatcher for every hotkey edge: fn take_hotkey_edge(app, state, edge: HotkeyEdge,
         source: HotkeySource) in lib.rs, with HotkeySource::{Carbon, FnTap} and HotkeyEdge::{Press,
         Release}. The global-shortcut handler's DICTATE_TOGGLE branch calls
         take_hotkey_edge(.., HotkeySource::Carbon); the ptt_macos PttEvent::Start / PttEvent::Stop arms
         at lib.rs ~6827/6856 call take_hotkey_edge(.., HotkeySource::FnTap). Press = start (or stop if a
         take is live); a press held past ptt_macos::HOLD_ARM_MS then Release = stop, so hold-to-talk works
         on Carbon too (ShortcutState::Released). No second start/stop code path survives.
      3. The fn / fn⌃ event tap stays, for chords Carbon cannot express, and is ALWAYS ATTEMPTED: at
         launch and again on every permission-watch edge, never gated on the probe (yap.log 07:40:34Z-
         07:40:39Z: fn started a take while AXIsProcessTrusted said "not trusted"). The probe drives only
         status and UI. A tap that failed to install is retried on the next edge instead of dying once at
         ptt_macos.rs:859-863. With valid grants, fn start/stop behaves exactly as on main.
         Carbon chord + fn must never double-start: take_hotkey_edge ignores a Press from one source while
         a take started by the other is live, except as a stop.
      4. Truthful status: hotkey_registered and its unconditional true at lib.rs:6888 are deleted. AppStatus
         carries hotkeys: {carbon: Registered|Failed(reason), fnTap: Delivering|ArmedNoDelivery|
         Blocked(reason)|SecureInput(holder)}. Blocked means "attempted, nothing delivered", never "not tried". set_carbon_status(..) is called in lib.rs with the Ok/Err of
         each DICTATE_TOGGLE register call; set_fn_tap_status(..) is called in ptt_macos.rs from run_tap's
         install_tap result, from recreate_tap, and from the first delivered edge. "Delivering" needs a
         delivered edge, never "the thread started".
      5. Degraded mode UI, explicit: when fnTap is not Delivering, the pill/home line reads
         "fn is off — <reason>. Use <DICTATE_TOGGLE label> (works without permissions)." with a Repair
         button (until yap-p0-perm-repair lands it opens the exact pane). Under Secure Input the line
         names the holder ("Terminal has Secure Keyboard Entry on").
      6. A registrar seam: the registration + arming code takes trait objects for the OS (Carbon registrar,
         permission probe, event source), so tests drive it with grants denied and no real OS hooks.
      7. tests/hotkey_no_tcc.rs, these exact test names, each with real assertions (>= 12 assert! /
         assert_eq! in the file in total): zero_grants_dictate_toggle_starts_and_stops,
         zero_grants_hold_release_stops_take, zero_grants_fn_tap_attempted_status_blocked,
         failed_registration_never_reported_registered, secure_input_carbon_still_starts_take,
         grant_arriving_later_arms_tap_without_restart, valid_grants_fn_start_stop_unchanged,
         probe_denied_but_tap_delivering_stays_armed, carbon_and_fn_never_double_start (>= 18 asserts in
         total with these). zero_grants_dictate_toggle_starts_and_stops
         prints exactly one line "hotkey_no_tcc: grants=none source=carbon started=true stopped=true"
         (println!, after its asserts) — acceptance greps it under --nocapture.
      8. scripts/qa/hotkey-walk.sh gains \`--chord\` ("hotkey-walk --chord" in the usage text): posts the
         DICTATE_TOGGLE chord (fn-post.swift gains a \`chord:<keycode>:<flags>\` op) and asserts
         "recording started" then the take-ended line; plus scripts/qa/fixtures/yap-walk-chord-pass.log,
         exercised by --self-test (which posts nothing). Same exit codes as today.

    WILSON'S WALK (PR body): with Yap's Accessibility and Input Monitoring rows REMOVED in Settings, press
    the chord in TextEdit: a take starts and stops; the pill names the degraded fn mode.
    YAP_QA_DRIVE_RUNNING=1 bash scripts/qa/hotkey-walk.sh --chord when he is not dictating.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/hotkey_no_tcc.rs
    test -f scripts/qa/fixtures/yap-walk-chord-pass.log
    grep -q 'pub const DICTATE_TOGGLE' desktop/src-tauri/src/shortcuts.rs
    grep -q 'DICTATE_TOGGLE.shortcut()' desktop/src-tauri/src/lib.rs
    grep -q 'HotkeySource::Carbon' desktop/src-tauri/src/lib.rs
    grep -q 'HotkeySource::FnTap' desktop/src-tauri/src/lib.rs
    test "$(grep -c 'take_hotkey_edge(' desktop/src-tauri/src/lib.rs)" -ge 4
    ! grep -q 'hotkey_registered' desktop/src-tauri/src/lib.rs
    grep -q 'set_carbon_status(' desktop/src-tauri/src/lib.rs
    test "$(grep -c 'set_fn_tap_status(' desktop/src-tauri/src/ptt_macos.rs)" -ge 3
    grep -q 'hotkey-walk --chord' scripts/qa/hotkey-walk.sh
    test "$(grep -cE 'assert(_eq|_ne)?!' desktop/src-tauri/tests/hotkey_no_tcc.rs)" -ge 18
    test -s docs/loop/evidence/yap-p0-hotkeys-no-permission.md
    grep -q '^ROOT-CAUSE-PROBE yap-p0-hotkeys-no-permission$' docs/loop/evidence/yap-p0-hotkeys-no-permission.md
    grep -q 'kCGEventTapDisabledByUserInput' docs/loop/evidence/yap-p0-hotkeys-no-permission.md
    grep -q 'AppleSymbolicHotKeys' docs/loop/evidence/yap-p0-hotkeys-no-permission.md
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    mkdir -p desktop/target
    (cd desktop/src-tauri && cargo test --features custom-protocol --test hotkey_no_tcc -- --list) > desktop/target/p0-hk-list.txt 2>/dev/null
    grep -q '^zero_grants_dictate_toggle_starts_and_stops: test$' desktop/target/p0-hk-list.txt
    grep -q '^zero_grants_hold_release_stops_take: test$' desktop/target/p0-hk-list.txt
    grep -q '^zero_grants_fn_tap_attempted_status_blocked: test$' desktop/target/p0-hk-list.txt
    grep -q '^failed_registration_never_reported_registered: test$' desktop/target/p0-hk-list.txt
    grep -q '^secure_input_carbon_still_starts_take: test$' desktop/target/p0-hk-list.txt
    grep -q '^grant_arriving_later_arms_tap_without_restart: test$' desktop/target/p0-hk-list.txt
    grep -q '^valid_grants_fn_start_stop_unchanged: test$' desktop/target/p0-hk-list.txt
    grep -q '^probe_denied_but_tap_delivering_stays_armed: test$' desktop/target/p0-hk-list.txt
    grep -q '^carbon_and_fn_never_double_start: test$' desktop/target/p0-hk-list.txt
    (cd desktop/src-tauri && cargo test --features custom-protocol --test hotkey_no_tcc -- --exact zero_grants_dictate_toggle_starts_and_stops --nocapture) > desktop/target/p0-hk-crux.txt 2>&1
    grep -q '^hotkey_no_tcc: grants=none source=carbon started=true stopped=true$' desktop/target/p0-hk-crux.txt
    grep -q 'test result: ok. 1 passed' desktop/target/p0-hk-crux.txt
    (cd desktop/src-tauri && cargo test --features custom-protocol --test hotkey_no_tcc)
    (cd desktop/src-tauri && cargo test --features custom-protocol --test tray_hotkey_no_collision)
    (cd desktop/src-tauri && cargo test --features custom-protocol --test hotkey_stop)
    (cd desktop/src-tauri && cargo test --features custom-protocol --test matrix_row1_tap_permission_denied)
    bash scripts/qa/hotkey-walk.sh --self-test
    shellcheck scripts/qa/hotkey-walk.sh
  `,
})

ITEMS.push({
  id: 'yap-p0-perm-repair', prompt: 'yap-p0-perm-repair', branch: 'loop/yap-p0-perm-repair-truthful-probe-one-click', gated: null,
  title: 'One truthful grant probe per service (live API, never the Settings list) in perm_probe.rs, a detected "stale identity" state, and a one-click Repair: tccutil reset for Yap\'s own bundle id, open the exact pane, re-probe until granted',
  preflight: `
    ( M=0; test -f desktop/src-tauri/tests/hotkey_no_tcc.rs || M=1; grep -q 'DICTATE_TOGGLE.shortcut()' desktop/src-tauri/src/lib.rs || M=1; grep -q 'HotkeySource::Carbon' desktop/src-tauri/src/lib.rs || M=1; if [ "$M" = 1 ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-p0-hotkeys-no-permission has not landed (no DICTATE_TOGGLE registration / HotkeySource::Carbon in lib.rs) — run the review pass to land it, then the build pass again"; exit 0; fi; test -f desktop/src-tauri/src/perm_probe.rs && grep -q 'StaleIdentity' desktop/src-tauri/src/perm_probe.rs && grep -q 'pub fn repair_argv' desktop/src-tauri/src/perm_probe.rs && grep -q 'repair-plan' scripts/qa/perm-probe.sh )
  `,
  spec: `
    Panel: not required — FIX (Wilson 2026-10-17: "permissioning ... nothing's working")
    DEPENDS: yap-p0-hotkeys-no-permission ON MAIN (the preflight skips this item with "dependency not on
      main" until it is). Blocks: yap-p0-insertion-bridge-fallback (its preflight greps perm_probe.rs
      pub fn probe_all and the StaleIdentity state).

    REQUIRE (before any code — NO GUESSING)
      - Run on unmodified main: bash scripts/qa/perm-probe.sh (read-only) and paste authority_leaf, cdhash,
        designated_requirement, helper_* lines. Paste a cargo test that FAILS on main showing that with no
        ledger entry (Wilson's real state: #211 shipped after 0.9.0) and a denied probe, stale_grants() is
        empty — the stale state is invisible today.
      - Paste all of it into docs/loop/evidence/yap-p0-perm-repair.md too (EVIDENCE FILE above).
      - Confirm the tccutil service names from \`tccutil\` usage / man page on this macOS (Accessibility,
        ListenEvent, PostEvent, Microphone) and paste them. Never run tccutil yourself (HAZARD).

    DO (names are contract)
      1. NEW module desktop/src-tauri/src/perm_probe.rs (declared in lib.rs). One function per service,
         each a LIVE API call: Accessibility = AXIsProcessTrusted; ListenEvent = CGPreflightListenEventAccess
         AND IOHIDCheckAccess (both recorded, disagreement reported); PostEvent = CGPreflightPostEventAccess;
         Microphone = AVCaptureDevice authorizationStatus. pub fn probe_all() -> ProbeReport. Nothing reads
         TCC.db or the Settings list. The ONE extern declaration of AXIsProcessTrusted lives here;
         permissions.rs report(), the banner, onboarding, the hotkey item's degraded line and (item 3)
         paste.rs all read perm_probe — permissions.rs keeps no grant FFI of its own.
      2. ProbeState per service: Granted | Denied | NotDetermined | StaleIdentity | Unknown. StaleIdentity
         = probe denied AND (perm_truth ledger says it was granted under a different signature OR the
         last-run signature, persisted at EVERY launch, differs from the running one in authority or team).
         pub fn classify(..) is pure. Persist last_run_signature every launch so the next identity change is
         caught even when no grant ever read true.
      3. Repair: a Tauri command repair_permission(service) (in the invoke handler list) reachable from the
         banner, onboarding and the degraded pill. pub fn repair_argv(service, bundle_id) -> Vec<String>
         is pure and returns exactly ["/usr/bin/tccutil", "reset", <Service>, <bundle id>], the bundle id
         read from NSBundle.mainBundle at runtime; never "All", never another bundle id. Repairing
         Accessibility ALSO resets PostEvent (the ⌘V insertion bridge depends on it). Then
         AXIsProcessTrustedWithOptions(prompt) for Accessibility / IOHIDRequestAccess for ListenEvent, open
         the exact pane through the existing open_privacy_pane allow-list
         (x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility / ?Privacy_ListenEvent
         / ?Privacy_Microphone), and re-probe every 1 s for 120 s: "Waiting for you to switch Yap on…", then
         "Fixed" the moment probe_all flips — for Accessibility, PostEvent and Microphone. Input Monitoring
         usually only takes effect after a quit and reopen, so for ListenEvent the flow says "Granted —
         quit and reopen Yap to finish" with a Relaunch button (app.restart()); never claim it works
         without a relaunch. tccutil non-zero -> show its stderr + the manual steps.
      4. scripts/qa/perm-probe.sh gains \`--repair-plan\`: prints repair_cmd_<service>= (the exact argv the
         app would run, space-joined) and repair_pane_<service>= lines for the probed app's bundle id,
         executes nothing; --self-test covers it (Calculator's id).
      5. tests/perm_repair.rs, these exact names, real assertions (>= 16 assert! / assert_eq! in total):
         probe_matrix_maps_every_combination (each service Granted/Denied x ledger none/same/different x
         last-run same/different), stale_identity_without_ledger_after_signature_change (the 0.9.0 case;
         prints "perm_repair: ledger=none last_run=changed probe=denied state=StaleIdentity"),
         stale_identity_from_ledger_different_signature, repair_argv_is_own_bundle_id_never_all,
         repair_pane_url_per_service, reprobe_stops_on_granted, reprobe_times_out_at_120s,
         banner_and_onboarding_share_one_probe, accessibility_repair_also_resets_post_event,
         listen_event_repair_says_relaunch (>= 20 asserts in total with these).
         desktop/src/permission.repair.test.ts (>= 3 vitest cases):
         StaleIdentity copy, Repair button for Denied and StaleIdentity calls invoke("repair_permission"),
         "Fixed" after the probe flips.

    WILSON'S WALK (PR body): open Yap; the banner says "macOS lists Yap as allowed, but that permission
    belongs to an older Yap" for Accessibility and Input Monitoring; click Repair, flip Yap on in the
    pane that opens; Accessibility turns to Fixed; Input Monitoring asks for one Relaunch, after which
    fn works.
  `,
  acceptance: `
    test -f desktop/src-tauri/src/perm_probe.rs
    test -f desktop/src-tauri/tests/perm_repair.rs
    test -f desktop/src/permission.repair.test.ts
    grep -q 'mod perm_probe' desktop/src-tauri/src/lib.rs
    grep -q 'StaleIdentity' desktop/src-tauri/src/perm_probe.rs
    grep -q 'pub fn probe_all' desktop/src-tauri/src/perm_probe.rs
    grep -q 'pub fn repair_argv' desktop/src-tauri/src/perm_probe.rs
    grep -rq 'CGPreflightListenEventAccess' desktop/src-tauri/src
    grep -rq 'CGPreflightPostEventAccess' desktop/src-tauri/src
    grep -rq '/usr/bin/tccutil' desktop/src-tauri/src
    test "$(grep -rl 'fn AXIsProcessTrusted()' desktop/src-tauri/src | wc -l | tr -d ' ')" -eq 1
    grep -q 'fn AXIsProcessTrusted()' desktop/src-tauri/src/perm_probe.rs
    grep -q 'perm_probe::probe_all' desktop/src-tauri/src/permissions.rs
    grep -q 'repair_permission' desktop/src-tauri/src/lib.rs
    grep -rq 'invoke("repair_permission"' desktop/src
    test "$(grep -cE 'assert(_eq|_ne)?!' desktop/src-tauri/tests/perm_repair.rs)" -ge 20
    grep -rq 'quit and reopen Yap to finish' desktop/src
    test -s docs/loop/evidence/yap-p0-perm-repair.md
    grep -q '^ROOT-CAUSE-PROBE yap-p0-perm-repair$' docs/loop/evidence/yap-p0-perm-repair.md
    grep -q 'authority_leaf=' docs/loop/evidence/yap-p0-perm-repair.md
    grep -q 'designated_requirement=' docs/loop/evidence/yap-p0-perm-repair.md
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    mkdir -p desktop/target
    (cd desktop && npx vitest run src/permission.repair.test.ts)
    (cd desktop/src-tauri && cargo test --features custom-protocol --test perm_repair -- --list) > desktop/target/p0-pr-list.txt 2>/dev/null
    grep -q '^probe_matrix_maps_every_combination: test$' desktop/target/p0-pr-list.txt
    grep -q '^stale_identity_without_ledger_after_signature_change: test$' desktop/target/p0-pr-list.txt
    grep -q '^stale_identity_from_ledger_different_signature: test$' desktop/target/p0-pr-list.txt
    grep -q '^repair_argv_is_own_bundle_id_never_all: test$' desktop/target/p0-pr-list.txt
    grep -q '^repair_pane_url_per_service: test$' desktop/target/p0-pr-list.txt
    grep -q '^reprobe_stops_on_granted: test$' desktop/target/p0-pr-list.txt
    grep -q '^reprobe_times_out_at_120s: test$' desktop/target/p0-pr-list.txt
    grep -q '^banner_and_onboarding_share_one_probe: test$' desktop/target/p0-pr-list.txt
    grep -q '^accessibility_repair_also_resets_post_event: test$' desktop/target/p0-pr-list.txt
    grep -q '^listen_event_repair_says_relaunch: test$' desktop/target/p0-pr-list.txt
    (cd desktop/src-tauri && cargo test --features custom-protocol --test perm_repair -- --exact stale_identity_without_ledger_after_signature_change --nocapture) > desktop/target/p0-pr-crux.txt 2>&1
    grep -q '^perm_repair: ledger=none last_run=changed probe=denied state=StaleIdentity$' desktop/target/p0-pr-crux.txt
    grep -q 'test result: ok. 1 passed' desktop/target/p0-pr-crux.txt
    (cd desktop/src-tauri && cargo test --features custom-protocol --test perm_repair)
    (cd desktop/src-tauri && cargo test --features custom-protocol --test perm_truth)
    bash scripts/qa/perm-probe.sh --self-test
    bash scripts/qa/perm-probe.sh --app /System/Applications/Calculator.app --repair-plan > desktop/target/qa-repair-plan.txt
    grep -q '^repair_cmd_accessibility=/usr/bin/tccutil reset Accessibility com.apple.calculator$' desktop/target/qa-repair-plan.txt
    grep -q '^repair_cmd_listenevent=/usr/bin/tccutil reset ListenEvent com.apple.calculator$' desktop/target/qa-repair-plan.txt
    grep -q '^repair_cmd_postevent=/usr/bin/tccutil reset PostEvent com.apple.calculator$' desktop/target/qa-repair-plan.txt
    grep -q '^repair_pane_accessibility=x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility$' desktop/target/qa-repair-plan.txt
    ! grep -q ' All ' desktop/target/qa-repair-plan.txt
    shellcheck scripts/qa/perm-probe.sh
  `,
})

ITEMS.push({
  id: 'yap-p0-insertion-bridge-fallback', prompt: 'yap-p0-insertion-bridge-fallback', branch: 'loop/yap-p0-insertion-fallback-copy-card', gated: null,
  title: 'Insertion never silently fails: AX insertion only when the probe says trusted, a clipboard + ⌘V bridge when PostEvent is granted, an instant Copy card when neither is, and one failing backend call can no longer blank the app as "Bridge error"',
  preflight: `
    ( M=0; test -f desktop/src-tauri/src/perm_probe.rs || M=1; grep -q 'pub fn probe_all' desktop/src-tauri/src/perm_probe.rs || M=1; grep -q 'StaleIdentity' desktop/src-tauri/src/perm_probe.rs || M=1; if [ "$M" = 1 ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-p0-perm-repair has not landed (no perm_probe.rs probe_all / StaleIdentity) — run the review pass to land it, then the build pass again"; exit 0; fi; test -f desktop/src-tauri/tests/insertion_matrix.rs && grep -q 'pub fn choose_insert_path' desktop/src-tauri/src/paste.rs && grep -q 'allSettled' desktop/src/appShell.ts )
  `,
  spec: `
    Panel: not required — FIX (Wilson 2026-10-17: "the bridges ... nothing's working")
    DEPENDS: yap-p0-perm-repair ON MAIN (preflight skips until perm_probe.rs is there; uses its single
      probe, never a second grant check). Blocks: yap-p0-release-smoke.

    REQUIRE (before any code — NO GUESSING)
      - Establish which "bridge" is broken, with output, not opinion: (a) read-only grep of yap.log for
        pasted=false / "paste not confirmed" / "Enable Accessibility" since 2026-10-16 and paste the counts;
        (b) a vitest on unmodified main that makes ONE of the boot invokes in appShell.ts reject and shows
        the whole app replaced by "Bridge error". Both go into the PR body and into
        docs/loop/evidence/yap-p0-insertion-bridge-fallback.md (EVIDENCE FILE above), with the
        post_event_access= line from the helper below.
      - Prove with a probe whether CGEventPost(⌘V) is delivered without Accessibility on this macOS: a tiny
        ad hoc Swift helper (scripts/qa/post-probe.swift, built like fn-post.swift) prints
        post_event_access=<CGPreflightPostEventAccess()> and, only with --post, posts ⌘V into a TextEdit
        document IT created. Do not assume "keyDown needs no AX" — fn-post.swift:32 shows PostEvent is gated.

    DO (names are contract)
      1. paste.rs: pub enum InsertPath { Ax, PasteBridge, CopyCard } and pub fn choose_insert_path(probe:
         &perm_probe::ProbeReport) -> InsertPath, pure. Ax when Accessibility is Granted (AX kAXSelectedText
         on the focused element, falling back to PasteBridge when the element refuses); PasteBridge when
         PostEvent is Granted (pasteboard + CGEventPost ⌘V, receipt-checked by paste_tx as today); CopyCard
         otherwise (no keystroke attempted). copy_and_maybe_paste AND delete_selection call
         choose_insert_path(&perm_probe::probe_all()); every permissions::is_accessibility_trusted() call
         in paste.rs is removed (paste.rs:184, :246, :306 today).
      2. CopyCard: paste.rs emits the Tauri event "insert-copy-card" with the transcript; the pill listens
         for it and shows the text within 100 ms with a Copy button and "Copied — press ⌘V. Repair to
         auto-insert." (Repair = invoke("repair_permission")). A PasteBridge with no receipt
         (paste_tx RECEIPT_WINDOW) also emits insert-copy-card — never a silent pasted=false.
      3. Backend-bridge boot: appShell.ts loads with Promise.allSettled; each failed command shows its own
         inline error + retry in the view that needs it; the status line is never replaced by "Bridge
         error" while any command succeeded. App.tsx's full-screen "UI failed to load backend bridge" only
         when get_settings itself fails.
      4. tests/insertion_matrix.rs, exact names, real assertions (>= 12 in total): ax_trusted_uses_ax,
         ax_dead_post_event_granted_uses_paste_bridge, none_granted_shows_copy_card_and_posts_nothing (a
         counting fake poster proves zero CGEventPost; prints "insertion_matrix: ax=denied post=denied
         path=CopyCard posts=0"), paste_bridge_no_receipt_falls_to_copy_card, wrong_target_copies_only,
         stale_take_copies_only. desktop/src/appShell.boot.test.ts (>= 2 vitest cases): one rejected invoke
         leaves the rest rendered and no "Bridge error" status; get_settings rejection shows the full-screen
         retry.

    WILSON'S WALK (PR body): with Accessibility removed, dictate into TextEdit: the Copy card shows the
    text at once and ⌘V pastes it; after Repair, the same take inserts by itself.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/insertion_matrix.rs
    test -f desktop/src/appShell.boot.test.ts
    test -f scripts/qa/post-probe.swift
    grep -q 'pub enum InsertPath' desktop/src-tauri/src/paste.rs
    test "$(grep -c 'choose_insert_path(' desktop/src-tauri/src/paste.rs)" -ge 3
    grep -q 'perm_probe::probe_all()' desktop/src-tauri/src/paste.rs
    ! grep -q 'is_accessibility_trusted()' desktop/src-tauri/src/paste.rs
    grep -q 'insert-copy-card' desktop/src-tauri/src/paste.rs
    grep -rq 'insert-copy-card' desktop/src
    grep -q 'allSettled' desktop/src/appShell.ts
    test "$(grep -cE 'assert(_eq|_ne)?!' desktop/src-tauri/tests/insertion_matrix.rs)" -ge 12
    test -s docs/loop/evidence/yap-p0-insertion-bridge-fallback.md
    grep -q '^ROOT-CAUSE-PROBE yap-p0-insertion-bridge-fallback$' docs/loop/evidence/yap-p0-insertion-bridge-fallback.md
    grep -q 'post_event_access=' docs/loop/evidence/yap-p0-insertion-bridge-fallback.md
    grep -q 'Bridge error' docs/loop/evidence/yap-p0-insertion-bridge-fallback.md
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    mkdir -p desktop/target
    (cd desktop && npx vitest run src/appShell.boot.test.ts)
    (cd desktop/src-tauri && cargo test --features custom-protocol --test insertion_matrix -- --list) > desktop/target/p0-ins-list.txt 2>/dev/null
    grep -q '^ax_trusted_uses_ax: test$' desktop/target/p0-ins-list.txt
    grep -q '^ax_dead_post_event_granted_uses_paste_bridge: test$' desktop/target/p0-ins-list.txt
    grep -q '^none_granted_shows_copy_card_and_posts_nothing: test$' desktop/target/p0-ins-list.txt
    grep -q '^paste_bridge_no_receipt_falls_to_copy_card: test$' desktop/target/p0-ins-list.txt
    grep -q '^wrong_target_copies_only: test$' desktop/target/p0-ins-list.txt
    grep -q '^stale_take_copies_only: test$' desktop/target/p0-ins-list.txt
    (cd desktop/src-tauri && cargo test --features custom-protocol --test insertion_matrix -- --exact none_granted_shows_copy_card_and_posts_nothing --nocapture) > desktop/target/p0-ins-crux.txt 2>&1
    grep -q '^insertion_matrix: ax=denied post=denied path=CopyCard posts=0$' desktop/target/p0-ins-crux.txt
    grep -q 'test result: ok. 1 passed' desktop/target/p0-ins-crux.txt
    (cd desktop/src-tauri && cargo test --features custom-protocol --test insertion_matrix)
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib paste)
    bash scripts/qa/hotkey-walk.sh --self-test
  `,
})

ITEMS.push({
  id: 'yap-p0-release-smoke', prompt: 'yap-p0-release-smoke', branch: 'loop/yap-p0-release-smoke-signed-app-gate', gated: null,
  title: 'Every DMG build runs a signed-app smoke after signing (codesign --strict on app + DMG with the pinned Developer ID team, a designated-requirement change gate, launch, probe every service, register hotkeys, insert into TextEdit) and the release FAILS when any step fails, including a stale-identity grant',
  preflight: `
    ( M=0; grep -q 'DICTATE_TOGGLE.shortcut()' desktop/src-tauri/src/lib.rs || M=1; grep -q 'HotkeySource::Carbon' desktop/src-tauri/src/lib.rs || M=1; grep -q 'pub fn probe_all' desktop/src-tauri/src/perm_probe.rs || M=1; grep -q 'StaleIdentity' desktop/src-tauri/src/perm_probe.rs || M=1; grep -q 'pub fn choose_insert_path' desktop/src-tauri/src/paste.rs || M=1; grep -q 'insert-copy-card' desktop/src-tauri/src/paste.rs || M=1; if [ "$M" = 1 ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-p0-hotkeys-no-permission, yap-p0-perm-repair and yap-p0-insertion-bridge-fallback must all be on main (DICTATE_TOGGLE / perm_probe::probe_all / choose_insert_path) — run the review pass, then the build pass again"; exit 0; fi; test -x scripts/qa/release-smoke.sh && grep -q 'release-smoke.sh' scripts/sign-local.sh && test -f scripts/qa/fixtures/release-smoke-negative-2026-10-17.log )
  `,
  spec: `
    Panel: not required — FIX (0.9.0 shipped with every TCC grant dead and nothing noticed)
    DEPENDS: yap-p0-hotkeys-no-permission, yap-p0-perm-repair, yap-p0-insertion-bridge-fallback, ALL ON
      MAIN (the preflight skips this item with "dependency not on main" until they are).

    REQUIRE (before any code — NO GUESSING)
      - Read scripts/sign-local.sh --tauri-build and docs/RELEASING.md and paste the exact step where the
        .app/.dmg is produced; the smoke hooks in right after it. Paste perm-probe.sh --compare output for
        the built app vs /Applications/Yap.app (dr_change=yes|no) — the 0.9.0 signal. Both go into
        docs/loop/evidence/yap-p0-release-smoke.md (EVIDENCE FILE above).

    DO (names are contract)
      1. The app's --release-smoke flag (distinct from --smoke, which refuses hotkeys and paste) logs ONE
         line per step through pure formatter functions in desktop/src-tauri/src/smoke.rs, exact formats:
           release-smoke step=probe service=<accessibility|listenevent|postevent|microphone> state=<ProbeState> ledger=<granted|none>
           release-smoke step=hotkeys carbon=<registered|failed> fn_tap=<status>
           release-smoke step=insert path=<Ax|PasteBridge|CopyCard> ok=<true|false>
           release-smoke step=quit ok=true
         It registers hotkeys, allows ONE insertion into the TextEdit document the smoke created (frontmost
         checked first; a unit test proves it never inserts when frontmost is not TextEdit), then quits.
      2. SIGNATURE + IDENTITY STEPS (second-pass reviewer B2), run FIRST and only after signing, launching
         nothing: step=signature runs codesign --verify --deep --strict on the .app AND on the .app inside
         the DMG (hdiutil attach -readonly -nobrowse to a temp mountpoint, detach after), and requires
         Authority "Developer ID Application: ..." with TeamIdentifier=VHYV8C2JNU (pinned) on both.
         step=dr_change runs perm-probe.sh --compare <installed or last-release app> <new app>; dr_change=yes
         FAILS unless BOTH --allow-dr-change and --release-note "<line>" are passed, and the note is printed
         as release_note=<line> for the release notes. --signature-only / --dr-only run just these steps
         (safe on any machine; read-only).
      3. scripts/qa/release-smoke.sh --app PATH | --parse LOG | --self-test [--force-fail STEP]. --app
         launches (open -n, temp data dir, --release-smoke; exit 2 if another Yap with the same bundle id
         is running, never signal it), drives a headless-AppleScript TextEdit document, reads the app's
         yap.log and hands it to the SAME parser --parse uses. It prints step=<name> ok|fail per step.
         THE MATRIX the parser enforces (per service, then per run):
           probe Granted                                  -> ok
           probe Denied/StaleIdentity AND ledger=granted  -> FAIL step=probe (stale identity: a grant the
                                                             ledger saw working reads denied; 0.9.0)
           probe StaleIdentity, any ledger                -> FAIL step=probe
           probe Denied/NotDetermined AND ledger=none     -> ok (genuinely absent: a fresh host)
           EVERY grant dead AND ledger=granted for any    -> FAIL step=probe (the 0.9.0 shape exactly)
           carbon=failed                                  -> FAIL step=hotkeys, always
           accessibility or postevent Granted AND path=CopyCard -> FAIL step=insert (a grant is live, so
                                                             the Copy card is a regression, not a fallback)
           both accessibility and postevent absent (ledger=none) AND path=CopyCard ok=true -> ok
           path=Ax|PasteBridge AND ok=false, or the document text != the smoke string -> FAIL step=insert
           no step=quit ok=true line within 10 s          -> FAIL step=quit
         Exit 0 all ok; 1 any FAIL (names the step); 2 usage / another Yap running. --force-fail STEP makes
         that step fail and must exit 1.
      4. FIXTURES. scripts/qa/fixtures/release-smoke-negative-2026-10-17.log: a REAL captured log — the
         verbatim yap.log excerpt 2026-10-17T07:40:34Z..07:45:48Z from Wilson's machine (read-only copy,
         transcript text redacted, header "# captured from yap.log 2026-10-17 Yap 0.9.0"), with the
         release-smoke lines the 0.9.0 state implies appended under "# expected-under-new-build" (ledger=
         granted, state=StaleIdentity). --parse on it must exit 1 at step=probe. Matrix fixtures in
         scripts/qa/fixtures/release-smoke-matrix/<row>.log are GOLDEN files written by
         tests/release_smoke_lines.rs from the real smoke.rs formatters (golden_fixtures_match_formatter
         regenerates and compares, so the parser is proven against lines the app actually emits), one
         per matrix row, named: granted-all-ax.log (exit 0), absent-grants-copy-card.log (exit 0),
         all-dead-ledger-granted.log (exit 1),
         stale-ledger-granted-probe-denied.log (exit 1), granted-but-copy-card.log (exit 1),
         carbon-failed.log (exit 1), no-quit.log (exit 1). --self-test runs all of them plus every refusal
         and force-fail path and launches nothing.
      5. scripts/sign-local.sh --tauri-build runs release-smoke.sh (signature, dr_change, then --app) AFTER
         its last codesign/notarize step, on the built app and DMG, and exits non-zero
         if it fails (bare exit propagation, no pipe in the exit path). docs/RELEASING.md: the smoke is
         mandatory; dr_change=yes goes into the release notes with the Repair instructions.
      6. This seat never runs --app on Wilson's machine (HAZARD); only --parse / --self-test /
         --signature-only / --dr-only. The full run
         is the release host's step, listed in the PR body.
  `,
  acceptance: `
    test -x scripts/qa/release-smoke.sh
    grep -q 'release-smoke.sh' scripts/sign-local.sh
    grep -q 'release-smoke.sh' docs/RELEASING.md
    grep -q 'release-smoke step=probe' desktop/src-tauri/src/smoke.rs
    grep -q 'release-smoke step=hotkeys' desktop/src-tauri/src/smoke.rs
    grep -q 'release-smoke step=insert' desktop/src-tauri/src/smoke.rs
    test -f scripts/qa/fixtures/release-smoke-negative-2026-10-17.log
    grep -q '^# captured from yap.log 2026-10-17' scripts/qa/fixtures/release-smoke-negative-2026-10-17.log
    grep -q 'wilson_voice_lib\\] Accessibility not trusted' scripts/qa/fixtures/release-smoke-negative-2026-10-17.log
    grep -q 'pasted=false' scripts/qa/fixtures/release-smoke-negative-2026-10-17.log
    test -f desktop/src-tauri/tests/release_smoke_lines.rs
    bash scripts/qa/release-smoke.sh --self-test
    bash scripts/qa/release-smoke.sh --parse scripts/qa/fixtures/release-smoke-matrix/granted-all-ax.log
    bash scripts/qa/release-smoke.sh --parse scripts/qa/fixtures/release-smoke-matrix/absent-grants-copy-card.log
    ! bash scripts/qa/release-smoke.sh --parse scripts/qa/fixtures/release-smoke-negative-2026-10-17.log
    ! bash scripts/qa/release-smoke.sh --parse scripts/qa/fixtures/release-smoke-matrix/stale-ledger-granted-probe-denied.log
    ! bash scripts/qa/release-smoke.sh --parse scripts/qa/fixtures/release-smoke-matrix/granted-but-copy-card.log
    ! bash scripts/qa/release-smoke.sh --parse scripts/qa/fixtures/release-smoke-matrix/carbon-failed.log
    ! bash scripts/qa/release-smoke.sh --parse scripts/qa/fixtures/release-smoke-matrix/no-quit.log
    ! bash scripts/qa/release-smoke.sh --parse scripts/qa/fixtures/release-smoke-matrix/all-dead-ledger-granted.log
    grep -q 'codesign --verify --deep --strict' scripts/qa/release-smoke.sh
    grep -q 'VHYV8C2JNU' scripts/qa/release-smoke.sh
    grep -q 'hdiutil attach' scripts/qa/release-smoke.sh
    awk '/codesign/ {c=NR} /release-smoke.sh/ {r=NR} END {exit !(c > 0 && r > c)}' scripts/sign-local.sh
    test -s docs/loop/evidence/yap-p0-release-smoke.md
    grep -q '^ROOT-CAUSE-PROBE yap-p0-release-smoke$' docs/loop/evidence/yap-p0-release-smoke.md
    grep -q 'dr_change=' docs/loop/evidence/yap-p0-release-smoke.md
    mkdir -p desktop/target
    bash scripts/qa/release-smoke.sh --parse scripts/qa/fixtures/release-smoke-negative-2026-10-17.log > desktop/target/p0-smoke-neg.txt 2>&1; test $? -eq 1
    grep -q '^step=probe fail' desktop/target/p0-smoke-neg.txt
    bash scripts/qa/release-smoke.sh --self-test --force-fail probe; test $? -eq 1
    bash scripts/qa/release-smoke.sh --self-test --force-fail hotkeys; test $? -eq 1
    bash scripts/qa/release-smoke.sh --self-test --force-fail insert; test $? -eq 1
    bash scripts/qa/release-smoke.sh --self-test --force-fail signature; test $? -eq 1
    bash scripts/qa/release-smoke.sh --self-test --force-fail dr_change; test $? -eq 1
    bash scripts/qa/release-smoke.sh --signature-only --app /System/Applications/Calculator.app > desktop/target/p0-smoke-sig.txt 2>&1; test $? -eq 1
    grep -q '^step=signature fail' desktop/target/p0-smoke-sig.txt
    bash scripts/qa/release-smoke.sh --dr-only --app /System/Applications/Calculator.app --compare /System/Applications/TextEdit.app; test $? -eq 1
    bash scripts/qa/release-smoke.sh --dr-only --app /System/Applications/Calculator.app --compare /System/Applications/TextEdit.app --allow-dr-change; test $? -eq 1
    bash scripts/qa/release-smoke.sh --dr-only --app /System/Applications/Calculator.app --compare /System/Applications/TextEdit.app --allow-dr-change --release-note 'loop acceptance override check'
    shellcheck scripts/qa/release-smoke.sh
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    (cd desktop/src-tauri && cargo test --features custom-protocol --test release_smoke_lines)
    git diff --exit-code -- scripts/qa/fixtures/release-smoke-matrix
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib smoke)
  `,
})

// ── 07b-yap-walk-defects-2026-10-19.mjs ───────────────────────────────────
// yap-walk-defects-2026-10-19 — the defects the live walk of main 3569ce4 + installed 0.9.1 found
// (Projects/Loop-Logs/yap-live-walk-2026-10-19.md, seat yap-live-walk, 03:14Z-03:22Z). Sorted after
// 07-yap-p0-broken so this lane runs after the P0 lane. D1 (Carbon chord deadlock), D4 (capture tail)
// and D6 (RSS) are fixed or answered in PR fix/chord-deadlock-and-take-length, not here.
//
// HAZARD (same as 07): no seat launches the GUI app, posts events into, signals or quits Wilson's
// running /Applications/Yap.app, or writes TCC on this machine. Every automated proof is a cargo test,
// a vitest, or a script self-test. SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.
// NO GUESSING: the root-cause probe goes into the PR body under "Root cause, proven" AND into
// docs/loop/evidence/<item id>.md, first line exactly "ROOT-CAUSE-PROBE <item id>".

ITEMS.push({
  id: 'yap-qa-hotkey-walk-binding', prompt: 'yap-qa-hotkey-walk-binding', branch: 'loop/yap-qa-hotkey-walk-configured-chord', gated: null,
  title: 'hotkey-walk presses the CONFIGURED push-to-talk chord (fn, fn⌃, …) read from settings.json, has a dev-build flag, and the app shows a hint when fn is held without the binding\'s modifier',
  preflight: `
    grep -q 'pttBinding' scripts/qa/hotkey-walk.sh && grep -q -- '--dev-build' scripts/qa/hotkey-walk.sh && grep -q 'FN_WITHOUT_MODIFIER_HINT' desktop/src-tauri/src/ptt_macos.rs
  `,
  spec: `
    Panel: not required — FIX (walk D2, P1).
    Root cause (walk): scripts/qa/fn-post.swift "down" sets only maskSecondaryFn and HOLD_STEPS in
    scripts/qa/hotkey-walk.sh assumes binding "fn"; Wilson's binding (and the default) is
    pttBinding: "fn_control", so the walk's fn-only post never matches and the walk FAILS on a
    working app. Product side: the UI says "Hold fn⌃"; a user holding fn alone gets nothing and no
    reason.
    DO 1 — hotkey-walk.sh reads pttBinding from Yap's settings.json (same data dir the app uses;
    --settings <path> override for tests) and posts the matching chord through fn-post.swift (ctrl
    down, then fn down; release in reverse). Unknown binding -> refuse with a named error, post
    nothing. --self-test covers fn, fn_control and an unknown value against fixture settings files
    and prints one line per case "binding=<value> chord=<keys>".
    DO 2 — --dev-build flag for an unbundled dev binary (matches the process by its target path
    instead of the bundle), replacing the YAP_QA_FAKE_PIDS workaround for live dev walks
    (YAP_QA_FAKE_PIDS stays self-test-only).
    DO 3 — ptt_macos: when fn goes down and up (or is held > 400 ms) WITHOUT the binding's modifier,
    emit one hint event (constant FN_WITHOUT_MODIFIER_HINT, e.g. "Hold fn⌃ — fn alone does nothing
    with this binding"), throttled to once per 30 s, shown on the pill. Pure decision function
    fn_hint_due(binding, fn_down, ctrl_down, held_ms, last_hint_ms, now_ms) -> bool with unit tests.
  `,
  acceptance: `
    bash scripts/qa/hotkey-walk.sh --self-test > desktop/target/qa-walk-binding.txt 2>&1
    grep -q '^binding=fn_control chord=ctrl+fn$' desktop/target/qa-walk-binding.txt
    grep -q '^binding=fn chord=fn$' desktop/target/qa-walk-binding.txt
    grep -q 'unknown binding' desktop/target/qa-walk-binding.txt
    shellcheck scripts/qa/hotkey-walk.sh
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib fn_hint_due) > desktop/target/qa-fn-hint.txt 2>&1
    grep -Eq 'test result: ok. [1-9][0-9]* passed' desktop/target/qa-fn-hint.txt
    test -s docs/loop/evidence/yap-qa-hotkey-walk-binding.md
    head -1 docs/loop/evidence/yap-qa-hotkey-walk-binding.md | grep -qx 'ROOT-CAUSE-PROBE yap-qa-hotkey-walk-binding'
  `,
})

ITEMS.push({
  id: 'yap-lic-00-kill-forge-fetch', prompt: 'yap-lic-00-kill-forge-fetch', branch: 'loop/yap-lic-00-kill-forge-revocation-fetch', gated: null,
  title: 'SECURITY: delete the launch-time revocation GET to the dead Forge sslip.io host entirely; no licensing network call at all until the new client lands',
  preflight: `
    ! git grep -qnE 'sslip\\.io|forge\\.87|87-99-149-214' -- desktop/src-tauri/src desktop/src site docs && ! git grep -qnE 'REVOCATION_URL|ISSUER_HOST|fetch_revocations|spawn_revocation_refresh' -- desktop/src-tauri/src && grep -q 'launch_license_init_makes_no_network_call' desktop/src-tauri/src/license.rs && ! grep -qiE 'revocation (check|list|refresh|download)|refunded or charged back' PRIVACY.md
  `,
  spec: `
    Panel: not required — FIX, class SECURITY (walk D3; supersedes the earlier yap-license-revocation-dead-host
    draft). Spec source: Projects/Yap/licensing-migration-plan-2026-10-19.md "## Items (draft, not yet filed)"
    -> yap-lic-00.
    Root cause (walk yap.log 2026-10-19T03:19:57Z): "license: revocation refresh skipped (error sending request
    for url (https://forge.87-99-149-214.sslip.io/v1/yap/revoked.json))". Every launch (and every activation)
    sends a GET to the decommissioned Forge box: license.rs ISSUER_HOST / REVOCATION_URL (:151-156) and the
    fetch at :1467-1479; callers lib.rs spawn_revocation_refresh (:5672 in activate_license, :5724-5736 the fn,
    :6891-6895 at launch). A dead sslip.io name that anybody can later answer for is a licensing input the app
    trusts — security class, not cosmetics.
    DO — remove REVOCATION_URL, ISSUER_HOST, fetch_revocations and spawn_revocation_refresh and both call
    sites. NO replacement endpoint: licensing moves to the consulting app's existing /api/v1/licensing (sibling repo) in later
    items yap-lic-01..06 — do NOT implement any of those here. Keep READING any cached revoked_kids, so an
    install that has a cached list behaves exactly as before. Local licence verification (Ed25519 claims,
    trial, grace) is unchanged. Remove the host from every config and doc mention, and correct
    docs/ARCHITECTURE-AUDIT-2026-09-26.md §9 ("harmless no-op" was wrong: it was a live GET every launch).
    PRIVACY.md (repo root, outside the grep scope above, so it is named here): delete list item 3 "The
    revocation check" from "The only times Yap uses the network" — the list becomes the model download and
    the update check, TWO calls — and say plainly that the licensing path makes no network call: a licence
    is verified on the Mac against the compiled-in public key. The acceptance refuses PRIVACY.md text
    matching revocation (check|list|refresh|download) or "refunded or charged back", so do not phrase the
    removal as "no longer downloads a revocation list". PRIV-A (30-y6) pins the same two call sites.
    Legitimate unrelated "forge" words stay: forget/forged/forgeable, and the issuer-repo citations
    "drivia-forge" and the signing-key path.
    Tests: (1) launch_license_init_makes_no_network_call — the launch-time licence init takes an injected
    transport/client; a recording fake asserts zero requests across init + status + a cached revoked_kids load.
    (2) license_rs_has_no_http_client — reads license.rs via include_str! and fails on "sslip.io", "reqwest",
    "ureq" or "http://"/"https://" outside the PAYMENT_LINK_URL constant. (3) existing licence tests green.
    Can ship alone in the next release.
  `,
  acceptance: `
    ! git grep -nE 'sslip\\.io|forge\\.87|87-99-149-214' -- desktop/src-tauri/src desktop/src site docs
    ! git grep -nE 'REVOCATION_URL|ISSUER_HOST|fetch_revocations|spawn_revocation_refresh' -- desktop/src-tauri/src desktop/src
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib license) > desktop/target/qa-lic00.txt 2>&1
    grep -q 'launch_license_init_makes_no_network_call ... ok' desktop/target/qa-lic00.txt
    grep -q 'license_rs_has_no_http_client ... ok' desktop/target/qa-lic00.txt
    grep -Eq 'test result: ok. [1-9][0-9]* passed' desktop/target/qa-lic00.txt
    (cd desktop/src-tauri && cargo test --features custom-protocol --test license_gate)
    ! grep -niE 'revocation (check|list|refresh|download)|refunded or charged back' PRIVACY.md
    test -s docs/loop/evidence/yap-lic-00-kill-forge-fetch.md
    head -1 docs/loop/evidence/yap-lic-00-kill-forge-fetch.md | grep -qx 'ROOT-CAUSE-PROBE yap-lic-00-kill-forge-fetch'
    (cd desktop && npm run loop:validate)
  `,
})

ITEMS.push({
  id: 'yap-dev-perm-probe-attribution', prompt: 'yap-dev-perm-probe-attribution', branch: 'loop/yap-dev-perm-probe-host-terminal', gated: null,
  title: 'A dev build\'s permission probe says "dev build: permissions are the host terminal\'s" instead of showing the launching terminal\'s TCC grants as Yap\'s green rows',
  preflight: `
    grep -q 'pub fn running_as_bundle' desktop/src-tauri/src/perm_probe.rs && grep -q 'DevHostAttributed' desktop/src-tauri/src/perm_probe.rs
  `,
  spec: `
    Panel: not required — FIX (walk D5, P2).
    Root cause (walk step 2c): the dev binary is ad hoc signed and unbundled, so TCC attributes it to
    the RESPONSIBLE process (the launching terminal) and answered accessibility=true
    input_monitoring=true — the terminal's grants, shown as Yap's.
    DO — perm_probe gets pub fn running_as_bundle(exe_path, bundle_id_of_main_bundle,
    code_signature) -> bool (pure, unit-tested: /Applications/Yap.app/Contents/MacOS/wilson-voice with
    the Developer ID -> true; desktop/target/debug/wilson-voice ad hoc -> false). When false,
    probe_all marks every service ProbeState::DevHostAttributed (new variant) and the Permissions
    screen shows ONE line "dev build: permissions are the host terminal's" with no green rows and no
    Repair button (Repair would reset the wrong identity). The installed build is unchanged.
    Frontend: a vitest for the Permissions screen rendering DevHostAttributed.
  `,
  acceptance: `
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib running_as_bundle) > desktop/target/qa-dev-attrib.txt 2>&1
    grep -Eq 'test result: ok. [1-9][0-9]* passed' desktop/target/qa-dev-attrib.txt
    grep -rq "dev build: permissions are the host terminal's" desktop/src
    (cd desktop && npx vitest run --reporter=dot) 
    test -s docs/loop/evidence/yap-dev-perm-probe-attribution.md
    head -1 docs/loop/evidence/yap-dev-perm-probe-attribution.md | grep -qx 'ROOT-CAUSE-PROBE yap-dev-perm-probe-attribution'
  `,
})

ITEMS.push({
  id: 'loop-dependency-marker-stdout-only', prompt: 'loop-dependency-marker-stdout-only', branch: 'loop/harness-dependency-marker-executed-output-only', gated: null,
  title: 'The harness\'s dependency hold matches DEPENDENCY-NOT-ON-MAIN only as a line the EXECUTED preflight printed, never anywhere in the runner seat\'s JSON',
  preflight: `
    grep -q 'dependencyMarkerPrinted' scripts/loop/template.mjs && grep -q 'dependencyMarkerPrinted' scripts/loop/review-gate.test.mjs
  `,
  spec: `
    Panel: not required — FIX (walk D7, harness).
    Root cause: on main 3569ce4 the harness recorded yap-p0-perm-repair as "skipped: dependency not
    on main" although #228 is merged and its preflight passed. Every item preflight already assembles
    the marker at run time ("DEPENDENCY-NOT-ON-$(printf MAIN)"; build.mjs now REFUSES a literal one),
    but template.mjs runBuild tests /DEPENDENCY-NOT-ON-MAIN/ against asText(pre) — the WHOLE runner
    result JSON, including any free text the seat wrote (a note such as "no DEPENDENCY-NOT-ON-MAIN
    printed" trips it). Prove which field carried it from that run's log before changing code.
    DO — export a pure dependencyMarkerPrinted(pre) that looks only at the executed commands' captured
    stdout and matches /^DEPENDENCY-NOT-ON-MAIN:/m, and use it in runBuild. Tests (node --test, in
    review-gate.test.mjs or a sibling): marker line in stdout -> true; marker only in a notes/summary
    field -> false; marker text inside the echoed command string -> false.
  `,
  acceptance: `
    grep -q 'dependencyMarkerPrinted(pre)' scripts/loop/template.mjs
    ! grep -q '/DEPENDENCY-NOT-ON-MAIN/.test(asText(pre))' scripts/loop/template.mjs
    mkdir -p desktop/target && node --test --test-reporter=tap 'scripts/loop/*.test.mjs' > desktop/target/qa-dep-marker.txt 2>&1
    grep -q '^# fail 0$' desktop/target/qa-dep-marker.txt
    grep -Eq '^# pass [1-9][0-9]*$' desktop/target/qa-dep-marker.txt
    (cd desktop && npm run loop:build && npm run loop:validate)
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
