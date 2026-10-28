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
  name: "yap-overhaul-all-part-03",
  description:
    "Part 03 of the Yap (wilson-voice) overhaul, two or three builder lanes (args.lanes): land whatever is already gate-green, pre-flight on unmodified main (already-done items are skipped), build, open a labelled PR. The adversarial review, the independent second-Opus gate and the merge bar run afterwards in the same script with args {mode:'review'}. 32 items (PERM-A..yap-res-0), 8 awaiting the Senior Panel. The gate is local (Actions is disabled by the account spending limit); the DMG is not in it.",
  phases: [
    { title: "Recon", detail: "two lane worktrees, one npm ci + one warm cargo build each, the loop-build label, ci-mode measured" },
    { title: "PERM-A", detail: "Ask macOS the actual question: AVCaptureDevice authorizationStatus + requestAccess, replacing the device probe that cannot see a denial" },
    { title: "PERM-B", detail: "A denied microphone gets its own screen with a working System Settings deep link, not a green check" },
    { title: "PERM-C", detail: "Every hotkey press re-checks the grant, and the pill shows a denied state instead of recording silence" },
    { title: "Y1-A", detail: "A tap macOS disabled is re-armed and reported — the other half of \"the hotkey is dead\"" },
    { title: "PERM-D", detail: "First run refuses to reach calibration without a real grant, and a silent take is diagnosed instead of pasted as nothing" },
    { title: "SEC-A", detail: "Stop ad-hoc signing local builds — the reason permissions \"reset\" on Wilson's own machine" },
    { title: "Y1-B", detail: "Register the sleep/wake observer two later items already claim exists" },
    { title: "PERM-E", detail: "One permission health surface, watched for revocation, covering mic + Accessibility + Input Monitoring + audio capture" },
    { title: "yap-flakes-2026-10-16", detail: "Main goes green: per_chunk_deadline_not_per_take and meeting_manual_start_stop stop asserting on the scheduler" },
    { title: "yap-flake-timing-tests-under-load", detail: "Timing tests stop failing on a loaded Mac: CPU-time ratio plus a load guard that never silently passes" },
    { title: "yap-flake-sleep-test-worker-race", detail: "The sleep-mid-recording test stops racing the pipeline worker: a held worker, then both states asserted in order" },
    { title: "yap-lifecycle-2026-10-16", detail: "Quit means quit, reopen means a window: RunEvent::Reopen, one clear Quit, the pill gone at quit, bounded teardown, RSS flat under load" },
    { title: "yap-p0-hotkeys-no-permission", detail: "A take starts and stops with ZERO TCC grants: a fixed-chord dictation hotkey on Carbon RegisterEventHotKey is the primary path, the fn event tap is the extra, and a dead tap is shown as a degraded mode instead of a working hotkey" },
    { title: "yap-p0-perm-repair", detail: "One truthful grant probe per service (live API, never the Settings list) in perm_probe.rs, a detected \"stale identity\" state, and a one-click Repair: tccutil reset for Yap's own bundle id, open the exact pane, re-probe until granted" },
    { title: "yap-p0-insertion-bridge-fallback", detail: "Insertion never silently fails: AX insertion only when the probe says trusted, a clipboard + ⌘V bridge when PostEvent is granted, an instant Copy card when neither is, and one failing backend call can no longer blank the app as \"Bridge error\"" },
    { title: "yap-p0-release-smoke", detail: "Every DMG build runs a signed-app smoke after signing (codesign --strict on app + DMG with the pinned Developer ID team, a designated-requirement change gate, launch, probe every service, register hotkeys, insert into TextEdit) and the release FAILS when any step fails, including a stale-identity grant" },
    { title: "yap-qa-hotkey-walk-binding", detail: "hotkey-walk presses the CONFIGURED push-to-talk chord (fn, fn⌃, …) read from settings.json, has a dev-build flag, and the app shows a hint when fn is held without the binding's modifier" },
    { title: "yap-lic-00-kill-forge-fetch", detail: "SECURITY: delete the launch-time revocation GET to the dead Forge sslip.io host entirely; no licensing network call at all until the new client lands" },
    { title: "yap-dev-perm-probe-attribution", detail: "A dev build's permission probe says \"dev build: permissions are the host terminal's\" instead of showing the launching terminal's TCC grants as Yap's green rows" },
    { title: "loop-dependency-marker-stdout-only", detail: "The harness's dependency hold matches DEPENDENCY-NOT-ON-MAIN only as a line the EXECUTED preflight printed, never anywhere in the runner seat's JSON" },
    { title: "yap-lic-01-jws-verifier", detail: "Offline verifier for drivia[.]consulting license tokens: compact JWS, alg EdDSA + typ JWT pinned, kid looked up in EMBEDDED keys only (never a fetched key set), prd yap, ent.dictation, plan lifetime, exp + 30-day grace, rollback ratchet reused" },
    { title: "yap-lic-02-activation-client", detail: "Activate / refresh / deactivate against https://drivia[.]consulting/api/v1/licensing with X-Drivia-App; token + device_id in the Keychain (security-framework) with device_id mirrored in license.json; refresh at most every 24 h; 403 revoked/expired/deactivated + 401 reactivate_required/device_mismatch delete the token; never downgrade on network/5xx/429/400/413; NO key-list fetch; the Forge format deleted" },
    { title: "yap-lic-04-ui-states-and-buy", detail: "One license panel plus the existing tray state: key prefix, \"Manage Macs\" link to the portal, grace copy with the 60-day date, seat-limit lists devices; Buy opens https://drivia[.]consulting/yap (PAYMENT_LINK_URL -> PURCHASE_URL); no legacy state" },
    { title: "yap-lic-05-site-privacy-copy", detail: "Site, privacy and terms tell the truth for the launch: buying takes one email-code sign-in, activation needs the network once and re-checks, 60 days offline, Buy -> drivia[.]consulting/yap, updater -> yapvoice.app first; ships WITH the yap-lic-02/04 build" },
    { title: "yap-call-0", detail: "Call signal matrix: per-process mic-in-use + bundle id from CoreAudio, a call-app catalog, one fused confidence score, recorded fixtures, a --probe-calls CLI that never prints a raw title, and the walk script + coverage ledger" },
    { title: "yap-call-1", detail: "The pill says \"Zoom call — record it?\" (or \"FaceTime call — record your side?\") with a true consent line, offers once and then stays as a one-click chip for the life of the call, Offer / Never per app, and asks \"Call ended\" — it never starts or stops on its own" },
    { title: "yap-call-2", detail: "Browser calls in Chrome-family browsers (Chrome, Arc, Edge, Brave): Meet, Zoom web, Teams web, Slack huddles, Discord, Whereby, Webex — matched by the front tab's AX window title only when the browser holds the mic, with permission truth when Accessibility is off" },
    { title: "yap-call-3", detail: "DEFERRED to the next release (UNAPPROVED): SPIKE on macOS 26 — capture only the call app (CATapDescription.bundleIDs, exclusive = false) behind a default-OFF setting, or record NO-GO with evidence and keep global-exclude-self" },
    { title: "yap-byom-0", detail: "Bring your own model, the frame: a provider model (Bundled | OpenAI-compatible), per-task choice for notes and polish, API keys in the macOS Keychain bound to their host, one redirect-free HTTP client factory, Test connection, and a \"Leaves this Mac\" label on every choice that is TRUE" },
    { title: "yap-byom-1", detail: "OpenAI-compatible provider behind notes and polish: Ollama, LM Studio, OpenRouter and custom presets, \"Find local models\", a context-safe map/reduce, and a truthful fallback to the built-in model that never starts a silent download" },
    { title: "yap-byom-2", detail: "DEFERRED to the next release (UNAPPROVED): Anthropic provider on the native Messages API, built on the shared redirect-free client with x-api-key never leaving its host, same fallback and labels" },
    { title: "yap-res-0", detail: "Where your notes live: a Markdown+JSON folder (Obsidian-ready, edit-safe, with a true \"Leaves this Mac\" label for iCloud, Obsidian-iCloud and CloudStorage folders) and a signed webhook with a persisted outbox — cloud SaaS connectors and the SQLite file are deferred" },
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
const LOG = '/Users/wilsonguenther/Obsidian/Wilson-Brain/Projects/Loop-Logs/2026-09-12-yap-part03.md'
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
const PART = 'part-03'
/**
 * Stamped by build.mjs: item id -> the item FILE's ordinal (informational) or a pinned lane letter
 * ('A'|'B'|'C', item field `lane`). Only the PIN is read: since 2026-10-27 lanes are assigned PER PART
 * at run time (splitLanes below) — the old per-file round-robin left lane C idle for a whole part.
 */
const LANE_BY_ID = {"PERM-A":9,"PERM-B":9,"PERM-C":9,"Y1-A":9,"PERM-D":9,"SEC-A":9,"Y1-B":9,"PERM-E":9,"yap-flakes-2026-10-16":10,"yap-flake-timing-tests-under-load":10,"yap-flake-sleep-test-worker-race":10,"yap-lifecycle-2026-10-16":11,"yap-p0-hotkeys-no-permission":12,"yap-p0-perm-repair":12,"yap-p0-insertion-bridge-fallback":12,"yap-p0-release-smoke":12,"yap-qa-hotkey-walk-binding":13,"yap-lic-00-kill-forge-fetch":13,"yap-dev-perm-probe-attribution":13,"loop-dependency-marker-stdout-only":13,"yap-lic-01-jws-verifier":14,"yap-lic-02-activation-client":14,"yap-lic-04-ui-states-and-buy":14,"yap-lic-05-site-privacy-copy":14,"yap-call-0":15,"yap-call-1":15,"yap-call-2":15,"yap-call-3":15,"yap-byom-0":15,"yap-byom-1":15,"yap-byom-2":15,"yap-res-0":15}
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

// yap-flake-timing-tests-under-load — Written 2026-10-27 by a spec-writer seat. Audit 2026-10-27: false red in
// fix-land on #259 at 1-minute load ~39 (ratio ~21x against a 1.6x ceiling); the identical head passed at load ~6.
// STANDARD class: every file touched (desktop/src-tauri/tests/*.rs, optionally scripts/ none) misses SECURITY_PATHS in
// scripts/loop/review-gate.mjs:113-118 (checked: no tests/ path matches db, migration, sidecar, updater/sign/network/http
// keywords). CAUTION: tests/diarize_sidecar_*.rs and tests/matrix_row7_diarize_sidecar_wedge.rs match /sidecar/i, so if
// the PR edits ANY of those files it becomes SECURITY class (two Opus seats); step 3 therefore defaults those to
// "left alone" unless the sweep proves one is a ratio assert. Say so in the PR body.
ITEMS.push({
  id: 'yap-flake-timing-tests-under-load', prompt: 'yap-flakes', branch: 'loop/yap-flake-timing-tests-under-load', gated: null, builder: 'sonnet',
  title: 'Timing tests stop failing on a loaded Mac: CPU-time ratio plus a load guard that never silently passes',
  preflight: `
    test -f desktop/src-tauri/tests/long_take_budget.rs
    grep -q 'fn chunk_decode_wall_per_audio_second' desktop/src-tauri/tests/long_take_budget.rs
    grep -q '^libc = ' desktop/src-tauri/Cargo.toml
  `,
  notes: `Audit 2026-10-27: false red in fix-land on #259 at load 39; passed at load 6.`,
  spec: `
    Panel: not required — FIX (unambiguous intent: a gate must not go red because other seats are compiling)
    DEPENDS: none
    CLASS: standard (see header comment for the exempt-file rule in step 3)

    EVIDENCE
      - desktop/src-tauri/tests/long_take_budget.rs:381 chunk_decode_wall_per_audio_second times a
        single-window decode (t0, line 420) and a 15 s-window chunked decode (t1, line 428) with
        Instant::now()/elapsed() and asserts the wall ratio against
        CHUNK_DECODE_WALL_PER_AUDIO_SECOND_RATIO_CEILING = 1.60 (const at line 98; assert at line 450).
        With four to six cargo seats running at once, the two arms get different scheduler
        slices; measured ~21x at load ~39, passed at load ~6 on the identical head.
      - The module doc (lines 26-28) says the ratio is "deliberately not a wall-clock number",
        but the implementation measures wall clock; the intent is a CPU-cost ratio.
      - libc = "0.2" is already a direct dependency (desktop/src-tauri/Cargo.toml:45), so NO new
        dependency is needed or allowed. The file already holds heavy() (line 173), a mutex that
        serialises the heavy tests within the binary, so process CPU time is not polluted by
        another heavy test.

    DO
      1. CPU-time ratio. In long_take_budget.rs add a private fn process_cpu_micros() -> Option<u128>
         that calls libc::getrusage(libc::RUSAGE_SELF, &mut ru) (unsafe, zeroed libc::rusage), returns
         None on a non-zero return, else (ru_utime + ru_stime) in microseconds. Bracket the single arm
         and the chunked arm with it exactly where t0/t1 bracket them now, while _heavy is held.
         Assert the SAME 1.60 ceiling against cpu_chunked / cpu_single (max(1) on the divisor) when both
         readings are Some and cpu_single is at least 50_000 us (below that the tick granularity makes
         the ratio noise: print "cpu ratio unusable" and fall to step 2's path). Keep the wall-clock
         figures in the println as information only. Update the doc comment above the test and the
         const's doc comment to say the ratio is CPU time; do not change the 1.60 value.
         If getrusage proves unusable (None), choose step 2 only and say so in the PR body.
      2. Load guard. Add pure fn should_skip_timing(load: f64, cores: usize) -> bool { load > 2.0 * cores as f64 }
         and fn load_average_1m() -> Option<f64> using libc::getloadavg(&mut [f64; 3], 3) (returns < 1 on
         failure -> None), and cores from std::thread::available_parallelism(). Wherever the test would
         assert a WALL-clock ratio (the fallback path of step 1, or any assertion that stays wall-clock),
         run the decode and the functional asserts first (windows > 1 at line 444 stays unconditional),
         then if should_skip_timing(load, cores): eprintln!("skipped: load {load:.1} > threshold {thr:.1}")
         and return WITHOUT asserting the ratio. Never return before the decode runs. Never silently pass:
         the eprintln is mandatory, and if the load is unreadable (None) the assertion runs.
         Put should_skip_timing and load_average_1m in desktop/src-tauri/tests/support/mod.rs so step 3
         files share one copy (mod support already exists in the tree); keep long_take_budget.rs importing it.
      3. Sweep. Run: grep -rn "elapsed\\|Instant::now" desktop/src-tauri/src desktop/src-tauri/tests
         In the PR body list EVERY hit group by file with a disposition. Guard (same skip, same eprintln)
         only an assertion that compares a measured elapsed Duration to a RATIO or to an upper bound the
         scheduler can blow (examples to examine first: tests/matrix_row5_journal_backpressure.rs:131 and
         :173, tests/meeting_capture_rt_safety.rs:194, tests/matrix_new_asr_chunk_timeout.rs:250,
         tests/long_take_budget.rs:314-319 spill-overhead ceiling). Leave alone, with the reason written
         in the PR body: src/ hits (production timing, not assertions), deadline-polling loops
         (while Instant::now() < deadline), already-fixed wait_until shapes (meeting_manual_start_stop.rs),
         and any assertion whose point is the bound itself (a hang detector such as
         matrix_new_asr_chunk_timeout.rs HANG proves a deadline fires; guarding it would hide the bug it
         exists to catch, so exempt it and say so). Any test file whose NAME contains the word for
         an out-of-process helper (the diarize pool, handshake and wedge tests) is EXEMPT: editing one
         would silently raise this PR to security class, so list those hits as "left alone: class
         boundary" and file them in docs/loop/DEFERRED.md instead. Do NOT touch dictation_chunked.rs or
         meeting_manual_start_stop.rs (the sibling item yap-flakes-2026-10-16 owns them).
         Do not add #[ignore], a retry loop, or widen any bound.
      4. Tests. In support/mod.rs add #[cfg(test)] unit tests for should_skip_timing: (39.0, 12) true,
         (6.0, 12) false, (24.0, 12) false (equal is not over), (24.1, 12) true, (0.0, 1) false. If
         support/mod.rs is included by many test binaries, use #[allow(dead_code)] on the helpers so no
         binary emits a warning; confirm cargo test emits no new warnings.
      5. PR body: the step 3 table (file:line, assertion, guarded or left alone, why) plus the exit codes below.
  `,
  acceptance: `
    grep -q 'fn should_skip_timing' desktop/src-tauri/tests/support/mod.rs
    grep -q 'getrusage' desktop/src-tauri/tests/long_take_budget.rs
    grep -q 'skipped: load' desktop/src-tauri/tests/long_take_budget.rs
    ! grep -qF '#[ignore]' desktop/src-tauri/tests/long_take_budget.rs
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    cd desktop/src-tauri && cargo test --features custom-protocol --test long_take_budget ; test $? -eq 0
    cd desktop/src-tauri && cargo test --features custom-protocol ; test $? -eq 0
    grep -rn "elapsed()" desktop/src-tauri/tests | grep -E "assert|<|>" | cat
  `,
})

// yap-flake-sleep-test-worker-race — Written 2026-10-27 by a spec-writer seat. Audit 2026-10-27: false red in the land
// gate on two unrelated heads (1 of 5 runs on one head, twice on another that touches no meeting code):
// meeting_survives_sleep::will_sleep_mid_recording_finalizes_partial_then_acknowledges asserts left "transcribing",
// right "partial". A RACE IN THE TEST: the real pipeline worker takes the row and moves it to transcribing before the
// test reads it. No existing gate precedes that state write (asr, demand and quit in PipelineDeps are all consulted
// AFTER run_meeting_job's set_meeting_state(Transcribing), meeting_pipeline.rs:~499 at e0988a8), so option (a) "reuse an
// existing gate" is NOT available; this item takes option (b): a hook field mirroring the existing after_transcript.
// STANDARD class: securityClassOf (review-gate.mjs:138-145) tests SECURITY_PATHS (:113-118) against every file;
// meeting_pipeline.rs, lib.rs, tests/meeting_survives_sleep.rs and tests/meeting_pipeline_wired.rs match none of
// db|migration|polish_protocol|sidecar|updater|sign|notariz|keychain|licens|entitle|permission|accessibility|
// microphone|network|http|upload|telemetry nor scripts/loop/. So builder: 'sonnet'. If the builder ends up touching any
// path that matches (say so in the PR body), the PR becomes security class and needs two Opus seats.
// ANCHOR CAVEAT: the test file was written by PR #260 (commit e0988a8, yap24-NT10) and is not in the working tree this
// spec was written from; line numbers below are from e0988a8 and the builder re-verifies them on current main.
ITEMS.push({
  id: 'yap-flake-sleep-test-worker-race', prompt: 'yap-flakes', branch: 'loop/yap-flake-sleep-test-worker-race', gated: null, builder: 'sonnet',
  title: 'The sleep-mid-recording test stops racing the pipeline worker: a held worker, then both states asserted in order',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/tests/meeting_survives_sleep.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 260 (NT10, meeting survives sleep) has not landed; land it, then build this item"; exit 0; fi; grep -q 'fn will_sleep_mid_recording_finalizes_partial_then_acknowledges' desktop/src-tauri/tests/meeting_survives_sleep.rs && grep -q 'pub after_transcript' desktop/src-tauri/src/meeting_pipeline.rs )
  `,
  notes: `Audit 2026-10-27: false red in the land gate on two unrelated heads; worker advances the row before the read.`,
  spec: `
    Panel: not required — FIX (unambiguous intent: a gate must not go red on a race in a test)
    DEPENDS: none
    CLASS: standard (see header comment; if you touch a SECURITY_PATHS file, say so in the PR body)

    EVIDENCE
      - desktop/src-tauri/tests/meeting_survives_sleep.rs (as of e0988a8): the test starts with the REAL pipeline
        worker installed (spawn line 200, install line 202). controller.start (208) then the WillSleep injection
        (218) stops the meeting, which enqueues the id on that worker. run_meeting_job
        (src/meeting_pipeline.rs:454) reads the row, then calls set_meeting_state(id, Transcribing, ..) (about line
        499) and only afterwards builds the decoder via (deps.asr)(). The test reads the row in three places that
        expect "partial" BEFORE the worker is drained: inside the ack closure (222-227, logged "ack recording=false
        state=partial", compared at 231-238), and at 241-249 (assert at 242: state == "partial"). wait_idle (254) is
        the first synchronisation. Between enqueue and wait_idle the worker may already have flipped the row, hence
        left: "transcribing" right: "partial". It is a test race, not a product bug: the product is right to move
        the row to transcribing.
      - PipelineDeps (src/meeting_pipeline.rs:129-139) has hooks asr, demand, quit, sink, after_transcript (all
        invoked at or after the state write; after_transcript at ~593). There is NO hook that runs before the
        state write, so there is no existing gate to hold. The struct is built literally in exactly three places:
        src/lib.rs (about line 7190), tests/meeting_pipeline_wired.rs (deps(), line 121 sets after_transcript: None)
        and tests/meeting_survives_sleep.rs (deps(), line 116).

    DO
      1. The gate (option b, the minimum). In src/meeting_pipeline.rs add
           pub type BeforeJob = Arc<dyn Fn(&str) + Send + Sync>;
         and a field pub before_job: Option<BeforeJob> on PipelineDeps, documented as "called on the worker
         thread with the meeting id, before the job reads or advances the row; None in production". In
         run_meeting_job's caller, the worker loop in spawn (the for id in rx body, before catch_unwind(...
         run_meeting_job ...)), call it if Some. Do NOT put it inside run_meeting_job (pipeline tests call that
         function directly). Set before_job: None in lib.rs (the PipelineDeps literal near line 7190) and in
         tests/meeting_pipeline_wired.rs deps(). No cfg gate: the integration tests compile the lib without
         cfg(test), so a cfg(test) hook would not be visible to them. No behaviour change when None. No new
         dependency.
      2. In tests/meeting_survives_sleep.rs add a small test-local gate (about 20 lines): a struct Gate { open:
         Mutex<bool>, cv: Condvar } with hold() (sets closed), release() (sets open, notify_all) and wait()
         (cv.wait_while with a 30 s wait_timeout_while HANG guard that panics naming what never happened; the
         timeout is a hang guard, never a measurement). deps() takes the Gate and sets before_job to call
         gate.wait(). In will_sleep_mid_recording_finalizes_partial_then_acknowledges: build the gate CLOSED
         before spawn, keep an Arc for the test. Never add a sleep of any kind.
      3. Assert both states, in order. While the gate is closed run all the existing partial assertions
         unchanged (ack-time log 231-238, row 241-249 including line 242 state == partial, error note, WAV
         paths, diagnostics). Then gate.release(); pipeline.wait_idle(30 s) (existing line 254); then assert the
         final state: segments on both tracks (existing 259-265) and row.state == "partial" with the sleep
         sentence (existing 266-267). Add one explicit comment line: "partial observed while the worker was
         held, still partial after it drained" so the sequence, not a snapshot, is what the test proves. The
         other two states are NOT interchangeable: the test must not accept "transcribing" anywhere.
         If the held assertion at 222-227 is inside the ack closure on the test thread, that is fine: the
         worker cannot run past before_job while the gate is closed.
      4. Sweep the same pattern (read after a spawn or enqueue with no gate). Run
           grep -rn "meeting_pipeline::spawn\\|pipeline.enqueue\\|\\.enqueue(" desktop/src-tauri/tests
         and read every hit. Known set at e0988a8: meeting_survives_sleep.rs (this test, FIXED), the other three
         tests in that same file (no pipeline spawned: the idle test builds none, the message-table test and the
         power.rs source test never touch a row; say so), and tests/meeting_pipeline_wired.rs:177 (spawns and
         installs a pipeline; read the test at 177-200 onward and decide: if it reads a meeting row between enqueue
         and wait_idle it is racy and gets the same Gate; if it only reads after wait_idle it is not racy, say
         why). Also grep tests/meeting_dictation_preempts_transcription.rs and tests/meeting_capture_rt_safety.rs
         for a worker (run_meeting_job or spawn) and give each a disposition. Put the table in the PR body:
         file:line, what is read, FIXED or "not racy because X". Do not touch dictation_chunked.rs or
         meeting_manual_start_stop.rs (yap-flakes-2026-10-16 owns them). Do not add #[ignore], a retry loop,
         or widen any bound.
      5. Tests. A unit-level proof of the hook in meeting_pipeline.rs tests is NOT required; the integration
         test is the proof. Confirm cargo test emits no new warnings (before_job is read in spawn, so no
         dead_code). Put the five exit codes and the full cargotest exit in the PR body.
  `,
  acceptance: `
    grep -q 'pub before_job' desktop/src-tauri/src/meeting_pipeline.rs
    grep -q 'before_job' desktop/src-tauri/tests/meeting_survives_sleep.rs
    ! git diff origin/main -- desktop/src-tauri/tests/meeting_survives_sleep.rs desktop/src-tauri/src/meeting_pipeline.rs | grep -E '^\\+.*(thread::sleep|[^_a-z]sleep\\()'
    ! grep -qF '#[ignore]' desktop/src-tauri/tests/meeting_survives_sleep.rs
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    cd desktop/src-tauri && for i in 1 2 3 4 5; do cargo test --features custom-protocol --test meeting_survives_sleep; e=$?; echo "RUN$i EXIT=$e"; test $e -eq 0 || exit 1; done
    cd desktop/src-tauri && cargo test --features custom-protocol ; test $? -eq 0
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
    Audit 2026-10-26: still fires on the installed 0.9.3-dev at launch: 16:35:12Z 'revocation refresh skipped (error sending request for url (https://forge.87-99-149-214.sslip.io/...))'. See docs/audits/meeting-notetaker-audit-2026-10-26.md issue #15.
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

// ── 07c-yap-licensing-2026-10-19.mjs ──────────────────────────────────────
// yap-licensing-2026-10-19 — Yap becomes a client of the drivia[.]consulting licensing API (database #2). Filed 2026-10-19 by seat
// file-license-items from ~/Obsidian/Wilson-Brain/Projects/Yap/licensing-migration-plan-2026-10-19.md ("## Items", "## Wilson's
// decisions", "## Panel 2026-10-19"). The server half is drivia-consulting scripts/loop/items/99i-yap-licensing-2026-10-19.mjs
// (dc-yap-01..07). yap-lic-00-kill-forge-fetch is NOT here: it is filed in 07b-yap-walk-defects-2026-10-19.mjs (PR #230) and ships
// alone, first. yap-lic-03 (dual-read legacy) and yap-lic-06 (remove legacy) were REJECTED by the panel; the Forge removal is folded
// into yap-lic-02.
//
// HOST SPELLING: this loop's drift guard (build.mjs DRIFT) bans the sibling host string, so these items write the licensing host as
// drivia[.]consulting in prose, drivia\\.consulting in greps and drivia.$(printf consulting) in curls. The REAL host has a plain dot:
// code constants, URLs and site copy use https://drivia + .consulting exactly (decision ledger; the server is drivia-consulting DB #2).
//
// WILSON'S DECISIONS (2026-10-19T03:45Z, "Perfect, I agree on all five." — binding, never relitigated):
//   1. $29 one-time LIFETIME, 3 Macs.   2. 60 days offline for lifetime (server: token_ttl_days 30 + grace_days 30).
//   3. Live webhook + price now, checkout closed until launch.   4. Forge signing key LOST, Forge key format DROPPED.
//   5. Copy approved: buying takes one email-code sign-in; the "no account" wording changes.
// PANEL 2026-10-19 — the three plan-level changes, as encoded here:
//   (1) NO JWKS / KEY-LIST FETCH, EVER. Yap trusts only public keys compiled into the binary (EMBEDDED_KEYS in license_v2.rs).
//       Rotation ships in a minisign-signed app update that embeds the new kid at least one release before the server signs with it.
//       An unknown kid means "re-activate", never "fetch". (Yap's reqwest uses rustls-platform-verifier, which trusts user-installed
//       root CAs, so a fetched key set is an offline keygen.) yap-lic-01 tests it; yap-lic-02 greps for its absence.
//   (2) Checkout stays closed (dc-yap-01 purchase_enabled=false) — the server's job; the Buy button here just opens drivia[.]consulting/yap,
//       which renders its own closed state.
//   (3) ONE LAUNCH (phases 2 + 3 merged): the build carrying yap-lic-02/04 is HELD OFF THE UPDATER FEED (no release:, no latest.json
//       publish) until dc-yap-03 and dc-yap-04 are live on drivia[.]consulting and yap-lic-05's copy ships in the same launch.
//
// LAUNCH SEQUENCE (same as 07): every builder starts from origin/main and build mode never lands an unreviewed PR. Items 2-4 carry a
// DEPENDENCY preflight: when the previous item (or a drivia-consulting item) is not on main, it prints the dependency marker and exits
// 0, and the harness records "skipped: dependency not on main". The marker is ASSEMBLED AT RUN TIME ($(printf MAIN)), never written
// literally (build.mjs refuses a literal marker, PR #230). Alternate build -> review, one item per round, in this order:
//   yap-lic-01-jws-verifier -> yap-lic-02-activation-client -> yap-lic-04-ui-states-and-buy -> yap-lic-05-site-privacy-copy
// Review: Opus only (review-opus + review-opus2). yap-lic-01 and yap-lic-02 are SECURITY class (two Opus security seats; the panel also
// owes a round-2 verification seat on both specs). No Codex, no Gemini, no third-party reviewer.
// HAZARD (same as 07/08): no seat launches the GUI app, signals or quits Wilson's running /Applications/Yap.app, or writes TCC; no seat
// reads or writes the real Keychain item of the installed app (tests use a throwaway service name). No live Stripe, no sends.
// SYMBOLS ARE CONTRACT: the module, function, constant and test names below are what later preflights grep for. Do not rename them.

ITEMS.push({
  id: 'yap-lic-01-jws-verifier', prompt: 'yap-lic-01-jws-verifier', branch: 'loop/yap-lic-01-jws-verifier-embedded-keys', gated: null,
  title: 'Offline verifier for drivia[.]consulting license tokens: compact JWS, alg EdDSA + typ JWT pinned, kid looked up in EMBEDDED keys only (never a fetched key set), prd yap, ent.dictation, plan lifetime, exp + 30-day grace, rollback ratchet reused',
  preflight: `
    ( M=0; grep -q 'launch_license_init_makes_no_network_call' desktop/src-tauri/src/license.rs || M=1; git grep -qnE 'REVOCATION_URL|fetch_revocations|spawn_revocation_refresh' -- desktop/src-tauri/src && M=1; if [ "$M" = 1 ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-lic-00-kill-forge-fetch has not landed (07b, PR #230: the revocation fetch is still in license.rs) — land it, then the build pass again"; exit 0; fi; test -f desktop/src-tauri/src/license_v2.rs && grep -q 'pub const EMBEDDED_KEYS' desktop/src-tauri/src/license_v2.rs && grep -q 'pub fn verify_token' desktop/src-tauri/src/license_v2.rs && grep -q 'fn non_embedded_key_fails_even_when_cached' desktop/src-tauri/src/license_v2.rs )
  `,
  spec: `
    Panel 2026-10-19: APPROVED-WITH-CHANGES (BLOCKING) — embedded keys only; grace constant 30 d; reuse the trial ratchet.
    DEPENDS: yap-lic-00-kill-forge-fetch ON MAIN. Blocks: yap-lic-02 (its preflight greps verify_token / EMBEDDED_KEYS).
    SECURITY CLASS: two Opus security reviewers.

    DO (names are contract)
      1. NEW module desktop/src-tauri/src/license_v2.rs (declared in lib.rs). pub const EMBEDDED_KEYS: &[(&str, [u8; 32])] holding
         kid "dcl-2026-09-24" with x = NTXQamoM4o9K1MkQrZ_bWPRL7659nEDHaIssiCWodHs (base64url, decoded at compile time or in a
         const fn / test-checked literal). A test (embedded_key_matches_server_seed) asserts the constant equals the public_jwk of kid
         dcl-2026-09-24 seeded at drivia-consulting supabase/migrations/20260924002500_licensing_api.sql:258. Copy that JWK into
         desktop/src-tauri/tests/fixtures/dcl-signing-keys.json, with no network. Uses the ed25519-dalek already in Cargo.toml (no new crate).
      2. pub fn verify_token(token: &str, now_ms: i64) -> Result<TokenClaims, TokenError>: exactly 3 dot-separated parts; header alg ==
         "EdDSA" and typ == "JWT" (anything else refused, incl. "none" and "HS256"); kid looked up in EMBEDDED_KEYS ONLY — an unknown
         kid returns TokenError::UnknownKid, whose UI meaning is "re-activate"; the signature is checked over the exact
         "header.payload" bytes BEFORE the payload is parsed; then prd == "yap", ent.dictation == true, plan == "lifetime"; then
         now <= exp + GRACE_S with pub const GRACE_S = 30 days (decision 2: the TTL comes from the token's own exp, the grace is a
         constant here). Stable error codes: bad_format, bad_alg, bad_typ, unknown_kid, bad_signature, wrong_product, no_entitlement,
         wrong_plan, expired.
      3. NO KEY FETCH (Panel BLOCKING): there is no code path that adds a key to the trusted set at runtime: no reqwest, no JWKS URL,
         no cache of keys on disk. The public verifier signature takes no key-set argument.
      4. ROLLBACK: reuse the existing trial ratchet in license.rs (the max(wall, monotonic, floor) clock and DB_KEY_CLOCK_FLOOR
         "license.max_seen_wall_ms", ~:114-:183) for "now"; also record the highest token iat seen (maxIat) in license.json AND the
         SQLite corroborating row, and refuse a token with iat below it. Do not write a second ratchet.
      5. Wire into license.rs entitlement as the v2 path (the Forge path is removed in yap-lic-02, not here).
    TESTS live in #[cfg(test)] mod tests inside license_v2.rs. pub fn verify_token calls a pub(crate) fn verify_with(keys, token, now_ms)
    with EMBEDDED_KEYS (crate-private, never pub; yap-lic-02's client core is its only other caller), and only #[cfg(test)] code
    passes throwaway keys. Vectors: embedded_key_matches_server_seed
    (reads the fixture), valid, tampered payload, tampered signature, alg none, alg HS256,
    typ missing, unknown kid, wrong prd, no ent.dictation, plan not lifetime, expired-in-grace (accepted), past-grace (refused),
    clock rollback (refused), iat below maxIat (refused), and non_embedded_key_fails_even_when_cached: a token signed by a key that
    is present in a would-be cache file on disk is still refused because only EMBEDDED_KEYS are consulted.`,
  acceptance: `
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    mkdir -p desktop/target
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib -- --list) > desktop/target/lic01-list.txt 2>/dev/null
    grep -q '^license_v2::tests::non_embedded_key_fails_even_when_cached: test$' desktop/target/lic01-list.txt
    grep -q '^license_v2::tests::alg_none_refused: test$' desktop/target/lic01-list.txt
    grep -q '^license_v2::tests::alg_hs256_refused: test$' desktop/target/lic01-list.txt
    grep -q '^license_v2::tests::unknown_kid_means_reactivate: test$' desktop/target/lic01-list.txt
    grep -q '^license_v2::tests::expired_in_grace_accepted: test$' desktop/target/lic01-list.txt
    grep -q '^license_v2::tests::past_grace_refused: test$' desktop/target/lic01-list.txt
    grep -q '^license_v2::tests::clock_rollback_refused: test$' desktop/target/lic01-list.txt
    grep -q '^license_v2::tests::embedded_key_matches_server_seed: test$' desktop/target/lic01-list.txt
    test -f desktop/src-tauri/tests/fixtures/dcl-signing-keys.json
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib license_v2::)
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib license)
    ! git grep -qnE 'reqwest|https?://' -- desktop/src-tauri/src/license_v2.rs
    grep -q 'pub const GRACE_S' desktop/src-tauri/src/license_v2.rs
    (cd desktop && npm run loop:validate)
  `,
})

ITEMS.push({
  id: 'yap-lic-02-activation-client', prompt: 'yap-lic-02-activation-client', branch: 'loop/yap-lic-02-activation-client-forge-removed', gated: null,
  title: 'Activate / refresh / deactivate against https://drivia[.]consulting/api/v1/licensing with X-Drivia-App; token + device_id in the Keychain (security-framework) with device_id mirrored in license.json; refresh at most every 24 h; 403 revoked/expired/deactivated + 401 reactivate_required/device_mismatch delete the token; never downgrade on network/5xx/429/400/413; NO key-list fetch; the Forge format deleted',
  preflight: `
    ( M=0; test -f desktop/src-tauri/src/license_v2.rs || M=1; grep -q 'pub fn verify_token' desktop/src-tauri/src/license_v2.rs || M=1; grep -q 'pub const EMBEDDED_KEYS' desktop/src-tauri/src/license_v2.rs || M=1; GH_PAGER=cat GH_PROMPT_DISABLED=1 gh api repos/wilsonguenther-dev/$(printf drivia)-consulting/contents/docs/licensing/YAP-ROUNDTRIP.md --silent >/dev/null 2>&1 || M=1; if [ "$M" = 1 ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-lic-01-jws-verifier (license_v2.rs verify_token) and drivia-consulting dc-yap-05-test-roundtrip (docs/licensing/YAP-ROUNDTRIP.md) must both be on main — land them, then the build pass again"; exit 0; fi; test -f desktop/src-tauri/src/license_client.rs && grep -q 'pub const LICENSING_BASE' desktop/src-tauri/src/license_client.rs && ! git grep -qnE 'ISSUER_SKID|GRACE_PLAN|PRODUCTION_SIGNED_FIXTURE|ISSUER_PUBLIC_KEY_SPKI_B64' -- desktop/src-tauri/src desktop/src )
  `,
  spec: `
    Panel 2026-10-19: APPROVED-WITH-CHANGES (BLOCKING) — delete the daily JWKS fetch; mirror device_id; name the Keychain crate;
    handle 400/413/403 blocked; refresh at most once per 24 h; ALSO delete the Forge format (yap-lic-06's scope, rejected as a separate
    item); retarget license_gate.rs and tray_license.rs.
    DEPENDS: yap-lic-01 ON MAIN and drivia-consulting dc-yap-05 ON ITS MAIN (preflight). Blocks: yap-lic-04.
    SECURITY CLASS: two Opus security reviewers.
    HELD OFF THE UPDATER (Panel, one launch): merging this item is fine; no release: commit and no updates/latest.json publish may carry
    it until dc-yap-03 + dc-yap-04 are live and yap-lic-05 ships with it. Wilson's own comp DCL license (issued from /desk, plan lifetime)
    must exist before that release, because the Forge key path is gone after it.

    DO (names are contract)
      1. NEW desktop/src-tauri/src/license_client.rs: pub const LICENSING_BASE = "https://drivia[.]consulting/api/v1/licensing" (a test
         asserts no .vercel.app and no other host). activate / refresh / deactivate per drivia-consulting docs/licensing/CLIENT-INTEGRATION.md,
         header X-Drivia-App = the yap app key. Activation input accepts DCL- keys only.
      2. KEYCHAIN: crate security-framework (already in Cargo.lock through rustls-platform-verifier; add it as a direct dependency at the
         locked version, no new download). Token and device_id stored as generic passwords, accessibility AfterFirstUnlockThisDeviceOnly.
         device_id = random UUID created once. MIRROR device_id into license.json (it is not a secret) so a Keychain read failure after a
         signing-identity change re-activates the SAME seat instead of burning one. On any Keychain failure fall back to re-activation,
         never lock out.
      3. REFRESH: at most once every 24 h, and always when under 7 days remain to exp. No per-launch refresh beyond that rule,
         except a HELD (403 disputed) license: it refreshes every launch (:123).
      4. NO KEY-LIST FETCH (Panel BLOCKING): no JWKS URL, no key download, no merge into a cached set. Trust = license_v2::EMBEDDED_KEYS.
         An UnknownKid from verify_token triggers re-activation, not a fetch.
      5. RESPONSES: 200 stores the token; 404 invalid_key and 409 seat_limit are shown (409 lists the devices, links the portal);
         FAIL CLOSED (CLIENT-INTEGRATION.md:122): 403 revoked, 403 expired, 403 deactivated, 401 reactivate_required and 401
         device_mismatch delete the token and show activation with the reason. Refresh returns 403 deactivated for a seat freed in
         the portal or a staff-blocked device (licensing_api.sql:149); keeping the token would let seat-cycling exceed 3 Macs.
         403 disputed (:123) keeps the token but is NOT entitled: PERSIST the hold (license.json +
         settings_kv) across relaunch and offline: a valid token never re-entitles it; only a 200 refresh clears it. Dictation
         stops, its message shows.
         403 blocked (activate only) and 400 / 413 never downgrade a valid stored token; 401 invalid_token per CLIENT-INTEGRATION.md;
         5xx / timeout / offline keep the token; 429 honours Retry-After. Deactivate frees the seat server-side BEFORE clearing locally.
      6. lib.rs: activate_license / deactivate_license commands call license_client.
      7. FORGE REMOVAL (decision 4): delete ISSUER_PUBLIC_KEY_SPKI_B64, ISSUER_SKID, GRACE_PLAN and its grace path,
         PRODUCTION_SIGNED_FIXTURE and the test that verifies it, and the legacy branch of decide_entitlement in license.rs, and every
         drivia-forge citation (license.rs:14, :164, :1576; desktop/src/license/status.ts:61). KEEP the license tests: retarget
         desktop/src-tauri/tests/license_gate.rs and tests/tray_license.rs to the v2 entitlement path (license_gate.rs reads source
         through include_str and tray_license.rs builds LicenseStatus values; neither one signs tokens).
    TESTS live in #[cfg(test)] mod tests inside license_client.rs, against a local stub HTTP server. The client core takes (base, keys)
    privately, and release code has no runtime base or key override (the core reaches license_v2's verify_with crate-privately, never
    pub). No internet. Cases: 200, 404 invalid_key, 409 seat_limit,
    403 revoked/expired/deactivated and 401 reactivate_required/device_mismatch delete token, 403 disputed keeps it but not
    entitled, the three disputed_hold_* tests in the acceptance, 403 blocked keeps it, 400 keeps it, 413 keeps it, 503/timeout keeps it, 429 honours
    Retry-After, refresh skipped inside 24 h, refresh forced under 7 d, keychain_delete_reactivates_same_seat (deletes the throwaway
    Keychain item, re-activation sends the mirrored device_id).
    MANUAL PROOF (PR body): activate a test-mode key from dc-yap-05 in a clean YAP_DATA_DIR with a dev build (never the installed app),
    dictation completes, deactivate frees the seat.`,
  acceptance: `
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    mkdir -p desktop/target
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib -- --list) > desktop/target/lic02-list.txt 2>/dev/null
    grep -q '^license_client::tests::disputed_403_keeps_token: test$' desktop/target/lic02-list.txt
    grep -q '^license_client::tests::revoked_403_deletes_token: test$' desktop/target/lic02-list.txt
    grep -q '^license_client::tests::expired_403_deletes_token: test$' desktop/target/lic02-list.txt
    grep -q '^license_client::tests::deactivated_403_deletes_token: test$' desktop/target/lic02-list.txt
    grep -q '^license_client::tests::disputed_403_not_entitled: test$' desktop/target/lic02-list.txt
    grep -q '^license_client::tests::disputed_hold_survives_relaunch_offline: test$' desktop/target/lic02-list.txt
    grep -q '^license_client::tests::disputed_hold_retries_inside_24h: test$' desktop/target/lic02-list.txt
    grep -q '^license_client::tests::disputed_hold_cleared_by_200: test$' desktop/target/lic02-list.txt
    grep -q '^license_client::tests::reactivate_required_401_deletes_token: test$' desktop/target/lic02-list.txt
    grep -q '^license_client::tests::device_mismatch_401_deletes_token: test$' desktop/target/lic02-list.txt
    grep -q '^license_client::tests::blocked_403_keeps_token: test$' desktop/target/lic02-list.txt
    grep -q '^license_client::tests::too_large_413_keeps_token: test$' desktop/target/lic02-list.txt
    grep -q '^license_client::tests::retry_after_429_honoured: test$' desktop/target/lic02-list.txt
    grep -q '^license_client::tests::refresh_skipped_inside_24h: test$' desktop/target/lic02-list.txt
    grep -q '^license_client::tests::keychain_delete_reactivates_same_seat: test$' desktop/target/lic02-list.txt
    (cd desktop/src-tauri && cargo test --features custom-protocol --test license_gate)
    (cd desktop/src-tauri && cargo test --features custom-protocol --test tray_license)
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib license)
    ! git grep -qn 'vercel.app' -- desktop/src-tauri/src/license_client.rs
    ! git grep -qniE 'well-known|licensing-keys|jwks' -- desktop/src-tauri/src
    ! git grep -qnE 'ISSUER_SKID|GRACE_PLAN|PRODUCTION_SIGNED_FIXTURE|ISSUER_PUBLIC_KEY_SPKI_B64|drivia-forge' -- desktop/src-tauri/src desktop/src
    grep -q 'device_id' desktop/src-tauri/src/license_client.rs
    (cd desktop && npm run loop:validate)
  `,
})

ITEMS.push({
  id: 'yap-lic-04-ui-states-and-buy', prompt: 'yap-lic-04-ui-states-and-buy', branch: 'loop/yap-lic-04-license-panel-buy-drivia', gated: null,
  title: 'One license panel plus the existing tray state: key prefix, "Manage Macs" link to the portal, grace copy with the 60-day date, seat-limit lists devices; Buy opens https://drivia[.]consulting/yap (PAYMENT_LINK_URL -> PURCHASE_URL); no legacy state',
  preflight: `
    ( M=0; test -f desktop/src-tauri/src/license_client.rs || M=1; grep -q 'pub const LICENSING_BASE' desktop/src-tauri/src/license_client.rs || M=1; git grep -qn 'GRACE_PLAN' -- desktop/src-tauri/src && M=1; GH_PAGER=cat GH_PROMPT_DISABLED=1 gh api "repos/wilsonguenther-dev/$(printf drivia)-consulting/contents/src/app/%28site%29/yap/page.tsx" --silent >/dev/null 2>&1 || M=1; if [ "$M" = 1 ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-lic-02-activation-client (license_client.rs, Forge format gone) and drivia-consulting dc-yap-03-yap-buy-page (src/app/(site)/yap/page.tsx) must both be on main — land them, then the build pass again"; exit 0; fi; grep -q 'pub const PURCHASE_URL: &str = "https://drivia\\.consulting/yap"' desktop/src-tauri/src/license.rs && ! git grep -qn 'buy.stripe.com' -- desktop/src-tauri/src desktop/src )
  `,
  spec: `
    Panel 2026-10-19: APPROVED-WITH-CHANGES — no legacy state; ONE license panel plus the EXISTING tray state (the pill and tray copy
    matrix beyond that is cut); key prefix + "Manage Macs" portal link; grace copy shows the 60-day date.
    DEPENDS: yap-lic-02 ON MAIN and drivia-consulting dc-yap-03 ON ITS MAIN (preflight).
    HELD OFF THE UPDATER with yap-lic-02 (one launch, see header).

    DO (names are contract)
      1. license.rs: PAYMENT_LINK_URL (:176, buy.stripe.com) becomes pub const PURCHASE_URL: &str = "https://drivia[.]consulting/yap".
         Every caller (frontend PurchasePrompt.tsx and the tray) uses it. The page itself says when purchases are closed; Yap shows no
         separate closed state.
      2. desktop/src/license/LicensePanel.tsx + status.ts: one panel covering not activated / active (key prefix only, e.g. "DCL-ABCD…",
         never the full key) / offline in grace ("Works offline until <date>", the date = token exp + 30 d, i.e. up to 60 days) /
         on hold (403 disputed) / revoked / seat limit (lists the devices returned with 409, plus a "Manage Macs" link to
         https://drivia[.]consulting/portal/licenses). "Manage Macs" also shows in the active state.
      3. Tray: keep the existing tray license state; map the new states onto it, no new tray copy matrix.
      4. No "Legacy key" state anywhere (decision 4).
    TESTS: vitest for status.ts state mapping and LicensePanel rendering per state (src/license); cargo tray_license still green.
    SCREENSHOTS: each panel state, headless, from a dev build with a throwaway YAP_DATA_DIR (never the installed app), in
    docs/pr-screenshots/yap-lic-04-ui-states-and-buy/.`,
  acceptance: `
    (cd desktop && npm ci)
    (cd desktop && npm run build)
    (cd desktop && cargo build -p yap-polish --release)
    (cd desktop && cargo build -p yap-diarize --release)
    T="$CARGO_TARGET_DIR"; test -n "$T" || T="$PWD/desktop/target"; mkdir -p desktop/src-tauri/binaries && cp "$T/release/yap-polish" "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')" && cp "$T/release/yap-diarize" "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"
    mkdir -p desktop/target
    ! git grep -qn 'buy.stripe.com' -- desktop/src-tauri/src desktop/src
    ! git grep -qni 'legacy key' -- desktop/src
    grep -q 'pub const PURCHASE_URL: &str = "https://drivia\\.consulting/yap"' desktop/src-tauri/src/license.rs
    grep -q 'Manage Macs' desktop/src/license/LicensePanel.tsx
    (cd desktop && npx vitest run src/license src/pill/license.test.ts)
    (cd desktop/src-tauri && cargo test --features custom-protocol --test tray_license)
    (cd desktop/src-tauri && cargo test --features custom-protocol --lib license)
    test -n "$(ls docs/pr-screenshots/yap-lic-04-ui-states-and-buy/*.png 2>/dev/null)"
    (cd desktop && npm run loop:validate)
  `,
})

ITEMS.push({
  id: 'yap-lic-05-site-privacy-copy', prompt: 'yap-lic-05-site-privacy-copy', branch: 'loop/yap-lic-05-site-privacy-terms-copy', gated: null,
  title: 'Site, privacy and terms tell the truth for the launch: buying takes one email-code sign-in, activation needs the network once and re-checks, 60 days offline, Buy -> drivia[.]consulting/yap, updater -> yapvoice.app first; ships WITH the yap-lic-02/04 build',
  preflight: `
    ( M=0; grep -q 'pub const PURCHASE_URL: &str = "https://drivia\\.consulting/yap"' desktop/src-tauri/src/license.rs || M=1; GH_PAGER=cat GH_PROMPT_DISABLED=1 gh api repos/wilsonguenther-dev/$(printf drivia)-consulting/contents/scripts/stripe/ensure-yap-live.mjs --silent >/dev/null 2>&1 || M=1; test "$(curl -s -o /dev/null -w '%{http_code}' https://drivia.$(printf consulting)/yap)" = 200 || M=1; if [ "$M" = 1 ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-lic-04-ui-states-and-buy (PURCHASE_URL) must be on main and drivia-consulting dc-yap-03 + dc-yap-04 must be live (/yap answers 200, ensure-yap-live.mjs on its main) — land them, then the build pass again"; exit 0; fi; ! git grep -qnE 'buy.stripe.com|refunded licenses|refunded-licenses|no login, no server|No sign-in, no email' -- site/dist PRIVACY.md TERMS.md && grep -q 'drivia\\.consulting/yap' site/dist/index.html && grep -q 'https://yapvoice.app/updates/latest.json' desktop/src-tauri/tauri.conf.json && ! git grep -qinE 'involves no server at all|no device identifier|no account, no sign-in|revocation check|network call the licensing path makes' -- site/dist/privacy.html PRIVACY.md && ! git grep -qinE 'one-way hash of your email|not the address itself|without holding a list of who bought|none of them ever reach us|stored in a small file|re-verified from that file|crash reports, usage data, or personal information' -- site/dist/privacy.html PRIVACY.md && ! git grep -qinE 'update check, refund list|no subscription, no account' -- site/dist/index.html && grep -qi 'Keychain' site/dist/privacy.html && grep -qi 'Keychain' PRIVACY.md && grep -qiE 'email[- ]code' site/dist/privacy.html && grep -qiE 'email[- ]code' PRIVACY.md && grep -qiE 'IP( address)? prefix' site/dist/privacy.html && grep -qiE 'IP( address)? prefix' PRIVACY.md )
  `,
  spec: `
    Panel 2026-10-19: APPROVED-WITH-CHANGES — ships in the SAME launch as the yap-lic-02/04 build; scope adds site/dist/terms.html;
    carries the copy diffs below; proves yapvoice.app/updates/latest.json answers 200 before the endpoint swap. Decision 5 = approved.
    DEPENDS: yap-lic-04 ON MAIN, and drivia-consulting dc-yap-03 + dc-yap-04 LIVE (preflight).
    ONE LAUNCH (Panel, phases 2 + 3 merged): the new app build (yap-lic-02/04) is HELD OFF THE UPDATER until dc-yap-03 and dc-yap-04 are
    live on drivia[.]consulting AND this item's copy is merged; this item's site deploy and that build's updater publish go out together.
    Never publish the site copy before the build, nor the build before the copy. Checkout itself stays closed until Wilson's flip
    (dc-yap-01 / docs/runbooks/YAP-GO-LIVE.md); the copy is true either way.

    COPY DIFFS (sense fixed by the panel; final words are the builder's, in Wilson's voice, no bullets in prose)
      site/dist/index.html:72        "Account: None" -> "Account: email, only to buy"
      site/dist/index.html:180-181   "No account — No sign-in, no email, no profile." -> "No account to use it. Buying takes one email-code
                                     sign-in; that email is your account."
      site/dist/index.html:201       "no login, no server … the one call licensing makes is downloading a public list of refunded
                                     licenses" -> "Buy with an email code at drivia[.]consulting. Activate once with your key; Yap re-checks it
                                     when online and keeps working up to 60 days offline."   (if yap-lic-00 already removed the
                                     refund-list sentence, rewrite what is there to this sense)
      site/dist/index.html:204       network list ends "…a background download of the public refunded-licenses list" -> "…license
                                     activation and a periodic re-check with drivia[.]consulting"
      site/dist/index.html:229       href buy.stripe.com/… -> href="https://drivia[.]consulting/yap"
      site/dist/index.html:166       "no account" about transcription: UNCHANGED (still true for using the app)
      site/dist/privacy.html:51 and PRIVACY.md:34   revocation-check paragraph -> "License activation and re-check: sends your key, a random
                                     install id, your Mac's name and the app/macOS version to drivia[.]consulting. We keep your email, a
                                     salted device hash, the Mac name and a coarse IP prefix. Nothing from your dictation."
                                     (Panel Security: device_name is often the user's real name, and the IP prefix is stored — disclose both.)
      site/dist/privacy.html:30 and PRIVACY.md:9    "Yap has no account, no sign-in, and no server that receives your speech…" -> keep the sense
                                     "nothing about your dictation reaches a server", but drop "no account, no sign-in" (buying takes one
                                     email-code sign-in, and activation talks to drivia[.]consulting). The ENTIRE old revocation-check
                                     paragraph goes: "The revocation check", "the only network call the licensing path makes", "involves no
                                     server at all" and "no license key, no email, no device identifier" are all false after yap-lic-02.
      site/dist/privacy.html:43 and PRIVACY.md:24   purchase-record paragraph ("a one-way hash of your email address (not the address
                                     itself) ... we can email your key back to you without holding a list of who bought Yap") -> "Buying takes
                                     one email-code sign-in at drivia[.]consulting, and that email is your account. The server keeps the
                                     purchase record against it: your email address, the license's own identifiers, the Stripe session and
                                     payment identifiers, the plan and seat count, and the date. Your keys are shown in your account portal
                                     and emailed to you, so a lost key is one sign-in away." (Decision 5. The hash-only claim is false after
                                     launch: the license row references the account, drivia-consulting 20260924002400_licensing.sql:65.)
      site/dist/privacy.html:44 and PRIVACY.md:26   "Your name, your address ... none of them ever reach us" -> "Your postal address, your
                                     transcripts and anything you dictated never reach us. The Mac name sent at activation (below) can carry
                                     your real name, and it is disclosed there." (the activation call sends the Mac name)
      site/dist/privacy.html:53 and PRIVACY.md:36   "Yap sends no analytics, telemetry, crash reports, usage data, or personal information to
                                     us or anyone else" -> keep "no analytics, telemetry, crash reports or usage data", but scope it: "Apart
                                     from the license activation and re-check above, which carry the details listed there, Yap sends nothing
                                     to us or anyone else." The crash-report-button sentence after it is UNCHANGED.
      site/dist/privacy.html:59 and PRIVACY.md:42   "Your license key is stored in a small file alongside them, and is re-verified from that
                                     file on every read" -> "Your license token is kept in the macOS Keychain (only the random install id is
                                     mirrored in Yap's settings), and it is re-verified on your Mac against a public key built into the app.
                                     Removing the license from a Mac is a button in Settings." (true once yap-lic-02 moves the token to the
                                     Keychain)
      site/dist/index.html:71        "<dt>Network</dt><dd>Model download, update check, refund list</dd>" -> "Model download, update check,
                                     license activation" (there is no refund list after yap-lic-00/02)
      site/dist/index.html:216       "One payment, no subscription, no account, no renewal date." -> drop "no account": "One payment, no
                                     subscription, no renewal date. Buying takes one email-code sign-in; using the app needs no account."
      UNCHANGED (still true at launch; they are about speech and the model, not licensing): index.html:11, :177 and :243 ("no server at
                                     the other end" of transcription), terms.html:35 / TERMS.md:17 (the trial needs no account),
                                     privacy.html:31 / PRIVACY.md:11, privacy.html:69 / PRIVACY.md:51.
      site/dist/terms.html and TERMS.md   Forge-era license terms -> lifetime license, 3 Macs, 60-day offline operation, a full refund or a
                                     lost dispute revokes the license.
      desktop/src-tauri/tauri.conf.json:67   updater endpoints: https://yapvoice.app/updates/latest.json FIRST, yap-lemon.vercel.app second.
    UPDATER PRECONDITION (Panel Infra): site/dist has no updates/ folder today. Before swapping the endpoint order, prove
    curl -s -o /dev/null -w '%{http_code}' https://yapvoice.app/updates/latest.json answers 200 (paste it in the PR body); if it does not,
    add the updates/latest.json route to the site deploy in this item and prove it on a preview, never swap to a 404.`,
  acceptance: `
    ! git grep -qnE 'buy.stripe.com|refunded licenses|refunded-licenses|no login, no server|No sign-in, no email' -- site/dist PRIVACY.md TERMS.md
    ! git grep -qn '<dt>Account</dt><dd>None</dd>' -- site/dist/index.html
    grep -q '<dt>Telemetry</dt><dd>None</dd>' site/dist/index.html
    grep -q 'drivia\\.consulting/yap' site/dist/index.html
    grep -q 'drivia\\.consulting' site/dist/privacy.html
    grep -qi 'Mac' PRIVACY.md
    ! git grep -qinE 'involves no server at all|no device identifier|no account, no sign-in|revocation check|network call the licensing path makes' -- site/dist/privacy.html PRIVACY.md
    ! git grep -qinE 'one-way hash of your email|not the address itself|without holding a list of who bought|none of them ever reach us|stored in a small file|re-verified from that file|crash reports, usage data, or personal information' -- site/dist/privacy.html PRIVACY.md
    ! git grep -qinE 'update check, refund list|no subscription, no account' -- site/dist/index.html
    grep -qi 'Keychain' site/dist/privacy.html
    grep -qi 'Keychain' PRIVACY.md
    grep -qiE 'email[- ]code' site/dist/privacy.html
    grep -qiE 'email[- ]code' PRIVACY.md
    grep -qiE 'Mac(.{1,7}s)? name' site/dist/privacy.html
    grep -qiE 'Mac(.{1,7}s)? name' PRIVACY.md
    grep -qiE 'install(ation)? id' site/dist/privacy.html
    grep -qiE 'install(ation)? id' PRIVACY.md
    grep -qiE 'IP( address)? prefix' site/dist/privacy.html
    grep -qiE 'IP( address)? prefix' PRIVACY.md
    grep -qi '60' site/dist/terms.html
    grep -qi '60' TERMS.md
    test "$(grep -c 'https://yapvoice.app/updates/latest.json' desktop/src-tauri/tauri.conf.json)" = 1
    test "$(curl -s -o /dev/null -w '%{http_code}' https://yapvoice.app/updates/latest.json)" = 200
    (cd desktop && npm run loop:validate)
  `,
})

// ── 09-yap-calls-byom-2026-10-26.mjs ──────────────────────────────────────
// yap-calls-byom-2026-10-26 — CALL DETECTION (every calling app, browser vs desktop), BRING-YOUR-OWN-MODEL for notes and
// polish, and WHERE NOTES LIVE (destinations). Written 2026-10-26T14:43Z (date -u) by the research + spec seat.
// Wilson 2026-10-26: "I hope the calling system can detect phone calls like FaceTimes or Zooms like Read AI can … browser
// calls versus desktop calls: Zoom, Read AI, Google Meet, FaceTime, every calling service … build the API model plugin so
// people can bring their own models to do their note summaries and polishing, and they can connect their internal database
// on their phone or computer for where the data lives and notes live."
//
// PLAN + EVERY CITATION: docs/plans/yap-calls-and-byom-2026-10-26.md (read it first; the section numbers below are its; its
// "Panel revisions 2026-10-26" section at the end supersedes it wherever they differ).
// The CALL items EXTEND yap-notetaker-detect (08 file: meeting_detect.rs, the Record offer, the calendar signal) and DEPEND
// on it; they never rebuild it. The BYOM items sit BEHIND PR 250's NotesBackend trait (summary_jobs.rs) and DEPEND on it.
// yap-res-0 ABSORBS the deferred yap24-X1 (same setting key notes_mirror_dir, so X1's preflight passes and it retires).
// Dependency holds use the harness marker: ONE subshell that prints DEPENDENCY-NOT-ON-MAIN and exits 0 when the earlier
// work is absent from origin/main (template.mjs runBuild; the marker is assembled at run time with printf). Open-PR holds
// use: [ "$(gh pr view N --json state -q .state 2>/dev/null)" = OPEN ] — a gh failure never holds an item, only an OPEN PR does.
//
// PANEL 2026-10-26 (Senior Panel on calls + BYOM + destinations; seat A = Security + macOS, seat B = PM + User + Pre-mortem;
// synthesis = this edit). v1 SET = yap-call-0, yap-call-1, yap-call-2, yap-byom-0, yap-byom-1, yap-res-0.
//   FOLDED   yap-call-4  -> yap-call-0 (scripts/call-walk.sh + docs/loop/CALL-COVERAGE.md + tests/call_coverage_ledger.rs;
//                           the in-call walk is Wilson's checklist in the plan doc, not a loop item). Tombstone below.
//   FOLDED   yap-res-1   -> yap-res-0 (the path classifier: Mobile Documents / CloudStorage / else No). Tombstone below.
//   DEFERRED yap-call-3  -> next release (re-opens OS-5; invisible; macOS 26 only). Stays gated and UNAPPROVED; its spec now
//                           carries the CORRECT tap construction + test so it is right when built.
//   DEFERRED yap-byom-2  -> next release (OpenRouter/Custom already reach Claude). Its redirect/x-api-key rule lives in
//                           yap-byom-0's shared client factory so it is covered the day byom-2 is built.
//   DEFERRED res-0 SQLite destination -> next release (the schema is a contract forever). Folder + webhook ship.
//   DEFERRED call-2 Safari + Firefox path -> next release (Chrome-family ships).
// BLOCKING SET encoded below as real gates: consent.ts promise made conditional (+ CONSENT_PILL_LINE) in call-1 and
// yap-notetaker-detect; offer persists as a chip + accept_call_offer(offer_id) policy test; per-process input listeners
// re-registered on every ProcessObjectList change; --probe-calls never prints raw titles; Keychain account = provider id +
// host; sentinel-key leak test; ONE reqwest factory with redirect::Policy::none() + mock-307 test; prompts live in
// polish_protocol.rs; byom-1 own zero-network test; no silent 1.1 GB download; circuit breaker; Ollama context clamp;
// "-cloud" models label Yes; filename sanitiser; hash-of-last-write; destinations on their own worker with a persisted outbox;
// webhook = timestamp.body HMAC-SHA256 with a stable X-Yap-Delivery id and a 300 s replay window.
// REBASE NOTES (each item's preflight holds on the open PRs it overlaps): PR 250 (summary_jobs.rs) and PR 246 (progress.ts,
// Meetings.tsx) under byom/res; PR 241 (cli.rs) under yap-call-0's --probe-calls; PRs 247 and 239 (permissions.rs) under
// yap-call-2.
//
// EVERY ITEM IS gated:'panel' AND CLASS SEC (capture, permissions, keys or outbound network). Label each PR "security"
// (gh pr edit <N> --add-label security) so the gate seats review-opus AND review-opus2. Review: Opus only.
// HAZARD (same as 07/07b/08/00a): no agent launches the GUI app, posts events into, signals or quits Wilson's running
// /Applications/Yap.app, writes TCC, or places a real call. Automated proof = cargo test / vitest / a script self-test over
// RECORDED FIXTURES; on-device checks go in the PR body as WILSON'S WALK. No acceptance line needs a mic, a TCC grant, a
// network connection beyond 127.0.0.1, or a real API key.
// LEDGER: keep Tauri; never sandbox; no Nango (every connector from scratch); no Screen Recording permission; no private
// API; default notes model stays the bundled 1.5B (PR 250); polish_model stays the bundled-model override; nothing records
// without a click; "your words never leave the Mac unless you point them somewhere" stays TRUE and VISIBLE (every provider
// and destination carries a "Leaves this Mac: Yes/No" label and the label must be TRUE); cloud polish is allowed (label Yes)
// but off by default; a call ending only PROMPTS; destinations = meetings only; capture stays everything-except-Yap by
// default. Polling floor 30 s. Zero users: no backwards-compat burden.
// ACCEPTANCE HYGIENE (from 01): every cargo line carries YAP_DATA_DIR="$(mktemp -d)/yap-state" inline. A "!" absence line
// passes on a crash, so each one is paired with a positive line that proves its target exists (telemetry 2026-10-19).
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap-call-0', prompt: 'yap-calls', branch: 'loop/yap-call-0-signal-matrix-and-fixtures', gated: 'panel',
  title: 'Call signal matrix: per-process mic-in-use + bundle id from CoreAudio, a call-app catalog, one fused confidence score, recorded fixtures, a --probe-calls CLI that never prints a raw title, and the walk script + coverage ledger',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/meeting_detect.rs ) || [ "$(gh pr view 241 --json state -q .state 2>/dev/null)" = OPEN ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-notetaker-detect (meeting_detect.rs) has not landed, or PR 241 (it changes cli.rs, where --probe-calls goes) is still open; land it, then build this"; exit 0; fi; test -f desktop/src-tauri/src/call_signals.rs && test -f desktop/src-tauri/src/call_catalog.json && test -f desktop/src-tauri/tests/call_signal_matrix.rs && test -f desktop/src-tauri/tests/call_coverage_ledger.rs && test -x scripts/call-walk.sh && grep -q 'kAudioProcessPropertyIsRunningInput' desktop/src-tauri/src/syscapture.rs )
  `,
  spec: `
    Panel: APPROVED 2026-10-26 (new OS surface: per-process audio state of OTHER apps). Class SEC — label the PR "security".
    DEPENDS: yap-notetaker-detect (owns meeting_detect.rs, the offer and the calendar signal; its app/browser signal (b) is
    STRUCK there and OWNED HERE and by yap-call-2). REBASE: PR 241 changes cli.rs (hold in the preflight).
    ABSORBS the cut yap-call-4: scripts/call-walk.sh, docs/loop/CALL-COVERAGE.md, tests/call_coverage_ledger.rs.

    WHY (plan §2.2, measured on this Mac, macOS 26.6.2): audio is played and captured by HELPER processes — the CoreAudio
    process object for Chrome is com.google.Chrome.helper (parent pid -> Google Chrome), Safari's is com.apple.WebKit.GPU
    (parent pid 1, launchd), FaceTime/phone run through com.apple.avconferenced / callservicesd (com.apple.TelephonyUtilities).
    The UI pid produces no audio for any browser (syscapture.rs:13-25, finding OS-5). kAudioDevicePropertyDeviceIsRunningSomewhere
    (AudioHardware.h:1015 on this SDK, not :918) is useless while Yap holds the mic. The industry signal is "an app started
    using the microphone" (Circleback, Granola auto-stop — plan §2.1). Yap's own state is NOT a constant: a live probe shows Yap
    running with in=0 when idle; "Yap in=1" holds only while its mic is warm, so the idle-yap-holds-mic fixture is ONE state,
    not the baseline (fixture idle-yap-quiet is the other).

    DO
      1. syscapture.rs: generalise process_output_activity (syscapture.rs:3338) into process_audio_activity() returning
         ProcAudio { pid, parent_pid, bundle_id: Option<String>, running_input, running_output } — add
         kAudioProcessPropertyIsRunningInput and kAudioProcessPropertyBundleID (CFString; release it) per object. Keep the
         existing output-only callers working (a thin adapter), keep "unreadable = skipped, never evidence". Scope the
         comment at syscapture.rs:28-29 ("no ProcessObjectList enumeration, no re-enumeration listener") to the TAP TARGET in
         the same PR so it stops contradicting this item.
      2. call_catalog.json (bundled, include_str!): one row per call app — bundle_id, display name, kind (native|browser|
         system_daemon), helper patterns, title patterns (browsers only), verified: true|false. Seed with plan §2.2(4); rows
         not verified on a real install ship verified:false (a Wilson walk flips them).
      3. call_signals.rs: CallSnapshot (plain serde data: procs, running_apps, frontmost, browser_titles: Option<…> where
         None = Accessibility not granted, ax_source: "yap" | "terminal" | "unknown", calendar: Option<…>), attribution
         (catalog bundle -> helper via parent_pid -> WebKit.GPU ONLY with a Safari window-title match (it has ppid 1, so
         "Yap's children" can never remove it) -> avconferenced/callservicesd while FaceTime/Phone runs), and
         score(snapshot, catalog, memory) -> Option<CallCandidate { bundle_id, name, confidence, via }> with the table in
         plan §3 (offer at >= 60; browser input with AX granted and no title match caps at 55). Yap's own pid and its children
         are removed BEFORE scoring. meeting_detect.rs calls score() — there is no second, UI-pid app signal anywhere.
         Also here, pure and tested: offer_copy(candidate, catalog) -> the offer text: "<App> call — record it?" ("Zoom call —
         record it?"), and for kind system_daemon with verified:false (com.apple.FaceTime, com.apple.mobilephone) "FaceTime
         call — record your side?" — the far side of FaceTime/Phone is NOT measured, so the default copy never claims it.
      4. Event-driven cadence (macOS 14.4+, same gate as the tap). A ProcessObjectList listener
         (kAudioHardwarePropertyProcessObjectList) fires only when processes JOIN or LEAVE — Zoom and the Chrome helper are
         already HAL clients, so a call starting changes only kAudioProcessPropertyIsRunningInput. Therefore: on EVERY
         ProcessObjectList change, (re)register an AudioObjectAddPropertyListener for kAudioProcessPropertyIsRunningInput on
         each process object, reusing the listener pattern in input_format.rs:915-1081 (pure fn process_listener_diff(prev,
         next) -> { add, remove } is the tested seam). Plus the existing NSWorkspace notifications and the ONE 30 s timer as
         the BACKSTOP, never faster. The timer is OFF when Settings > Meetings "Offer to record" is off (pure fn
         timer_enabled(offer_enabled)). While recording, "call ended" reuses the 1 s meeting tick (meeting_control.rs:57): no
         new timer. Budget (measured, one property read = 112 µs): a snapshot is about 4x33 reads = 15 ms, so a 30 s backstop
         is about 0.05 % of a core.
      5. CLI: wilson-voice --probe-calls prints the current CallSnapshot as JSON (cli.rs, next to --asr-capabilities) and
         exits; it reads, never records. Window titles are PRIVATE (mail subjects, meeting codes): the JSON carries each
         title as { pattern_id: <catalog id or null>, len } and NEVER the text; a --raw-titles flag exists for a developer
         at a terminal and scripts/call-walk.sh NEVER passes it. tests/call_signal_matrix.rs asserts that no fixture has a
         "title" string field. The JSON also records ax_source: a CLI run from Terminal is attributed by TCC to Terminal
         (the responsible process), so it reports TERMINAL's AXIsProcessTrusted, not Yap's — ax_source says so.
      6. Fixtures: desktop/src-tauri/tests/fixtures/call_snapshots/*.json — at least: idle-yap-holds-mic (the measured
         snapshot, plan §2.2), idle-yap-quiet (Yap in=0), zoom-in-call, chrome-meet-in-call, chrome-youtube-playing (output
         only -> no offer), safari-webkit-input-no-title (-> no offer), facetime-daemon-input (-> offer text "FaceTime call
         — record your side?"), yap-dictating-only (-> no offer). Hand-built fixtures say so in a "synthetic": true field
         until a Wilson walk replaces them with recorded ones.
      7. scripts/call-walk.sh (executable, bash -n clean; folded from the cut yap-call-4): prints the walk steps from the
         plan's "Wilson's walk checklist" and runs --probe-calls into tests/fixtures/call_snapshots/<app>.json. It prints
         WHICH Accessibility grant it saw ("Terminal's, not Yap's" when ax_source is terminal); a Terminal-AX run records
         browser_titles as null = "AX unknown" and NEVER flips a browser row to verified. It never passes --raw-titles, and
         it refuses to write a snapshot that contains a title string.
      8. docs/loop/CALL-COVERAGE.md: one row per catalog entry — app, bundle id (verified with defaults read
         <app>/Contents/Info CFBundleIdentifier, or "UNVERIFIED: not installed"), which process carries the call's input
         (from a snapshot taken DURING a real call), detected? (Y/N), offer text, track-1 non-zero? (from the meeting
         diagnostics row), notes. Rows: Zoom (us.zoom.xos), Teams (com.microsoft.teams2), FaceTime (com.apple.FaceTime, audio
         via com.apple.avconferenced), Phone / iPhone calls on Mac (com.apple.mobilephone, via callservicesd), iPhone
         Mirroring (com.apple.ScreenContinuity), Slack, Discord, WhatsApp, Webex, Chrome-family Meet. The builder fills what it
         can verify on disk; every in-call column stays UNVERIFIED until Wilson's walk. FaceTime and Phone rows default to
         "your side only" until a walk shows a non-zero track 1 — never a two-sided claim without it.
      9. tests/call_coverage_ledger.rs: every catalog bundle id has a ledger row; every verified:true row has a non-synthetic
         fixture; a browser row is verified only from a fixture whose ax_source is "yap"; every fixture scores as its row says;
         every system_daemon row with verified:false yields the "your side" copy.

    TESTS: tests/call_signal_matrix.rs — table over every fixture -> expected (bundle_id, offer yes/no, via, offer text), plus
    "no fixture has a title string field". Unit tests: self-exclusion (Yap in=1 never scores), helper attribution via
    parent_pid, Safari capped without a title, output-only never offers, an unreadable property never raises confidence,
    process_listener_diff re-registers on every list change, timer_enabled(false) = off.
    WILSON'S WALK (PR body, UNVERIFIED until run): a 10-minute check that the HAL actually notifies on
    kAudioProcessPropertyIsRunningInput for a process that is ALREADY a HAL client (start a Zoom call, watch the listener log).
    If it does not, worst-case detection latency is the 30 s backstop and the 10 s offer still works.
    WHAT NOT TO DO: no Screen Recording, no CGWindowList, no private API (no responsibility_get_pid_responsible_for_pid), no
    polling under 30 s, no recording, no network, no raw title in any file, log or CLI output by default.
    RUNTIME DEPENDENCIES: | CoreAudio process objects, NSWorkspace | OS | — nothing found on the machine.
  `,
  acceptance: `
    test -f desktop/src-tauri/src/call_signals.rs
    test -f desktop/src-tauri/src/call_catalog.json
    grep -q 'kAudioProcessPropertyIsRunningInput' desktop/src-tauri/src/syscapture.rs
    grep -q 'kAudioProcessPropertyBundleID' desktop/src-tauri/src/syscapture.rs
    grep -rq 'kAudioHardwarePropertyProcessObjectList' desktop/src-tauri/src
    grep -rq 'process_listener_diff' desktop/src-tauri/src
    grep -rq 'timer_enabled' desktop/src-tauri/src
    grep -q 'probe-calls' desktop/src-tauri/src/cli.rs
    grep -q 'raw-titles' desktop/src-tauri/src/cli.rs
    test "$(ls desktop/src-tauri/tests/fixtures/call_snapshots/*.json | wc -l)" -ge 8
    ! grep -rlE '"title"' desktop/src-tauri/tests/fixtures/call_snapshots
    grep -q 'call_signals::' desktop/src-tauri/src/meeting_detect.rs
    grep -q 'your side' desktop/src-tauri/src/call_signals.rs
    ! grep -rnE 'CGWindowListCopyWindowInfo|responsibility_get_pid_responsible_for_pid' desktop/src-tauri/src
    ! grep -rn "auto_record\\|autoRecord" desktop/src desktop/src-tauri/src
    test -f docs/loop/CALL-COVERAGE.md
    grep -q 'com.apple.FaceTime' docs/loop/CALL-COVERAGE.md
    grep -q 'com.apple.mobilephone' docs/loop/CALL-COVERAGE.md
    grep -q 'us.zoom.xos' docs/loop/CALL-COVERAGE.md
    test -x scripts/call-walk.sh
    bash -n scripts/call-walk.sh
    grep -q 'probe-calls' scripts/call-walk.sh
    grep -q 'AX unknown' scripts/call-walk.sh
    ! grep -q -- '--raw-titles' scripts/call-walk.sh
    test -f desktop/src-tauri/tests/call_coverage_ledger.rs
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test call_signal_matrix ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test call_coverage_ledger ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol call_signals ; test $? -eq 0
    cd ${APP}/src-tauri && cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap-call-1', prompt: 'yap-calls', branch: 'loop/yap-call-1-call-detected-record-prompt', gated: 'panel',
  title: 'The pill says "Zoom call — record it?" (or "FaceTime call — record your side?") with a true consent line, offers once and then stays as a one-click chip for the life of the call, Offer / Never per app, and asks "Call ended" — it never starts or stops on its own',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/call_signals.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-call-0 has not landed; land it, then build this"; exit 0; fi; test -f desktop/src/pill/CallOffer.tsx && test -f desktop/src-tauri/tests/call_offer_policy.rs && grep -rq 'call_offer_policy_v1' desktop/src-tauri/src && grep -q 'CONSENT_PILL_LINE' desktop/src/pill/CallOffer.tsx )
  `,
  spec: `
    Panel: APPROVED 2026-10-26 (consent surface). Class SEC — label the PR "security".
    DEPENDS: yap-call-0 (CallCandidate, offer_copy), yap-notetaker-detect (the offer this extends; reuse its pill slot, do not
    add a second one; it also owns the CONSENT_PILL_LINE export this item renders).

    DO (plan §3 as revised)
      1. Offer copy comes from offer_copy() (yap-call-0): "Zoom call — record it?"; "FaceTime call — record your side?" while
         that row is unverified; "Browser call — record it?" with the site line "site unknown" when no title matched. Under it
         ONE consent line: CONSENT_PILL_LINE, exported from desktop/src/meetings/consent.ts, text "Let them know you're
         recording." (the three-paragraph CONSENT_NOTICE is too long for a pill). Buttons [Record] [Not now].
      2. consent.ts:68 is FALSE for this feature today: "Yap records your Mac's microphone … Nothing is uploaded, and no one
         else can read it." With a call, other people's audio is recorded; and it is false again whenever any provider or
         destination is Leaves-this-Mac: Yes. Replace the copy with a function consentNotice(activeLeaves: string[]) that
         (a) names what is recorded — "your microphone and the call audio playing on this Mac", or "your side only" for the
         system_daemon rows — and (b) renders the sentence "Nothing is uploaded" ONLY when activeLeaves is empty, otherwise
         it LISTS the hosts ("Notes go to openrouter.ai"). yap-byom-0 and yap-res-0 feed activeLeaves from their labels.
         vitest consent.test.ts: empty -> "Nothing is uploaded" present; ["openrouter.ai"] -> absent, host named.
      3. Record is the Tauri command accept_call_offer(offer_id): it starts only if offer_id is the LIVE candidate's id (the
         backend checks; the webview cannot start a recording for a stale or invented id). It starts kind Virtual and stores
         detected_app (bundle id) and detected_via (mic_in_use|title|calendar|manual) on the meeting row (one additive,
         nullable migration). tests/call_offer_policy.rs proves NO transition except UserChoice::Record yields a
         StartRecording effect (expiry, withdraw, offer, never, ended).
      4. 10 s countdown ring. Expiry == Not now for THIS call and NEVER means Record — but it must not make the offer vanish
         (a founder in a green room or full screen would miss it): after expiry the pill keeps ONE one-click chip "Record
         <App> call" (meeting.ts recordLabel) for as long as that candidate lives, no sound, no second banner. The chip calls
         accept_call_offer with the same offer_id; it disappears the moment the candidate does. A second call by the same app
         after running_input dropped is a NEW candidate (re-arms).
      5. Per-app memory: settings_kv call_offer_policy_v1 = { bundle_id: "offer" | "never" } (default offer). Exactly two
         states — "always_offer" and "ask" are collapsed (two "offer" states confuse). "Never for <App>" in the expanded pill
         and in Settings > Meetings (a list with Reset). There is no auto-record setting anywhere.
      6. Call ended: while recording a detected call, the detected app's running_input false for 20 s (read off the 1 s
         meeting tick, no new timer) -> pill shows "Call ended — stop recording?" [Stop] [Keep going]. Nothing stops by
         itself (plan open question 2: if Wilson picks auto-stop, that is a follow-up).
      7. The pill never lies: a recording that started from an offer shows the app name in the meeting badge
         (desktop/src/pill/MeetingBadge.tsx); FaceTime/Phone recordings say "your side" there too.
      8. CONTENT PROTECTION (Wilson 2026-10-26): the call-offer chip is hidden from screen capture and screen share —
         the offer surface's NSWindow sharingType = none, via Tauri \`set_content_protected(true)\` on THAT window only.
         The recording dot / pill recording state is NEVER content-protected. If the offer renders inside the pill
         window today, it moves to its own small window (label "call-offer") so protecting it cannot hide the dot.
         One pure fn decides it, \`content_protected_for(label: &str) -> bool\` (true only for "call-offer"), called
         where each window is built; nothing else calls set_content_protected.

    TESTS: tests/call_offer_policy.rs (table: offer -> offer with timer; never -> no offer; expiry -> chip, no recording;
    chip click -> Record with the live id; stale id -> rejected; candidate gone -> offer and chip withdrawn; ended 20 s -> stop
    prompt, no stop; ONLY UserChoice::Record yields StartRecording). vitest: desktop/src/pill/callOffer.test.ts (CONSENT_PILL_LINE
    rendered, countdown 10 s, the chip is present after 10 s, Never persists, FaceTime copy) and consent.test.ts.
    CONTENT PROTECTION TEST (tests/call_offer_policy.rs, no GUI launch): the offer window reports content-protected
    true (content_protected_for("call-offer") == true, and the offer window builder passes it to set_content_protected);
    the pill / recording-dot window reports false (content_protected_for("pill") == false, and the pill window is never
    built with content protection).
    WHAT NOT TO DO: no auto-record, no auto-stop, no sound/notification spam (one offer per call), no polling < 30 s.
    DECIDED (was OWNER-ONLY): Wilson 2026-10-26 — hide the OFFER from screen capture/share (DO 8), never the recording dot.
    WILSON'S WALK (PR body): start a full-screen share in Zoom/Meet, trigger an offer — it is absent from the shared
    view and visible on his screen; start recording — the recording dot IS visible in the shared view.
    Panel revisions: 2026-10-26 Wilson: hide the offer only, never the recording dot.
  `,
  acceptance: `
    test -f desktop/src/pill/CallOffer.tsx
    test -f desktop/src/pill/callOffer.test.ts
    test -f desktop/src/meetings/consent.test.ts
    test -f desktop/src-tauri/tests/call_offer_policy.rs
    grep -rq 'call_offer_policy_v1' desktop/src-tauri/src
    grep -q 'Call ended' desktop/src/pill/CallOffer.tsx
    ! grep -rn "auto_record\\|autoRecord\\|auto_stop\\|autoStop" desktop/src desktop/src-tauri/src
    grep -q 'export const CONSENT_PILL_LINE' desktop/src/meetings/consent.ts
    grep -q 'CONSENT_PILL_LINE' desktop/src/pill/CallOffer.tsx
    grep -q 'consentNotice' desktop/src/meetings/consent.ts
    grep -rq 'accept_call_offer' desktop/src-tauri/src/lib.rs
    sed -n '/generate_handler!\\[/,/\\]/p' desktop/src-tauri/src/lib.rs | grep -q 'accept_call_offer'
    grep -rq 'fn content_protected_for' desktop/src-tauri/src
    grep -rq 'set_content_protected' desktop/src-tauri/src
    test "$(grep -rn 'set_content_protected(true)' desktop/src-tauri/src | wc -l | tr -d ' ')" -le 1
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test call_offer_policy ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'yap-call-2', prompt: 'yap-calls', branch: 'loop/yap-call-2-browser-calls-by-ax-title', gated: 'panel',
  title: 'Browser calls in Chrome-family browsers (Chrome, Arc, Edge, Brave): Meet, Zoom web, Teams web, Slack huddles, Discord, Whereby, Webex — matched by the front tab\'s AX window title only when the browser holds the mic, with permission truth when Accessibility is off',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/call_signals.rs ) || [ "$(gh pr view 247 --json state -q .state 2>/dev/null)" = OPEN ] || [ "$(gh pr view 239 --json state -q .state 2>/dev/null)" = OPEN ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-call-0 has not landed, or PR 247 / PR 239 (both change permissions.rs) are still open; land them, then build this"; exit 0; fi; test -f desktop/src-tauri/src/call_titles.rs && test -f desktop/src-tauri/tests/call_browser_titles.rs )
  `,
  spec: `
    Panel: APPROVED 2026-10-26 for the Chrome family (reads other apps' window titles via Accessibility). Class SEC — label the PR "security".
    DEPENDS: yap-call-0. REBASE: PRs 247 and 239 change permissions.rs (hold in the preflight).
    DEFERRED to the next release: the Safari path (com.apple.WebKit.GPU attribution rests on an unproven title+running rule)
    and Firefox. Until then a Safari/WebKit.GPU input NEVER produces an offer (the safari-webkit-input-no-title fixture stays
    green) and docs/loop/DEFERRED.md gets one line starting "yap-call-2 Safari + Firefox:".

    DO (plan §2.2(3), §3)
      1. call_titles.rs: for a Chromium-family browser that the snapshot attributes running_input to, read AXWindows ->
         kAXTitleAttribute via AXUIElementCreateApplication(pid) (focus.rs already reads AXTitle at :313 — reuse attr_string;
         focus.rs:278 already reads other apps through AX, so this needs NO Screen Recording). Titles only; no AXURL, no
         web-area traversal, no AppleScript/Automation permission, no CGWindowList. Read once per candidate, never on a timer
         of its own; titles are matched in memory and NEVER stored or logged (log the matched pattern id and the length only).
      2. Patterns live in call_catalog.json (yap-call-0) per service, case-insensitive: Meet ("Meet - ", "meet.google.com"),
         Zoom web ("Zoom"), Teams web ("Microsoft Teams", "teams.microsoft.com"), Slack ("Huddle"), Discord ("Discord"),
         Whereby, Webex, Around. Every pattern carries verified:false until a Wilson walk records a real title.
      3. Permission truth: browser_titles = None when AXIsProcessTrusted() is false -> the offer reads "Browser call (site
         unknown)" and only fires with a calendar match (plan §3); Settings > Meetings shows one line "Turn on Accessibility
         so Yap can tell which site the call is on" linking the existing permissions flow. Never prompt for Accessibility
         from here.
      4. Honest limit, stated in Settings: Chrome's window title is the FRONT tab, so a Meet in a background tab gets no title
         match and no offer. That is the safe failure. Settings > Meetings says "Yap reads the front tab's title; a call in a
         background tab is not offered".

    TESTS: tests/call_browser_titles.rs — fixtures (title lists) -> matched service; AX None -> "site unknown" path; YouTube
    tab with mic open -> no offer; a Meet title in a background tab (not the front title) -> no offer; Safari WebKit.GPU input
    -> no offer; a title is never written to the DB or log (grep the log sink in the test; only pattern id + len appear).
  `,
  acceptance: `
    test -f desktop/src-tauri/src/call_titles.rs
    test -f desktop/src-tauri/tests/call_browser_titles.rs
    ! grep -rnE 'CGWindowListCopyWindowInfo|NSAppleScript|AXURL' desktop/src-tauri/src/call_titles.rs
    grep -q 'site unknown' desktop/src-tauri/src/call_titles.rs desktop/src/pill/*.tsx
    grep -rq 'front tab' desktop/src/views desktop/src/settings
    grep -q 'yap-call-2 Safari' docs/loop/DEFERRED.md
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test call_browser_titles ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test call_signal_matrix ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'yap-call-3', prompt: 'yap-calls', branch: 'loop/yap-call-3-per-app-tap-spike-macos26', gated: 'panel',
  title: 'DEFERRED to the next release (UNAPPROVED): SPIKE on macOS 26 — capture only the call app (CATapDescription.bundleIDs, exclusive = false) behind a default-OFF setting, or record NO-GO with evidence and keep global-exclude-self',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/call_signals.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-call-0 has not landed; land it, then build this"; exit 0; fi; head -1 docs/loop/evidence/yap-call-3.md | grep -qE '^SPIKE-VERDICT yap-call-3 (GO|NO-GO)$' && test -f desktop/src-tauri/tests/tap_app_scoped_plan.rs )
  `,
  spec: `
    Panel 2026-10-26: DEFERRED to the next release and NOT in panelApproved. Seat B: cut until after 100 users (it re-opens the
    settled OS-5 decision, is macOS-26-only, default OFF and NO-GO "until Wilson walks it", so no user can see it pass). Seat A:
    keep, with the inverted-tap fix below. SETTLED: defer, but keep this spec CORRECT so it is right when it is built. The
    setting stays default OFF; global-exclude-self stays the default on every OS.
    Panel: REQUIRED — this re-opens the panel's OS-5 decision (syscapture.rs:13-31: global-exclude-self only, never
    app-scoped). Class SEC — label the PR "security".
    DEPENDS: yap-call-0 (which app is in the call).

    WHY: macOS 26 adds CATapDescription.bundleIDs ("the bundle ID of a process to tap or exclude") and processRestoreEnabled
    ("save tapped processes by bundle ID when they exit, and restore them … when they start up again"), both
    API_AVAILABLE(macos(26.0)) (SDK CATapDescription.h:131-167). That may answer OS-5's helper-process problem (the UI pid
    is silent) without pid resolution — Spotify and chimes would stop landing in track 1.

    THE INVERSION TRAP (seat A, grounded in CATapDescription.h): "exclusive" means "tap all processes EXCEPT the process
    listed", and bundleIDs means "tap or exclude". Setting bundleIDs on today's initMonoGlobalTapButExcludeProcesses
    description would therefore tap EVERYTHING EXCEPT the call app — the exact opposite.

    DO
      1. Pure plan: TrackBPlan gains AppScoped { bundle_ids } chosen ONLY when: macOS >= 26.0, setting
         meeting_app_scoped_tap (default OFF) is on, and a CallCandidate exists. tests/tap_app_scoped_plan.rs proves every
         other combination returns today's plan unchanged.
      2. Construction (correct form): build the AppScoped description with initMonoMixdownOfProcesses:@[] and THEN
         setExclusive(false) and THEN setBundleIDs([the app's bundle id + its catalog helper ids]); setProcessRestoreEnabled(true).
         Global keeps initMonoGlobalTapButExcludeProcesses with exclusive == true. Resolve the setters via objc2 msg_send
         with respondsToSelector so the binary still loads on 14.4 (objc2-core-audio 0.3.2 has setBundleIDs/
         setProcessRestoreEnabled at AudioHardware.rs:2007/2055; same weak-link rule as YV101). tests/tap_app_scoped_plan.rs
         asserts exclusive == false for AppScoped and exclusive == true for Global.
      3. EXCLUDE Safari from app-scoping: the com.apple.WebKit.GPU bundle id taps every WKWebView process, Yap's own webview
         included — a Safari call stays on the global plan.
      4. The silence discriminator must survive: TapLiveness::ever_nonzero (syscapture.rs:196) still separates denial from
         wedge; an app-scoped tap that never delivers a non-zero sample within the existing grace falls back to the global
         plan for that meeting and logs why. Never two taps at once.
      5. Evidence: docs/loop/evidence/yap-call-3.md, FIRST LINE exactly "SPIKE-VERDICT yap-call-3 GO" or
         "SPIKE-VERDICT yap-call-3 NO-GO", then WILSON'S WALK results (Chrome Meet, Zoom, a YouTube tab playing at the same
         time): per-track RMS from the meeting diagnostics row, with and without the setting. Until Wilson walks it, the
         verdict is NO-GO and the setting stays hidden.

    WHAT NOT TO DO: no default change, no ScreenCaptureKit, no second concurrent tap, no change below macOS 26, no bundleIDs
    on an exclusive description.
  `,
  acceptance: `
    head -1 docs/loop/evidence/yap-call-3.md | grep -qE '^SPIKE-VERDICT yap-call-3 (GO|NO-GO)$'
    test -f desktop/src-tauri/tests/tap_app_scoped_plan.rs
    grep -q 'meeting_app_scoped_tap' desktop/src-tauri/src/syscapture.rs desktop/src-tauri/src/meeting_control.rs
    grep -q 'initMonoGlobalTapButExcludeProcesses' desktop/src-tauri/src/syscapture.rs
    grep -q 'initMonoMixdownOfProcesses' desktop/src-tauri/src/syscapture.rs
    grep -q 'setExclusive' desktop/src-tauri/src/syscapture.rs
    grep -q 'exclusive' desktop/src-tauri/tests/tap_app_scoped_plan.rs
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test tap_app_scoped_plan ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_availability_144_gate ; test $? -eq 0
    bash scripts/assert-weak-linked-14_4-symbols.sh
  `,
})

// TOMBSTONE yap-call-4 (panel 2026-10-26, FOLDED, not dropped): its deliverables moved into yap-call-0 — scripts/call-walk.sh
// (no raw titles, prints which AX grant it saw, never verifies a browser row on a Terminal-AX run), docs/loop/CALL-COVERAGE.md,
// tests/call_coverage_ledger.rs, and the FaceTime/Phone "your side only" default copy. The in-call walk is now WILSON'S WALK CHECKLIST,
// a section in docs/plans/yap-calls-and-byom-2026-10-26.md. Reason (seat B): its acceptance passed with every row UNVERIFIED, so
// it proved nothing as a loop item; the machinery is cheap, the walk is a human act.

ITEMS.push({
  id: 'yap-byom-0', prompt: 'yap-byom', branch: 'loop/yap-byom-0-provider-model-keychain-settings', gated: 'panel',
  title: 'Bring your own model, the frame: a provider model (Bundled | OpenAI-compatible), per-task choice for notes and polish, API keys in the macOS Keychain bound to their host, one redirect-free HTTP client factory, Test connection, and a "Leaves this Mac" label on every choice that is TRUE',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/summary_jobs.rs ) || [ "$(gh pr view 246 --json state -q .state 2>/dev/null)" = OPEN ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 250 (default meeting notes, summary_jobs.rs) has not landed, or PR 246 (Meetings UI: progress.ts, Meetings.tsx) is still open; land them, then build this"; exit 0; fi; test -f desktop/src-tauri/src/model_providers.rs && test -f desktop/src-tauri/src/keychain.rs && test -f desktop/src-tauri/src/http_client.rs && test -f desktop/src-tauri/tests/model_providers_contract.rs && test -f desktop/src-tauri/tests/key_sentinel_leak.rs && test -f desktop/src/settings/ModelProviders.tsx )
  `,
  spec: `
    Panel: APPROVED 2026-10-26 (secrets + the product's privacy promise). Class SEC — label the PR "security".
    DEPENDS: PR 250 (summary_jobs.rs NotesBackend; the bundled 1.5B notes model stays the default). REBASE: PR 250 touches
    lib.rs/meetings.rs/NotesView; PR 246 touches progress.ts and Meetings.tsx (hold in the preflight).

    DO (plan §2.3, §5 as revised)
      1. model_providers.rs: ProviderKind { Bundled, OpenAiCompatible } (the Anthropic kind arrives with the deferred
         yap-byom-2; the UI HIDES any kind that is not wired — no "Coming in this release" placeholder, a release can break that
         promise), ProviderConfig { id, kind, label, base_url, model, context_tokens (default 4096), timeout_s }, trait
         ChatProvider { complete(system, user, max_tokens, cancel) -> Result<String, ProviderError>; leaves_this_mac() -> bool }.
         leaves_this_mac is false for Bundled and for a base_url whose host is 127.0.0.1 / ::1 / localhost, true otherwise —
         EXCEPT a loopback model name containing "-cloud" or ":cloud" (Ollama cloud models run on ollama.com THROUGH localhost,
         docs.ollama.com/cloud): those are true and the label reads "Yes — ollama.com". ProviderError has plain-words reasons
         (unreachable, timeout, 401 key rejected, 404 model not found, 429 rate limited, 5xx server error, redirected to
         <host> — not followed, bad response).
      2. Settings: providers list in settings_kv model_providers_v1 (NO key in it); notes_provider and polish_provider ids
         (empty = Bundled). polish_model keeps its meaning (the bundled-model override). Bundled is always present and
         cannot be deleted.
      3. keychain.rs: security-framework passwords API (already in desktop/Cargo.lock 3.7.0 via native-tls; declare it
         directly). Service name is derived from the RUNNING bundle id, com.wilsonguenther.wilson-voice.byom for the shipped
         app and the Yap Test bundle's own for the test channel (same rule as PR 248), so a test build never reads or deletes
         the real keys. Account = provider id + host. Changing a provider's base_url HOST deletes the key and requires
         re-entry. set / get / delete. Keys never reach SQLite, settings, logs, crash reports or the frontend after save (the UI
         shows "Key saved" + last 4 chars). Deleting a provider deletes its Keychain item.
      4. http_client.rs: the ONE reqwest client factory every outbound BYOM and destination call goes through (byom-1, the
         deferred byom-2, and the res-0 webhook). redirect::Policy::none() — reqwest 0.13.5 defaults to limited(10)
         (redirect.rs:161) and strips only Authorization/Cookie across hosts (:239-249), so a 307/308 would re-POST the
         transcript to an unlabelled host and forward x-api-key with it. A 3xx becomes ProviderError/DestError "redirected to
         <host> — not followed". One keep-alive Client per provider (cold TLS inside a short deadline loses). RULE for any
         later change: nothing may follow a redirect; if one ever does it must strip x-api-key, api-key, Authorization and
         anthropic-workspace-id across hosts. This is the outbound allowlist test.
      5. desktop/src/settings/ModelProviders.tsx, reached from views/Settings.tsx: rows with label, kind, model, "Leaves this
         Mac: Yes — <host>" / "No", Test connection, Edit, Delete; two pickers "Notes use" / "Polish uses". A summary line at
         the top when any Yes is active ("Notes go to openrouter.ai"). Copy says plainly what leaves: notes = the meeting
         transcript; polish = every dictation. The active Yes labels feed consentNotice(activeLeaves) in consent.ts (yap-call-1)
         so "Nothing is uploaded" disappears the moment any Yes is active.
      6. Test connection: the Tauri command test_model_provider(provider_id) — it takes the provider ID only, NEVER a URL or key
         from the frontend, so a webview bug cannot aim a stored key at a new host. A 10-token completion (max_tokens 10) with a
         10 s timeout; shows latency and the model's reply or the ProviderError sentence. Registered in generate_handler!.
      7. In this item only Bundled is implemented behind ChatProvider (wrapping the existing sidecar path); OpenAI-compatible is
         wired by yap-byom-1 and is hidden until then — never a fake success.

    TESTS: tests/model_providers_contract.rs — leaves_this_mac table (loopback = No, LAN IP = Yes, https host = Yes, loopback
    with model "gpt-oss:120b-cloud" and "kimi-k2:cloud" = Yes — ollama.com); settings round-trip never contains a key; keychain
    set/get/delete behind a trait with an in-memory fake (no real Keychain write in CI), account = id + host, a host change
    deletes the key, the service name follows the bundle id; Bundled cannot be deleted.
    tests/key_sentinel_leak.rs — save the key "sk-YAPSENTINEL-…" through the fake keychain, run Test connection against the
    127.0.0.1 mock, assert the sentinel is ABSENT from the log sink, settings_kv, every emitted event payload and the command's
    return value (last 4 only). tests/http_client_no_redirect.rs — the mock returns 307 to a SECOND 127.0.0.1 port; that port
    receives ZERO connections and the call ends "redirected to <host> — not followed". vitest ModelProviders (label text,
    pickers, key never rendered beyond last 4, unwired kinds hidden) and consent.test.ts (a Yes label removes "Nothing is uploaded").
    RUNTIME DEPENDENCIES: | Security.framework (Keychain) | OS; security-framework crate ships |.
  `,
  acceptance: `
    test -f desktop/src-tauri/src/model_providers.rs
    test -f desktop/src-tauri/src/keychain.rs
    test -f desktop/src-tauri/src/http_client.rs
    test -f desktop/src/settings/ModelProviders.tsx
    grep -q 'com.wilsonguenther.wilson-voice.byom' desktop/src-tauri/src/keychain.rs
    grep -q 'Policy::none' desktop/src-tauri/src/http_client.rs
    grep -q 'Leaves this Mac' desktop/src/settings/ModelProviders.tsx
    grep -q 'ollama.com' desktop/src-tauri/src/model_providers.rs
    ! grep -rn 'Coming in this release' desktop/src
    sed -n '/generate_handler!\\[/,/\\]/p' desktop/src-tauri/src/lib.rs | grep -q 'test_model_provider'
    test -f desktop/src-tauri/tests/key_sentinel_leak.rs
    grep -q 'YAPSENTINEL' desktop/src-tauri/tests/key_sentinel_leak.rs
    test -f desktop/src-tauri/tests/http_client_no_redirect.rs
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test model_providers_contract ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test key_sentinel_leak ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test http_client_no_redirect ; test $? -eq 0
    cd ${APP}/src-tauri && cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap-byom-1', prompt: 'yap-byom', branch: 'loop/yap-byom-1-openai-compatible-provider', gated: 'panel',
  title: 'OpenAI-compatible provider behind notes and polish: Ollama, LM Studio, OpenRouter and custom presets, "Find local models", a context-safe map/reduce, and a truthful fallback to the built-in model that never starts a silent download',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/model_providers.rs ) || [ "$(gh pr view 246 --json state -q .state 2>/dev/null)" = OPEN ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-byom-0 has not landed, or PR 246 (progress.ts, Meetings.tsx; the meeting card shows which provider wrote the notes) is still open; land them, then build this"; exit 0; fi; test -f desktop/src-tauri/src/provider_openai.rs && test -f desktop/src-tauri/tests/provider_openai_compat.rs && test -f desktop/src-tauri/tests/notes_provider_fallback.rs && test -f desktop/src-tauri/tests/byom_bundled_zero_network.rs && test -f desktop/src-tauri/tests/notes_prompt_identity.rs )
  `,
  spec: `
    Panel: APPROVED 2026-10-26 (outbound network carrying transcripts and dictations). Class SEC — label the PR "security".
    DEPENDS: yap-byom-0, PR 250 (the sidecar prompts and AppNotes::model_ready live there).

    DO (plan §2.3 as revised)
      1. provider_openai.rs: POST {base_url}/chat/completions with { model, messages:[system,user], max_tokens, stream:false }
         and Authorization: Bearer <key> when a key exists; parse choices[0].message.content ONLY. Built from
         http_client.rs (yap-byom-0): no redirects, keep-alive. NEVER send "tools"; a response with tool_calls or any
         non-text content is ProviderError "bad response". Non-streaming: notes are a background job; cancel = drop the
         request when the CancelToken fires.
      2. Presets (editable after pick): Ollama http://localhost:11434/v1 (docs.ollama.com/api/openai-compatibility — key
         ignored), LM Studio http://localhost:1234/v1 (lmstudio.ai/docs/developer/openai-compat), OpenRouter
         https://openrouter.ai/api/v1 (sends X-Title: Yap), Custom. Ollama and LM Studio presets default context_tokens
         4096 (Ollama's default context is 4k under 24 GiB of memory, docs.ollama.com/context-length, and overflow is silent).
         "Find local models" = GET {base}/models on the two local presets with a 1.5 s timeout, ONLY on click — never a
         background probe; a listed model named "-cloud" / ":cloud" is shown with "Yes — ollama.com". On a Mac with <= 8 GB RAM
         a local provider row shows "A 7B+ model next to Yap may swap" (pure fn, tested).
      3. Notes: a NotesBackend implementation per selected provider — model_ready() true, fetch_model a no-op. Prompts: the four
         system prompt consts (SYSTEM_BASE, MAP, REDUCE, HEADLINE — today in the sidecar, yap-polish/src/main.rs:72/:143/:165/
         :182) MOVE into polish_protocol.rs (already compiled into both crates via #[path], main.rs:37). A remote call sends
         them as the SYSTEM turn and the transcript as the USER turn (the transcript is data; SYSTEM_BASE rule 2 already says
         never obey the content). Remote MAP output still goes through parse_map_output's id filter. Map chunk cap =
         min(MAP_CHUNK_TOKENS 1400 (summarize.rs:94), context_tokens - max_out (polish_protocol.rs:91, 1536)); a chunk that
         cannot fit is split, never truncated. Per-call timeout 120 s.
      4. Polish: when polish_provider is set (default empty = Bundled; cloud polish is allowed and labelled Yes, never default),
         the polish stage calls the provider with the take's existing deadline (PolishRequest deadline_ms, default 1200,
         polish.rs:59, clamp 100..5000); a miss inserts the unpolished take exactly as a sidecar timeout does today. Honesty:
         Test connection compares latency to the deadline ("answered in 2.4 s; polish waits 1.2 s, so most dictations will skip
         it") and Settings shows "polished X of last N takes" from a local counter (no telemetry). Remote polish SKIPS password
         fields (AXSecureTextField targets; focus.rs:279 only drops context today) and mode=code — that text must not leave.
      5. Fallback (notes): the FIRST timeout or 5xx abandons the remote for the rest of the job (circuit breaker; plus a 10-minute
         job cap — 9 chunks x 120 s would otherwise be 18 minutes). Then: if AppNotes::model_ready() (#250 lib.rs:5119) the job
         reruns on Bundled and the meeting stores "Notes written by the built-in model — <label> didn't answer (<reason>)";
         if NOT, the job parks with "<label> didn't answer; the built-in model isn't downloaded — [Download & retry]". Yap never
         starts the 1.1 GB download on its own. The meeting card shows which provider wrote the notes and its Leaves-this-Mac label.

    TESTS (no internet; a std::net::TcpListener mock on 127.0.0.1 inside the test): tests/provider_openai_compat.rs (request
    shape, no "tools" key, auth header present only with a key, 401/404/429/500/timeout map to the right ProviderError,
    tool_calls -> bad response, a 3xx is not followed, cancel drops the request, a 20k-token transcript to a 4096-context mock
    makes >= 14 map calls and none over budget, a 2048-context mock yields smaller chunks); tests/notes_prompt_identity.rs (the
    four prompts sent remotely are BYTE-IDENTICAL to the sidecar's, system/user framing); tests/notes_provider_fallback.rs
    (provider 500 with model ready -> Bundled stub writes notes + banner; provider 500 with NO model -> parked "Download & retry",
    zero download started; the first timeout opens the breaker); tests/byom_bundled_zero_network.rs (every provider Bundled ->
    notes + polish make ZERO connections, deny-all seam; this replaces the old "if meeting_notes_stay_local exists" line, which
    was vacuous until yap-notetaker-notes landed); remote polish skipped for a secure-field fixture and for mode=code.
  `,
  acceptance: `
    test -f desktop/src-tauri/src/provider_openai.rs
    grep -q 'localhost:11434/v1' desktop/src-tauri/src/provider_openai.rs desktop/src/settings/ModelProviders.tsx
    grep -q 'localhost:1234/v1' desktop/src-tauri/src/provider_openai.rs desktop/src/settings/ModelProviders.tsx
    grep -q 'openrouter.ai/api/v1' desktop/src-tauri/src/provider_openai.rs desktop/src/settings/ModelProviders.tsx
    grep -q 'http_client::' desktop/src-tauri/src/provider_openai.rs
    ! grep -rnE 'reqwest::Client::(new|builder)' desktop/src-tauri/src/provider_openai.rs
    grep -q 'SYSTEM_BASE' desktop/src-tauri/src/polish_protocol.rs
    test -f desktop/src-tauri/tests/notes_prompt_identity.rs
    test -f desktop/src-tauri/tests/byom_bundled_zero_network.rs
    grep -rq 'Download & retry' desktop/src
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test provider_openai_compat ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test notes_prompt_identity ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test notes_provider_fallback ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test byom_bundled_zero_network ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'yap-byom-2', prompt: 'yap-byom', branch: 'loop/yap-byom-2-anthropic-native-provider', gated: 'panel',
  title: 'DEFERRED to the next release (UNAPPROVED): Anthropic provider on the native Messages API, built on the shared redirect-free client with x-api-key never leaving its host, same fallback and labels',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/model_providers.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-byom-0 has not landed; land it, then build this"; exit 0; fi; test -f desktop/src-tauri/src/provider_anthropic.rs && test -f desktop/src-tauri/tests/provider_anthropic.rs )
  `,
  spec: `
    Panel 2026-10-26: DEFERRED to the next release and NOT in panelApproved (seat B: OpenRouter and Custom already reach Claude, a
    second HTTP client is not first-session wow). Seat A's rule is carried so it holds the day this is built: yap-byom-0's
    http_client.rs factory (redirect::Policy::none()) ALREADY covers it.
    Panel: REQUIRED (outbound network). Class SEC — label the PR "security".
    DEPENDS: yap-byom-0 (the factory, keychain, labels) and yap-byom-1's fallback path (share it, do not copy it).

    DO (plan §2.3)
      1. provider_anthropic.rs: POST https://api.anthropic.com/v1/messages with headers x-api-key, anthropic-version (one
         pinned constant, read from https://platform.claude.com/docs/en/api/overview at build time and cited in a comment),
         content-type application/json, and anthropic-workspace-id when the user filled the optional Workspace field. Body
         { model, max_tokens, system, messages:[{role:user}] }; text = concatenated content[].text blocks; any tool_use or
         non-text block -> "bad response". No "tools", no temperature/top_p. The client comes from http_client.rs ONLY: a 3xx is
         never followed, so x-api-key and anthropic-workspace-id cannot cross hosts.
      2. "Load models" = GET /v1/models on click only. Model id is free text with the loaded list as suggestions.
      3. Same NotesBackend + polish wiring, timeouts, circuit breaker, fallback and the Leaves-this-Mac: Yes —
         api.anthropic.com label as yap-byom-1; the Keychain account is provider id + host as in yap-byom-0.
    TESTS: tests/provider_anthropic.rs over a 127.0.0.1 mock: headers present, workspace header only when set, error mapping
    (401, 404, 429 with retry-after, 529/5xx, timeout), multi-block text joined, cancel drops the request, and a mock 307 to a
    second 127.0.0.1 port: that port receives zero connections (so no x-api-key arrives there).
  `,
  acceptance: `
    test -f desktop/src-tauri/src/provider_anthropic.rs
    grep -q 'anthropic-version' desktop/src-tauri/src/provider_anthropic.rs
    grep -q 'api.anthropic.com/v1/messages' desktop/src-tauri/src/provider_anthropic.rs
    grep -q 'http_client::' desktop/src-tauri/src/provider_anthropic.rs
    ! grep -rnE 'reqwest::Client::(new|builder)' desktop/src-tauri/src/provider_anthropic.rs
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test provider_anthropic ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'yap-res-0', prompt: 'yap-residency', branch: 'loop/yap-res-0-destinations-folder-webhook', gated: 'panel',
  title: 'Where your notes live: a Markdown+JSON folder (Obsidian-ready, edit-safe, with a true "Leaves this Mac" label for iCloud, Obsidian-iCloud and CloudStorage folders) and a signed webhook with a persisted outbox — cloud SaaS connectors and the SQLite file are deferred',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/summary_jobs.rs && test -f desktop/src-tauri/src/keychain.rs && test -f desktop/src-tauri/src/http_client.rs ) || [ "$(gh pr view 246 --json state -q .state 2>/dev/null)" = OPEN ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 250 and yap-byom-0 (keychain.rs, http_client.rs) have not landed, or PR 246 (progress.ts, Meetings.tsx; the card badges) is still open; land them, then build this"; exit 0; fi; test -f desktop/src-tauri/src/destinations.rs && test -f desktop/src-tauri/tests/destinations.rs && grep -q 'notes_mirror_dir' desktop/src-tauri/src/lib.rs && grep -q 'Mobile Documents' desktop/src-tauri/src/destinations.rs )
  `,
  spec: `
    Panel: APPROVED 2026-10-26 for the folder and the webhook (writes user data outside the app; outbound webhook). Class SEC —
    label the PR "security".
    DEPENDS: PR 250 (notes + export_meeting_markdown, lib.rs:5330; summary_jobs.rs), yap-byom-0 (keychain.rs for the webhook
    secret; http_client.rs for the redirect-free client). REBASE: PR 250 and PR 246 (hold in the preflight).
    ABSORBS yap24-X1: use the setting key notes_mirror_dir for the folder destination so X1's preflight passes and X1
    retires as already-done. ABSORBS the cut yap-res-1 (the iCloud label, as a path classifier — below).
    DEFERRED to the next release: the SQLite destination (seat B: the schema is a contract forever; seat A's rules are kept for
    when it is built: never set WAL on a user's file — it persists in the file and WAL plus a sync folder corrupts, sqlite.org/wal.html —
    and refuse paths under com~apple~CloudDocs / Mobile Documents). One line in docs/loop/DEFERRED.md.

    DO (plan §2.4 as revised)
      1. destinations.rs: trait Destination { kind, leaves_this_mac(), deliver(&MeetingExport) -> Result<(), DestError> }, driven
         by its OWN single worker thread fed from a PERSISTED outbox (table destination_outbox: id, meeting_id, destination_id,
         notes_revision, delivery_id, attempt, next_at, state; one additive migration). It must NEVER run on the one wv-summary
         worker (summary_jobs.rs:236-260): retries of 5 s / 30 s / 2 min on that thread would park every later meeting's notes
         for about 2.6 minutes. Notes never wait on a delivery; a quit mid-retry resumes from the outbox on next launch. Never
         concurrently with dictation. Per-destination last result on the meeting card ("Saved to Obsidian · Webhook failed: 401").
      2. Folder: <folder>/Yap Meetings/<date> <title>.md (front matter: date, duration, kind, detected_app, attendees if known)
         + a JSON sidecar <same name>.json { id, title, started_at, segments[], summary, actions[] } — schema documented in
         docs/destinations.md. Atomic (temp file + fsync + rename), never deletes a user file, a missing folder pauses with one
         sentence in Settings. The TITLE comes from the model's headline pass, which a transcript can steer ("../../Library/
         LaunchAgents/x"), so: safe_file_stem() strips / \\ : NUL and every control character, collapses leading dots, caps at
         120 BYTES on a char boundary, falls back to "Untitled meeting" when empty; the resolved path is canonicalised and
         asserted to be INSIDE the folder before any write; every YAML front-matter value is double-quoted and escaped.
      3. Edit-safe rewrite: store the sha256 Yap last wrote per (meeting, destination). When notes change, if the on-disk file's
         hash differs (or a file Yap did not write sits at that name) never overwrite — write "<name> (Yap update).md" and show
         on the card "Edited in Obsidian — kept yours".
      4. Privacy label (absorbed from yap-res-1) — a path classifier on the CANONICAL folder path (symlinks resolved): anything
         under ~/Library/Mobile Documents (this includes iCloud Drive's com~apple~CloudDocs AND Obsidian's iCloud vaults,
         iCloud~md~obsidian) -> "Leaves this Mac: Yes — Apple iCloud"; ~/Library/CloudStorage/<Provider>-<account> ->
         "Yes — <Provider>" (the part before the first "-": OneDrive, GoogleDrive, Dropbox, Box); anything else -> "No".
         A preset "iCloud Drive / Yap" at ~/Library/Mobile Documents/com~apple~CloudDocs/Yap/ is shown only when that directory
         exists, otherwise "iCloud Drive is off on this Mac". No entitlement, no CloudKit, never sandboxed. Evicted files
         ("Optimize Mac Storage"): a rewrite writes a fresh file via temp+rename and NEVER reads the evicted one first; a .icloud
         placeholder is never touched or deleted. WILSON'S WALK (PR body, UNVERIFIED): enable it, finish a meeting, open Files
         on the iPhone > iCloud Drive > Yap and see the note; and note whether the first write to iCloud Drive raised a Files &
         Folders permission prompt.
      5. Webhook: POST the same MeetingExport JSON to an https URL (plain http only for 127.0.0.0/8, ::1, localhost), through
         http_client.rs (no redirects: a 3xx is "redirected to <host> — not followed"). Headers: X-Yap-Timestamp (unix
         seconds), X-Yap-Signature: sha256=<hex HMAC-SHA256(secret, timestamp + "." + body)> — this is the Stripe-style
         timestamp.body scheme; it is NOT GitHub's body-only X-Hub-Signature-256, so do not call it that anywhere —
         X-Yap-Delivery: <uuid> (the outbox delivery_id, STABLE across retries of the same delivery; each retry re-signs with a
         FRESH timestamp) and X-Yap-Event-Id: <meeting id>:<notes revision> (so a receiver can dedupe a re-fire on notes edit).
         The secret is 32 random bytes generated by Yap, stored in the Keychain, shown ONCE with Copy. docs/destinations.md
         gives the 300 s replay window and a ten-line verify snippet; a test runs that snippet's logic. 3 retries (5 s, 30 s,
         2 min) from the outbox, 10 s timeout, then "failed" on the card with a Retry; "Send test event". Leaves this Mac:
         Yes — <host> (No for loopback).
      6. Settings > Meetings > "Where notes go": add/remove destinations, each row with its label; meetings only (dictation
         log is a follow-up). The active Yes labels feed consentNotice(activeLeaves) (yap-call-1's consent.ts).
      7. docs/loop/DEFERRED.md: add — Notion, Google Drive, Dropbox, generic Postgres destinations deferred to after the first 100
         users, built from scratch (no Nango); and a line starting "Yap SQLite destination:" deferred to the next release with
         the no-WAL / no-iCloud rules above.
    TESTS: tests/destinations.rs — folder atomic write + rewrite + never-delete; safe_file_stem over "../../x" and
    "a\\n---\\nb" and a 300-byte title; path-inside-folder check; front matter quotes a hostile value; external edit -> "(Yap
    update).md" and the user's bytes unchanged; classifier over THREE real shapes (~/Library/Mobile Documents/com~apple~
    CloudDocs/x, ~/Library/Mobile Documents/iCloud~md~obsidian/Documents/Vault, ~/Library/CloudStorage/OneDrive-Personal/x
    -> Yes iCloud, Yes iCloud, Yes OneDrive; ~/Documents/x -> No; a symlink into iCloud resolves to Yes); the preset hidden
    when the dir is absent (temp HOME); .icloud placeholders untouched; JSON sidecar validates against the documented fields;
    webhook over a 127.0.0.1 mock: signature verifies with the secret and a 300 s-old timestamp is rejected by the snippet,
    retries on 500 with the SAME X-Yap-Delivery and a NEW signature, gives up after 3, a 307 to a second port gets zero
    connections, no secret in logs; a slow webhook does not delay the summary worker (notes finish first); outbox survives a
    restart (resume from the table); leaves_this_mac table.
    RUNTIME DEPENDENCIES: | reqwest, sha2, hmac (new small crate) | ship |. (rusqlite is NOT added: the SQLite destination is deferred.)
  `,
  acceptance: `
    test -f desktop/src-tauri/src/destinations.rs
    test -f docs/destinations.md
    grep -q 'notes_mirror_dir' desktop/src-tauri/src/lib.rs
    grep -q 'X-Yap-Signature' desktop/src-tauri/src/destinations.rs
    grep -q 'X-Yap-Delivery' desktop/src-tauri/src/destinations.rs
    ! grep -rniE 'x-hub-signature|github' desktop/src-tauri/src/destinations.rs docs/destinations.md
    grep -q 'X-Yap-Signature' docs/destinations.md
    grep -q '300' docs/destinations.md
    grep -q 'http_client::' desktop/src-tauri/src/destinations.rs
    grep -q 'safe_file_stem' desktop/src-tauri/src/destinations.rs
    grep -q 'destination_outbox' desktop/src-tauri/src/db.rs
    grep -q 'Mobile Documents' desktop/src-tauri/src/destinations.rs
    grep -q 'CloudStorage' desktop/src-tauri/src/destinations.rs
    grep -q 'Yap update' desktop/src-tauri/src/destinations.rs
    grep -rq 'Apple iCloud' desktop/src
    grep -qi 'notion' docs/loop/DEFERRED.md
    grep -q 'Yap SQLite destination' docs/loop/DEFERRED.md
    ! grep -rniE 'nango' desktop/src-tauri/src desktop/src
    ! grep -rn 'rusqlite' desktop/src-tauri/src/destinations.rs
    cd ${APP} && npm ci && npx tsc --noEmit && npm test
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test destinations ; test $? -eq 0
    cd ${APP}/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib notes_mirror ; test $? -eq 0
    cd ${APP}/src-tauri && cargo clippy --all-targets --features custom-protocol
  `,
})

// TOMBSTONE yap-res-1 (panel 2026-10-26, FOLDED, not dropped): the iCloud Drive mirror is now a preset of yap-res-0's folder
// destination and its label rule is yap-res-0's path classifier, widened by both seats: anything under ~/Library/Mobile Documents
// (CloudDocs AND iCloud~md~obsidian vaults) -> "Yes — Apple iCloud"; ~/Library/CloudStorage/<Provider>-* -> "Yes — <Provider>"; else
// No; .icloud placeholders never touched; tests over all three. Reason: res-0 would have shipped "No" for an iCloud-synced Obsidian
// vault until res-1 landed (a false label). UNVERIFIED: the Files & Folders / iCloud Drive first-write permission prompt (WILSON'S WALK).


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
