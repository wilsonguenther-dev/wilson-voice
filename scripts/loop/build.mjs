/**
 * scripts/loop/build.mjs — stamps the Yap overhaul CI/CD loop out of a template + item files.
 * Ported from the Drivia harness (drivia-hustle/scripts/loop/build.mjs); every validation below is
 * a rule the Workflow tool, or a previous loop's telemetry, has already made us pay for. The Yap
 * additions are at the bottom of section 3: the branch prefix, the interpolation whitelist and the
 * wrong-repo/wrong-script-name bans.
 *
 *     node scripts/loop/build.mjs                 # stamp the parts + the parent
 *     node scripts/loop/build.mjs --validate-only # validate and report; write NOTHING
 *     node scripts/loop/build.mjs --items <dir> --gen <dir> --parent <file>
 *                                                 # stamp from a throwaway item dir, elsewhere
 *
 * INPUT   scripts/loop/template.mjs        the harness: prose, seats, merge bar, drain, reflect
 *         scripts/loop/items/NN-*.mjs      the work: ITEMS.push({...}) statements, filename order
 * OUTPUT  scripts/loop/generated/part-NN.mjs   one PART per consecutive run of whole item files
 *         scripts/cicd-loop-all.mjs            the thin PARENT that runs the parts in order
 *
 * WHY PARTS. The Claude Code Workflow tool refuses any script file over 524288 bytes. The single
 * generated script had grown to 781367. So the template is stamped once per PART with a subset of
 * the items (whole item files only, rollout order preserved), each part is kept under
 * PART_BUDGET_BYTES, and a small parent runs them with `workflow({scriptPath}, args)` — one level
 * of nesting, which is all the runtime allows, so a part may never call workflow() itself.
 * Every part shares ONE worktree: KEEP_WORKTREE is stamped true for every part but the last, and
 * the template's Drain honours it, so only the last part tears the worktree down.
 *
 * Unlike its output, THIS file is a real node module: `node --check` passes on it. The output is
 * a Workflow script (top-level await/return, `export const meta` as a pure literal first
 * statement) and `node --check` on THAT is meaningless — which is exactly why the validation
 * below exists. Every check here is a rule the Workflow tool, or a previous loop's telemetry,
 * has already made us pay for.
 */
import fs from 'node:fs'
import path from 'node:path'

const ROOT = path.resolve(import.meta.dirname, '../..')
/**
 * FLAGS. The defaults are the real thing; the overrides exist so the harness can be validated
 * against a throwaway item file without writing anything into the repo (which is how this port was
 * verified before any real item existed).
 */
const argv = process.argv.slice(2)
const flag = (name) => {
  const at = argv.indexOf(`--${name}`)
  return at >= 0 && argv[at + 1] && !argv[at + 1].startsWith('--') ? argv[at + 1] : null
}
const VALIDATE_ONLY = argv.includes('--validate-only')
const TEMPLATE = path.join(ROOT, 'scripts/loop/template.mjs')
const ITEMS_DIR = flag('items') ? path.resolve(flag('items')) : path.join(ROOT, 'scripts/loop/items')
const GEN_DIR = flag('gen') ? path.resolve(flag('gen')) : path.join(ROOT, 'scripts/loop/generated')
const PARENT_OUT = flag('parent') ? path.resolve(flag('parent')) : path.join(ROOT, 'scripts/cicd-loop-all.mjs')

const META_MARKER = '/*__META__*/'
const ITEMS_MARKER = '/*__ITEMS__*/'
const KEEP_MARKER = '/*__KEEP_WORKTREE__*/'
const LANES_MARKER = '/*__LANES__*/'
const LOOP_NAME = 'yap-overhaul-all'
/** The Workflow tool's hard refusal point. Nothing we emit may reach it. */
const WORKFLOW_MAX_BYTES = 524288
/** The packing budget: headroom under the hard cap for prose edits to the template. */
const PART_BUDGET_BYTES = 450000
/** THE ITEM CONTRACT: every field required on every item. `notes` is the one optional extra. */
const REQUIRED_FIELDS = ['id', 'prompt', 'branch', 'title', 'gated', 'preflight', 'spec', 'acceptance']
/** Every loop branch is namespaced, so Drain can tell this run's branches from the repo's own. */
const BRANCH_PREFIX = 'loop/'
/**
 * THE ONLY IDENTIFIERS AN ITEM FILE MAY INTERPOLATE. The item files are evaluated in a sandbox that
 * is handed exactly the template's own single-quoted constants, so a `${SOMETHING_ELSE}` in an item
 * throws a ReferenceError deep inside `new Function` with no useful line number. This list turns
 * that into a named error before anything is evaluated.
 */
const ALLOWED_INTERPOLATIONS = [
  'REPO', 'LOCAL_REPO', 'APP', 'WORKDIR', 'WORKDIR_B', 'CARGO_TARGET_A', 'CARGO_TARGET_B',
  'PREVIEW_PORT', 'PREVIEW_PORT_B', 'LOG', 'TELEMETRY', 'NPM_CACHE', 'STATUS_BOARD', 'STATUS',
  'LOOP_LABEL', 'LOCAL_GATE_TITLE', 'PART', 'SECURITY_IDS', 'SECURITY_PATHS', 'SPEC_SOURCES',
]
/** Nullable: `gated: null` is the normal case and must not read as a missing field. */
const NULLABLE_FIELDS = ['gated']

const problems = []
const fail = (msg) => problems.push(msg)
const die = () => {
  console.error(`\n✗ loop:build FAILED — ${problems.length} problem${problems.length === 1 ? '' : 's'}:\n`)
  for (const p of problems) console.error(`  - ${p}`)
  console.error(`\nNothing was written: not ${path.relative(ROOT, PARENT_OUT)}, not ${path.relative(ROOT, GEN_DIR)}/.\n`)
  process.exit(1)
}

// ── 1. READ THE TEMPLATE ────────────────────────────────────────────────────
if (!fs.existsSync(TEMPLATE)) {
  fail(`template not found: ${TEMPLATE}`)
  die()
}
const template = fs.readFileSync(TEMPLATE, 'utf8')
for (const marker of [META_MARKER, ITEMS_MARKER, KEEP_MARKER, LANES_MARKER]) {
  const n = template.split(marker).length - 1
  if (n !== 1) fail(`template must contain the marker ${marker} exactly once (found ${n})`)
}
/** The per-part log path is stamped off this line, so it has to exist and be unambiguous. */
const LOG_LINE = /^const LOG = '([^']*)\.md'/m
{
  const n = (template.match(new RegExp(LOG_LINE.source, 'gm')) || []).length
  if (n !== 1) fail(`template must declare \`const LOG = '<path>.md'\` exactly once (found ${n}) — the per-part log suffix is stamped onto it`)
}
/** Same idea: the part's own name is stamped onto this line, and the status board is keyed on it. */
const PART_LINE = /^const PART = '[^']*'/m
{
  const n = (template.match(new RegExp(PART_LINE.source, 'gm')) || []).length
  if (n !== 1) fail(`template must declare \`const PART = '<name>'\` exactly once (found ${n}) — the part name is stamped onto it`)
}
/**
 * THE CI-UNAVAILABLE FALLBACK MUST SURVIVE EVERY EDIT TO THE TEMPLATE.
 *
 * On 2026-09-05 GitHub Actions stopped placing jobs on runners for this account (spending limit),
 * so the required "build" check on every PR could never turn green and every merge agent in the
 * loop returned "ci-pending" forever. The fix is a second, LOCAL merge gate that the acting agent
 * runs itself. It only works if BOTH branches are actually reachable from the prompts, so this is
 * a grep-level structural check — cheap, and it fails loudly the moment someone deletes a branch
 * while "simplifying" a prompt.
 *
 * Checked: the mode variable exists, the gate function exists and is a FUNCTION (a const template
 * literal would freeze CI_MODE at module load, before Recon assigns it), both prompt branches are
 * present, both Recon prompts ask for ciMode, and the three merge sites interpolate the gate.
 */
{
  if (!/^let CI_MODE = 'local'$/m.test(template))
    fail(`template must declare \`let CI_MODE = 'local'\` — Recon assigns it after it runs, so it cannot be a const, and on THIS repo the default is local: Actions is disabled account-wide by a spending limit, so an unmeasured run must fall back to the stronger (local) gate rather than onto a check that can never report`)
  if (!/^const CI_GATE = \(dir, target\) =>/m.test(template))
    fail(`template must declare \`const CI_GATE = (dir, target) =>\` as a FUNCTION — a const template literal would bake in CI_MODE's pre-Recon value, and the gate has to name the worktree AND the cargo target dir it measured`)
  for (const branch of ["CI_MODE = local", "CI_MODE = github"]) {
    if (!template.includes(branch))
      fail(`CI_GATE is missing its "${branch}" prompt branch — both must exist or one CI world has no merge gate`)
  }
  if (!template.includes('scripts/loop/ci-mode.mjs'))
    fail(`template must tell Recon to run scripts/loop/ci-mode.mjs — the CI mode is measured, never guessed`)
  const ciModeAsks = (template.match(/ciMode/g) || []).length
  if (ciModeAsks < 4)
    fail(`both Recon prompts must request and return ciMode (found only ${ciModeAsks} mentions of "ciMode" in the template)`)
  if (!/schema: RECON_SCHEMA/.test(template))
    fail(`the recon agent call must pass \`schema: RECON_SCHEMA\` — ciMode is what the rest of the run branches on and free text is not good enough`)
  const gateSites = (template.match(/\$\{CI_GATE\(/g) || []).length
  if (gateSites < 3)
    fail(`every merge site must interpolate \${CI_GATE(...)} — LAND (build mode), FIX+LAND and the SWEEPER. Found ${gateSites} of 3`)
  // THE GATE COMMANDS LIVE IN ONE PLACE. If GATE_CMDS stops being a function, or stops being
  // interpolated everywhere a tree is gated, two agents start gating two different trees.
  if (!/^const GATE_CMDS = \(dir, target\) =>/m.test(template))
    fail(`template must declare \`const GATE_CMDS = (dir, target) =>\` — the gate command list is written down once and interpolated, never retyped per prompt`)
  const cmdSites = (template.match(/\$\{GATE_CMDS\(/g) || []).length
  if (cmdSites < 5)
    fail(`GATE_CMDS must be interpolated at every gating site — the build agent's hard gate, the local CI gate, the fix+land gate, both recon baselines and MAIN VERIFY. Found ${cmdSites}`)
  for (const needle of ['cargo test --features custom-protocol', 'cargo build -p yap-polish --release', 'npx tsc --noEmit']) {
    if (!template.includes(needle))
      fail(`the gate is missing \`${needle}\` — this is Yap's gate, not the sibling project's web gate`)
  }
  if (!template.includes('Local gate (CI unavailable)'))
    fail(`template must pin the PR-comment title "Local gate (CI unavailable)" — Reflect counts local-gate merges by that exact string`)
  /**
   * ACCEPTANCE AND PRE-FLIGHT MUST BE EXECUTED, NOT INTERPOLATED INTO A PROMPT AND FORGOTTEN.
   *
   * The first port of this harness put `item.acceptance` in the builder's prompt and nowhere else:
   * no code path ran it, so "acceptance passed" was a sentence written by the agent whose work it
   * judged. A Workflow script has no shell, so execution is a command-runner SEAT plus control flow
   * in the script. These checks are structural on purpose — they fail the build the moment someone
   * "simplifies" that seat away, which is exactly how the defect got in.
   */
  for (const needle of [
    'async function runCommands(',            // the seat exists
    'const RUN_SCHEMA =',                     // and returns a validated payload
    'schema: RUN_SCHEMA',                     // and the seat is actually given that schema
    'const runPassed =',                      // and the script, not the agent, decides pass/fail
    'item.preflight,',                        // pre-flight is PASSED TO the seat
    'const pre = await runCommands(',         // executed before any builder is dispatched
    "    'preflight',",                         // ...as the pre-flight kind
    "runCommands('acceptance',",              // executed after the builder returns
    "runCommands('acceptance-rerun',",        // and re-executed after the one fix round
    'item.acceptance, p)',                    // with the item's own commands, not a paraphrase
    "status: 'failed-acceptance'",            // a twice-failed acceptance is a recorded outcome
    'FAILURE RECORDER',                       // which is written onto the PR for a human
  ]) {
    if (!template.includes(needle))
      fail(
        `the acceptance-execution path is missing \`${needle}\`. item.preflight must be EXECUTED by a ` +
          `command-runner seat on unmodified main before the builder is dispatched, and item.acceptance ` +
          `EXECUTED on the item's branch after it returns, with ONE fix round and then status ` +
          `'failed-acceptance'. Interpolating acceptance into a prompt is not running it.`
      )
  }
  {
    // The runner seat may not be handed the power to paper over its own result: it is the one seat
    // that must never edit, commit, push or merge.
    const at = template.indexOf('async function runCommands(')
    const body = at < 0 ? '' : template.slice(at, at + 4000)
    if (!/Do not edit, create, move or delete a file/.test(body) || !/Do not commit, rebase, push, merge, close or comment/.test(body))
      fail(`the command-runner seat must be told, in its own prompt, that it may not edit, commit, push, merge or comment — a runner that can "fix" the failure it found is not a gate`)
  }
  const helper = path.join(ROOT, 'scripts/loop/ci-mode.mjs')
  if (!fs.existsSync(helper))
    fail(`scripts/loop/ci-mode.mjs is missing — every agent calls it instead of re-deriving the CI mode by hand`)
}
if (problems.length) die()

/**
 * The item files interpolate the template's own constants (${WORKDIR}, ${PREVIEW_PORT},
 * ${PROD_URL}, ${REPO}, ${LOCAL_REPO}) because in the generated file the pushes run after the
 * constants. To evaluate an item file in isolation we hand those same values to the sandbox,
 * scraped from the template so the two can never drift apart.
 */
const CONSTANTS = {}
for (const m of template.matchAll(/^const ([A-Z][A-Z0-9_]*) = '([^']*)'/gm)) CONSTANTS[m[1]] = m[2]

/**
 * The parent hands the Workflow tool an ABSOLUTE scriptPath per part, and that path must be the
 * real checkout — never wherever this build happened to run from. Built in a git worktree, a
 * ROOT-relative path would bake the worktree in and dangle the moment it is removed. LOCAL_REPO is
 * the template's own pinned constant, so parent and prompts can never disagree about the checkout.
 */
const RUN_ROOT = CONSTANTS.LOCAL_REPO
if (!RUN_ROOT || !path.isAbsolute(RUN_ROOT)) {
  fail(`template must pin \`const LOCAL_REPO = '<absolute path>'\` — the parent's scriptPath is built from it (got ${JSON.stringify(RUN_ROOT)})`)
  die()
}

// ── 2. READ + EVALUATE THE ITEM FILES, IN FILENAME ORDER ────────────────────
if (!fs.existsSync(ITEMS_DIR)) {
  fail(`items directory not found: ${ITEMS_DIR}`)
  die()
}
const itemFiles = fs
  .readdirSync(ITEMS_DIR)
  .filter((f) => f.endsWith('.mjs'))
  .sort()
if (itemFiles.length === 0) {
  fail(`no item files in ${ITEMS_DIR} — an empty loop is never what you meant`)
  die()
}

const ITEMS = []
/**
 * LANE ASSIGNMENT, and it is deliberately coarse: round-robin over the SOURCE ITEM FILES, not over
 * the items. Items inside one prompt group routinely depend on one another (a schema item that a
 * later UI item builds on), so a whole file stays sequential on one lane and only unrelated groups
 * run side by side. File 00 -> lane A, 05 -> lane B, 06 -> lane A, ...
 */
const laneById = new Map()
const sources = [] // parallel to itemFiles: the rendered source block for each file
const itemsByFile = [] // parallel to itemFiles: the items each file pushed, in order
const origin = [] // parallel to ITEMS: the file each item came from, for error messages
for (const file of itemFiles) {
  const src = fs.readFileSync(path.join(ITEMS_DIR, file), 'utf8')
  // NAMED ERROR INSTEAD OF A ReferenceError FROM INSIDE new Function. An item file may interpolate
  // the template's pinned constants and nothing else.
  for (const m of src.matchAll(/\$\{\s*([A-Z][A-Z0-9_]*)\s*[}.[]/g)) {
    if (!ALLOWED_INTERPOLATIONS.includes(m[1]) && !(m[1] in CONSTANTS)) {
      fail(
        `${file}: interpolates \${${m[1]}}, which the template does not pin. An item file may use only: ` +
          `${ALLOWED_INTERPOLATIONS.join(', ')}. Write the literal value instead, or add the constant to the template.`
      )
    }
  }
  sources.push(`// ── ${file} ${'─'.repeat(Math.max(0, 70 - file.length))}\n${src.trim()}\n`)
  const before = ITEMS.length
  try {
    // Sandbox: the file sees ITEMS and the template's constants, and nothing else. No imports,
    // no fs, no network — the contract says an item file is data, so it is evaluated as data.
    const names = ['ITEMS', ...Object.keys(CONSTANTS)]
    new Function(...names, src)(ITEMS, ...Object.keys(CONSTANTS).map((k) => CONSTANTS[k]))
  } catch (err) {
    fail(`${file}: could not be evaluated — ${err && err.message ? err.message : err}`)
    itemsByFile.push([])
    continue
  }
  if (ITEMS.length === before) fail(`${file}: pushed no items (does it call ITEMS.push?)`)
  const lane = itemFiles.indexOf(file) % 2
  for (let i = before; i < ITEMS.length; i++) {
    origin[i] = file
    if (ITEMS[i] && typeof ITEMS[i].id === 'string') laneById.set(ITEMS[i].id, lane)
  }
  itemsByFile.push(ITEMS.slice(before))
}
if (problems.length) die()

// ── 3. VALIDATE THE ITEMS ───────────────────────────────────────────────────
const seenIds = new Map()
const seenBranches = new Map()
for (const [i, item] of ITEMS.entries()) {
  const where = `${origin[i]} item #${i + 1}`
  if (!item || typeof item !== 'object') {
    fail(`${where}: not an object`)
    continue
  }
  for (const field of REQUIRED_FIELDS) {
    if (!(field in item)) {
      fail(`${where} (${item.id || 'no id'}): missing required field \`${field}\` (CONTRACT.md)`)
    } else if (!NULLABLE_FIELDS.includes(field) && (typeof item[field] !== 'string' || item[field].trim() === '')) {
      fail(`${where} (${item.id || 'no id'}): field \`${field}\` must be a non-empty string`)
    }
  }
  if (item.gated !== null && item.gated !== 'panel') {
    fail(`${where} (${item.id}): \`gated\` must be null or 'panel', got ${JSON.stringify(item.gated)}`)
  }
  if (typeof item.id === 'string') {
    if (seenIds.has(item.id)) fail(`duplicate item id ${item.id} — in ${seenIds.get(item.id)} and ${where}`)
    else seenIds.set(item.id, where)
  }
  if (typeof item.branch === 'string' && !item.branch.startsWith(BRANCH_PREFIX)) {
    fail(
      `${where} (${item.id}): branch ${JSON.stringify(item.branch)} must start with ${JSON.stringify(BRANCH_PREFIX)}. ` +
        `The repo's own history uses feat/* and fix/*; the loop's branches are namespaced so Drain can tell them apart ` +
        `and so a sweep never deletes a human's branch.`
    )
  }
  if (typeof item.branch === 'string') {
    if (seenBranches.has(item.branch)) {
      fail(`duplicate branch ${item.branch} — ${seenBranches.get(item.branch)} and ${item.id}. Two items on one branch cannot both merge.`)
    } else seenBranches.set(item.branch, item.id)
  }
}
if (problems.length) die()

// ── 4. RENDER ───────────────────────────────────────────────────────────────
// The meta literal must be a PURE LITERAL and the first statement in the file: the Workflow tool
// reads it without executing the script. No template literals, no calls, no interpolation.
const lit = (s) => JSON.stringify(String(s))
const pad2 = (n) => String(n).padStart(2, '0')
const bytes = (s) => Buffer.byteLength(s, 'utf8')
const partName = (n) => `part-${pad2(n)}`
const partFile = (n) => path.join(GEN_DIR, `${partName(n)}.mjs`)
/** Where the parent tells the Workflow tool to find the part: the real checkout, always. */
const partRunPath = (n) => path.join(RUN_ROOT, 'scripts/loop/generated', `${partName(n)}.mjs`)
const span = (items) => `${items[0].id}..${items[items.length - 1].id}`

/** One PART: the whole template, a subset of the items, its own log file, its own meta. */
function renderPart(partNo, items, srcs, keepWorktree) {
  const nn = pad2(partNo)
  const gatedCount = items.filter((i) => i.gated === 'panel').length
  const description =
    `Part ${nn} of the Yap (wilson-voice) overhaul, two builder lanes: land whatever is already ` +
    `gate-green, pre-flight on unmodified main (already-done items are skipped), build, open a ` +
    `labelled PR. The adversarial review, the independent second-Opus gate and the merge bar run ` +
    `afterwards in the same script with args {mode:'review'}. ${items.length} item${items.length === 1 ? '' : 's'} (${span(items)})` +
    (gatedCount ? `, ${gatedCount} awaiting the Senior Panel` : '') +
    `. The gate is local (Actions is disabled by the account spending limit); the DMG is not in it.`
  const phaseLines = [
    `    { title: "Recon", detail: "two lane worktrees, one npm ci + one warm cargo build each, the loop-build label, ci-mode measured" },`,
    ...items.map((i) => `    { title: ${lit(i.id)}, detail: ${lit(i.title)} },`),
    `    { title: "Drain", detail: "triage every open PR (the stale feat/yv1xx ones included), sweep dead branches, tear down both worktrees and both cargo target dirs" },`,
    `    { title: "Reflect", detail: "count outcomes, reconcile against gh, append telemetry" },`,
  ]
  const metaSource = `export const meta = {
  name: ${lit(`${LOOP_NAME}-${partName(partNo)}`)},
  description:
    ${lit(description)},
  phases: [
${phaseLines.join('\n')}
  ],
}`
  const lanes = JSON.stringify(Object.fromEntries(items.map((it) => [it.id, laneById.get(it.id) === 1 ? 1 : 0])))
  const out = template
    .replace(META_MARKER, () => metaSource)
    .replace(ITEMS_MARKER, () => srcs.join('\n'))
    .replace(KEEP_MARKER, () => (keepWorktree ? 'true' : 'false'))
    .replace(LANES_MARKER, () => lanes)
  // Per-part log file, so ten parts do not interleave their appends into one log, and the part's
  // own name, which the status board rows are grouped under.
  return out
    .replace(LOG_LINE, (_m, base) => `const LOG = '${base}-part${nn}.md'`)
    .replace(PART_LINE, () => `const PART = '${partName(partNo)}'`)
}

// ── 4a. PACK THE ITEM FILES INTO PARTS ──────────────────────────────────────
// Greedy, in rollout order, whole item files only. An item file is never split across parts.
const groups = []
let cur = { files: [], items: [], srcs: [] }
for (let i = 0; i < itemFiles.length; i++) {
  const cand = {
    files: [...cur.files, itemFiles[i]],
    items: [...cur.items, ...itemsByFile[i]],
    srcs: [...cur.srcs, sources[i]],
  }
  const size = bytes(renderPart(groups.length + 1, cand.items, cand.srcs, true))
  if (size > PART_BUDGET_BYTES && cur.files.length > 0) {
    groups.push(cur)
    cur = { files: [itemFiles[i]], items: [...itemsByFile[i]], srcs: [sources[i]] }
    const solo = bytes(renderPart(groups.length + 1, cur.items, cur.srcs, true))
    if (solo > PART_BUDGET_BYTES) {
      fail(
        `${itemFiles[i]} renders to ${solo} bytes on its own — over the ${PART_BUDGET_BYTES}-byte part budget. ` +
          `An item file is never split across parts, so this file has to be split into two item files by hand.`
      )
    }
  } else {
    cur = cand
  }
}
if (cur.files.length) groups.push(cur)
if (problems.length) die()

const parts = groups.map((g, idx) => {
  const partNo = idx + 1
  const isLast = idx === groups.length - 1
  const src = renderPart(partNo, g.items, g.srcs, !isLast)
  return {
    partNo,
    name: partName(partNo),
    file: partFile(partNo),
    runPath: partRunPath(partNo),
    src,
    size: bytes(src),
    items: g.items,
    files: g.files,
    keepWorktree: !isLast,
  }
})

// ── 4b. RENDER THE PARENT ───────────────────────────────────────────────────
// Small on purpose: it holds no prompts. It runs each part inline with
// `workflow({scriptPath}, args)` — ONE level of nesting, so no part may call workflow() itself —
// passing `args` straight through so args.panelApproved reaches the gated items.
function renderParent(list) {
  const gatedCount = ITEMS.filter((i) => i.gated === 'panel').length
  const description =
    `Every item of the Yap (wilson-voice) overhaul, run as ${list.length} part${list.length === 1 ? '' : 's'} ` +
    `because the Workflow tool caps a script at ${WORKFLOW_MAX_BYTES} bytes. BUILD FIRST: each part runs two builder ` +
    `lanes that land whatever is already gate-green, pre-flight on unmodified main (already-done items are skipped), ` +
    `build, and open a labelled unreviewed PR. The adversarial review, the independent second-Opus gate and the merge ` +
    `bar are a SEPARATE pass, run afterwards with args {mode:'review'}. ${ITEMS.length} item${ITEMS.length === 1 ? '' : 's'} ` +
    `in all` +
    (gatedCount ? `, ${gatedCount} awaiting the Senior Panel` : '') +
    `. The parts share two worktrees and two warm cargo caches; the last part tears them down. A failed part is logged ` +
    `and the run continues.`
  const phaseLines = list.map((p) => `    { title: ${lit(p.name)}, detail: ${lit(span(p.items))} },`)
  const metaSource = `export const meta = {
  name: ${lit(LOOP_NAME)},
  description:
    ${lit(description)},
  phases: [
${phaseLines.join('\n')}
  ],
}`
  const blocks = list.map((p, idx) => {
    const intro =
      `${p.name} — ${p.items.length} item(s), ${span(p.items)} — ` +
      `${p.runPath} (${p.size} bytes)`
    return `if (halted) {
  log(${lit(`${p.name} — SKIPPED, the run halted earlier: `)} + halted.reason)
  results.push({ part: ${lit(p.name)}, status: 'skipped: run halted', items: ${p.items.length} })
}${
      idx === 0
        ? ''
        : ` else if (REVIEW_MODE) {
  log(${lit(`${p.name} — SKIPPED: mode=review runs ${list[0].name} ONLY. The review pass is PR-DRIVEN — one triage agent enumerates every open loop-build PR on the repo and two chains consume that queue — so every further part would re-triage the same PRs and dispatch duplicate reviewers at them.`)})
  results.push({ part: ${lit(p.name)}, status: 'skipped: review mode runs ${list[0].name} only', items: ${p.items.length} })
}`
    } else {
  phase(${lit(p.name)})
  log(${lit(`START ${intro}`)})
  try {
    const result = await workflow({ scriptPath: ${lit(p.runPath)} }, args)
    log(${lit(`END ${p.name} — `)} + (result && result.halted ? 'halted' : 'finished') + ${lit(` (${p.items.length} item(s))`)})
    results.push({ part: ${lit(p.name)}, status: 'ok', items: ${p.items.length}, result })
    if (result && result.halted) {
      halted = { part: ${lit(p.name)}, at: result.at || null, reason: result.reason || 'halted' }
      log(${lit(`HALT: ${p.name} stopped at `)} + String(halted.at) + ': ' + halted.reason + ${lit(`. No further part will be launched; resume with resumeFromRunId after the reset. The shared worktree is left standing.`)})
    }
  } catch (err) {
    const message = err && err.message ? err.message : String(err)
    if (isHaltError(message)) {
      halted = { part: ${lit(p.name)}, at: null, reason: message }
      results.push({ part: ${lit(p.name)}, status: 'halted', items: ${p.items.length}, error: message })
      log(${lit(`HALT: ${p.name} threw `)} + message + ${lit(` at even the part level — stopping the run; resume with resumeFromRunId after the reset.`)})
    } else {
      log(${lit(`${p.name} FAILED — `)} + message + ${lit(`. Continuing to the next part; the drain of a later part triages what this one left open.`)})
      results.push({ part: ${lit(p.name)}, status: 'errored', items: ${p.items.length}, error: message })
    }
  }
}`
  })
  return `${metaSource}
/**
 * GENERATED by scripts/loop/build.mjs — do not hand-edit. Re-run \`npm run loop:build\`.
 *
 * The thin PARENT of the Yap overhaul loop. It holds no prompts and dispatches no agents:
 * every prompt lives in scripts/loop/generated/part-NN.mjs, because the Workflow tool refuses a
 * script file over ${WORKFLOW_MAX_BYTES} bytes and the single-file loop had reached 781367.
 *
 * Each part is run inline with workflow({scriptPath}, args). That is ONE level of nesting and the
 * runtime allows no more, so a part must never call workflow() itself. \`args\` is passed through
 * untouched, which is how args.panelApproved reaches the panel-gated items inside a part.
 *
 * The parts share the two lane worktrees (${CONSTANTS.WORKDIR || 'the loop work dir'} and
 * ${CONSTANTS.WORKDIR_B || 'its lane-B sibling'}) and the two cargo target dirs beside them: every
 * part but the last is stamped KEEP_WORKTREE=true, so only the last part's Drain removes them. A
 * cold cargo build of this workspace is minutes, which is the whole reason they persist. A part
 * that throws is logged and the run moves to the next part — one bad part must not kill the run.
 *
 * HALT. A part that returns { halted: true } — or throws something that reads like an account
 * limit — stops the run: no further part is launched. agent() does NOT throw when a subagent dies
 * on a session/usage limit: it resolves to null, and a per-item catch then dispatches every
 * remaining agent into a wall (288 instant failures, once). Resume after the reset with
 * resumeFromRunId. Because the halting part's Drain never runs, the worktrees are deliberately
 * still there for that resume.
 */
const HALT_PATTERNS = [/session limit/i, /usage limit/i, /rate limit.*resets/i]
const isHaltError = (message) => HALT_PATTERNS.some((re) => re.test(String(message || '')))
/**
 * REVIEW MODE RUNS THE FIRST PART ONLY. Build mode's parts are a partition of the ITEMS, so every
 * one must run. Review mode is PR-DRIVEN: its triage agent enumerates every open loop-build PR on
 * the repo, so a second part would triage the same queue again and dispatch a second reviewer at
 * every PR. Every part but the first is skipped with a log line. THE INTENDED ORDER IS: every part
 * in build mode first, then ONE run with args {mode:'review'}.
 */
const REVIEW_MODE = (typeof args !== 'undefined' && args && args.mode) === 'review'
const results = []
let halted = null

${blocks.join('\n\n')}

log(
  (halted ? 'run HALTED at ' + halted.part + (halted.at ? '/' + halted.at : '') + ': ' + halted.reason + ' — ' : 'all parts finished: ') +
    results.filter((r) => r.status === 'ok').length +
    ' ok, ' +
    results.filter((r) => r.status === 'errored').length +
    ' errored, ' +
    results.filter((r) => r.status === 'skipped: run halted').length +
    ' skipped, of ' +
    results.length
)

return { parts: results, halted: halted !== null, at: halted ? halted.at : null }
`
}
const parentSrc = renderParent(parts)
const parent = { name: LOOP_NAME, file: PARENT_OUT, src: parentSrc, size: bytes(parentSrc) }

// ── 5. VALIDATE EVERY OUTPUT ────────────────────────────────────────────────
// Comments and blank lines may precede the meta literal; nothing else may.
const stripLeading = (s) => {
  let i = 0
  for (;;) {
    while (i < s.length && /\s/.test(s[i])) i++
    if (s.startsWith('/*', i)) {
      const end = s.indexOf('*/', i + 2)
      if (end < 0) break
      i = end + 2
      continue
    }
    if (s.startsWith('//', i)) {
      const end = s.indexOf('\n', i)
      if (end < 0) break
      i = end + 1
      continue
    }
    break
  }
  return s.slice(i)
}

/**
 * Every check the single-file build ran, run once per emitted file, plus the size cap that made
 * the split necessary in the first place. Returns meta.phases so the caller can cross-check them.
 */
function validateScript(label, generated, items) {
  const bad = (msg) => fail(`${label}: ${msg}`)
  // 5a. the meta literal is the FIRST STATEMENT
  const head = stripLeading(generated)
  if (!head.startsWith('export const meta = {')) {
    bad(`\`export const meta\` is not the first statement — the file starts with: ${head.slice(0, 80).split('\n')[0]}`)
  }
  // 5b. the meta block is a pure literal, and its phases are what we think they are
  let metaPhases = []
  const start = generated.indexOf('export const meta = {')
  let depth = 0
  let end = -1
  for (let i = generated.indexOf('{', start); i < generated.length; i++) {
    if (generated[i] === '{') depth++
    else if (generated[i] === '}' && --depth === 0) {
      end = i + 1
      break
    }
  }
  if (end < 0) bad('could not find the end of the meta literal')
  else {
    const body = generated.slice(start + 'export const meta = '.length, end)
    // Test the STRUCTURE, not the prose: string contents are data and may say anything.
    const skeleton = body.replace(/"(?:[^"\\]|\\.)*"/g, '""')
    if (/[`]|\$\{|\w\s*\(/.test(skeleton)) {
      bad('the meta literal is not pure: outside its strings it contains a backtick, an interpolation or a call')
    }
    try {
      metaPhases = new Function(`return ${body}`)().phases
    } catch (err) {
      bad(`the meta literal does not evaluate: ${err && err.message ? err.message : err}`)
    }
  }
  // 5c. the whole script parses the way the Workflow tool wraps it
  try {
    // eslint-disable-next-line no-new-func
    new Function(`(async()=>{${generated.replace(/^export const meta/m, 'const meta')}})`)
  } catch (err) {
    bad(`the script does not parse when wrapped: ${err && err.message ? err.message : err}`)
  }
  // 5d. banned strings: the retired CI mirror, the killed watchdog, a non-Opus seat, and every
  //     source of nondeterminism the Workflow runtime forbids
  const BANNED = /drivia-ci|watchdog|fable|Date\.now|Math\.random|new Date\(/g
  for (const m of generated.matchAll(BANNED)) {
    const line = generated.slice(0, m.index).split('\n').length
    bad(`banned string "${m[0]}" at line ${line}`)
  }
  // 5d-bis. WRONG-REPO / WRONG-SCRIPT DRIFT. These are the exact shapes that appear when an item is
  // written by copying the sibling project's prose: a path or repo that does not exist here, or an
  // npm script that desktop/package.json does not define (dev, build, preview, test, tauri,
  // sidecar, desktop:dev, desktop:build are the only ones). Each would fail at agent runtime, an
  // hour in, and read as a code defect rather than as the prompt bug it is.
  const DRIFT = [
    [/drivia-hustle|drivia\.consulting|wilsonguenther-dev\/drivia\b|drivia-fixtures/i, 'a path, repo or fixture from the sibling project — this loop runs against wilsonguenther-dev/wilson-voice'],
    [/WILSON_QA_ACK/, "the sibling project's pre-commit ack — this repo has no hooks path set, so plain `git commit` is correct"],
    [/npm run (lint|typecheck|dead-code|start|check:route-conflicts)\b/, 'an npm script desktop/package.json does not define — the gate is: npx tsc --noEmit, npm test, npm run build, the two cargo test suites and cargo clippy'],
    [/supabase|next\.config|app\/dashboard/i, "a web-stack surface Yap does not have"],
  ]
  for (const [re, why] of DRIFT) {
    const m = generated.match(re)
    if (m) {
      const line = generated.slice(0, m.index).split('\n').length
      bad(`"${m[0]}" at line ${line} is ${why}`)
    }
  }
  // (case-insensitive, but only where it would actually pick a model — the brief's filename is
  //  allowed to say FABLE, a seat is not)
  for (const m of generated.matchAll(/model:\s*['"](?!opus)([a-z0-9-]+)['"]/gi)) {
    bad(`non-opus seat model: ${m[0]} — every seat in this loop is opus`)
  }
  // 5e. every phase() the script actually calls exists in meta.phases
  const titles = new Set((metaPhases || []).map((p) => p && p.title))
  for (const m of generated.matchAll(/(?<![\w.])phase\(([^)]*)\)/g)) {
    const arg = m[1].trim()
    const line = generated.slice(0, m.index).split('\n').length
    const literal = arg.match(/^'([^']*)'$|^"([^"]*)"$/)
    if (literal) {
      const title = literal[1] !== undefined ? literal[1] : literal[2]
      if (!titles.has(title)) bad(`phase('${title}') at line ${line} is not in meta.phases`)
    } else if (arg === 'item.id') {
      for (const item of items) if (!titles.has(item.id)) bad(`phase(item.id) would emit "${item.id}", which is not in meta.phases`)
    } else {
      bad(`phase(${arg}) at line ${line} is neither a string literal nor item.id — the validator cannot prove it is declared`)
    }
  }
  // 5f. THE CAP THAT FORCED THE SPLIT
  const size = bytes(generated)
  if (size > WORKFLOW_MAX_BYTES) {
    bad(`${size} bytes — the Workflow tool refuses any script file over ${WORKFLOW_MAX_BYTES}. Lower PART_BUDGET_BYTES or split an item file.`)
  }
  return metaPhases || []
}

for (const p of parts) validateScript(path.relative(ROOT, p.file), p.src, p.items)
const parentPhases = validateScript(path.relative(ROOT, PARENT_OUT), parent.src, [])

// 5g. a part must not nest a workflow() of its own — the runtime allows one level only
for (const p of parts) {
  for (const m of p.src.matchAll(/(?<![\w.])workflow\s*\(/g)) {
    const line = p.src.slice(0, m.index).split('\n').length
    fail(`${path.relative(ROOT, p.file)}: calls workflow() at line ${line} — a child script may not nest another`)
  }
}
// 5k-5n. THE CONCURRENCY CONTRACT — parts only. The parent runs no lanes and dispatches no agents.
for (const p of parts) {
  const label = path.relative(ROOT, p.file)
    // 5k. THE PHASE CURSOR HAS ONE WRITER. phase() is global; two lanes run concurrently. Everything
  //     between the NO-PHASE sentinels (runBuild, runTail, the safety wrappers) must carry its
  //     phase in opts instead, and the lane that owns the cursor sets it outside that region.
  {
    const s = p.src.indexOf('══ NO-PHASE REGION START')
    const e = p.src.indexOf('══ NO-PHASE REGION END')
    if (s < 0 || e < 0 || e < s) fail(`${label}: ` + 'the NO-PHASE sentinels are missing or out of order — the concurrency check cannot run')
    else {
      for (const m of p.src.slice(s, e).matchAll(/(?<![\w.])phase\(/g)) {
        fail(`${label}: ` + `phase() is called inside the NO-PHASE region (offset ${s + m.index}) — that code runs concurrently and would race the global cursor`)
      }
    }
  }
  // 5l. parallel() IS THE REVIEW BARRIER AND NOTHING ELSE. Concurrency anywhere else in this
  //     harness would mean two agents in one worktree.
  {
    const uses = [...p.src.matchAll(/(?<![\w.])parallel\s*\(/g)]
    if (uses.length === 0) fail(`${label}: ` + 'no parallel() call — the two reviewers are supposed to run as a barrier')
    for (const m of uses) {
      if (!p.src.slice(Math.max(0, m.index - 1200), m.index).includes('/* REVIEW-STAGE-PARALLEL */')) {
        const line = p.src.slice(0, m.index).split('\n').length
        fail(`${label}: ` + `parallel() at line ${line} is not the review-stage barrier (no /* REVIEW-STAGE-PARALLEL */ marker above it)`)
      }
    }
  }
  // 5m. EVERY SEAT DECLARES ITS PHASE EXPLICITLY. Inside concurrent code a global phase() races, so
  //     opts.phase is the only thing that can be trusted to label an agent correctly.
  {
    const seats = [...p.src.matchAll(/model: 'opus'/g)]
    const dispatches = [...p.src.matchAll(/(?:await agentR\(|\(\) => agentR\()/g)]
    if (seats.length !== dispatches.length) {
      fail(`${label}: ` + `${dispatches.length} agent dispatch site(s) but ${seats.length} opus seat option object(s) — every agent() must carry its own { model, effort, phase, label }`)
    }
    for (const m of seats) {
      if (!p.src.slice(m.index, m.index + 240).includes('phase:')) {
        const line = p.src.slice(0, m.index).split('\n').length
        fail(`${label}: ` + `the agent options at line ${line} do not declare an explicit phase:`)
      }
    }
  }
  // 5n. EXACTLY TWO CONCURRENCY SITES, ONE PER MODE, AND THEY ARE NAMED.
  //     BUILD mode: the two builder lanes, and nothing else — that rule stays strict.
  //     REVIEW mode: the two review chains, and nothing else. Any third Promise.all is a
  //     concurrency site nobody designed, which in this harness means two agents in one worktree.
  {
    const alls = [...p.src.matchAll(/Promise\.all\(/g)]
    if (alls.length !== 2) fail(`${label}: ` + `${alls.length} Promise.all( call(s) — exactly two are allowed: the builder lanes (build mode) and the review chains (review mode)`)
    if (!p.src.includes('await Promise.all([buildLane(0), buildLane(1)])')) {
      fail(`${label}: ` + 'build mode must run `await Promise.all([buildLane(0), buildLane(1)])` — nothing else may be run concurrently there')
    }
    if (!p.src.includes('await Promise.all([reviewChain(0), reviewChain(1)])')) {
      fail(`${label}: ` + 'review mode must run `await Promise.all([reviewChain(0), reviewChain(1)])` — the two review chains are its only concurrency')
    }
  }
  // 5o. THE 3-AGENT CAP IS A CONSTANT, AND NO REVIEW DISPATCH MAY ESCAPE IT.
  //     Review mode runs two chains and a security PR seats two reviewers at once, so without the
  //     semaphore the run peaks at four concurrent opus/high agents. The cap is 3. It is checked
  //     as a literal, because a cap that a later edit can raise silently is not a cap.
  {
    const decl = [...p.src.matchAll(/const MAX_AGENTS = (\d+)/g)]
    if (decl.length !== 1) fail(`${label}: ` + `${decl.length} \`const MAX_AGENTS = <n>\` declaration(s) — there must be exactly one`)
    else if (decl[0][1] !== '3') fail(`${label}: ` + `MAX_AGENTS is ${decl[0][1]}, and the hard cap for this run is 3 concurrent agents`)
    for (const needle of [
      'async function acquireAgents(n)',
      'function releaseAgents(n)',
      'async function withAgents(n, fn)',
      'if (activeAgents + n <= MAX_AGENTS)',
      'if (activeAgents + w.n <= MAX_AGENTS)',
    ]) {
      if (!p.src.includes(needle)) fail(`${label}: ` + `the review semaphore is missing \`${needle}\` — the 3-agent cap cannot be enforced without it`)
    }
    const s = p.src.indexOf('══ REVIEW CHAIN REGION START')
    const e = p.src.indexOf('══ REVIEW CHAIN REGION END')
    if (s < 0 || e < 0 || e < s) fail(`${label}: ` + 'the REVIEW CHAIN REGION sentinels are missing or out of order — the agent-cap check cannot run')
    else {
      const region = p.src.slice(s, e)
      const leases = [...region.matchAll(/withAgents\(/g)]
      if (leases.length < 2) fail(`${label}: ` + `${leases.length} withAgents( lease(s) in the review chain region — the reviewer seat(s) and the fix+land seat must each take one`)
      for (const m of region.matchAll(/(?:await agentR\(|\(\) => agentR\()/g)) {
        if (!region.slice(0, m.index).includes('withAgents(')) {
          const line = p.src.slice(0, s + m.index).split('\n').length
          fail(`${label}: ` + `an agent is dispatched at line ${line} inside the review chain region without a withAgents() lease above it — that path can exceed MAX_AGENTS`)
        }
      }
    }
  }
}
// 5h. the parent's phases ARE the parts, in order
{
  const declared = parentPhases.map((p) => p && p.title)
  const expected = parts.map((p) => p.name)
  if (declared.join('|') !== expected.join('|')) {
    fail(`the parent's meta.phases [${declared.join(', ')}] do not match the parts [${expected.join(', ')}]`)
  }
  if (!parts.every((p) => parent.src.includes(p.runPath))) {
    fail(`the parent does not reference every part script by its absolute path under ${RUN_ROOT}`)
  }
  if (parts.some((p) => p.file !== p.runPath && parent.src.includes(p.file))) {
    fail(`the parent references a build-time path (${path.dirname(parts[0].file)}) instead of the pinned checkout ${RUN_ROOT} — that path dangles the moment this worktree is removed`)
  }
  if (!/workflow\(\{ scriptPath: "[^"]+" \}, args\)/.test(parent.src)) {
    fail('the parent must pass `args` through to each part untouched')
  }
}
// 5i. the union of the parts is every item, exactly once, in order
{
  const union = parts.flatMap((p) => p.items.map((i) => i.id))
  const all = ITEMS.map((i) => i.id)
  if (union.join('|') !== all.join('|')) {
    const missing = all.filter((id) => !union.includes(id))
    const extra = union.filter((id) => !all.includes(id))
    fail(
      `the parts are not the items: ${union.length} across parts vs ${all.length} total` +
        (missing.length ? `; missing ${missing.join(', ')}` : '') +
        (extra.length ? `; unexpected ${extra.join(', ')}` : '') +
        (!missing.length && !extra.length ? '; same set, wrong order' : '')
    )
  }
  const files = parts.flatMap((p) => p.files)
  if (files.join('|') !== itemFiles.join('|')) {
    fail(`the parts are not the item files: [${files.join(', ')}] vs [${itemFiles.join(', ')}]`)
  }
}
// 5j. exactly one part tears the worktree down, and it is the last one
{
  const keepers = parts.filter((p) => p.keepWorktree).length
  if (keepers !== parts.length - 1) fail(`${parts.length - keepers} part(s) would remove the shared worktree — exactly one (the last) may`)
  if (parts.length > 1 && parts[parts.length - 1].keepWorktree) fail('the last part must not keep the worktree')
}
if (problems.length) die()

// ── 6. WRITE ────────────────────────────────────────────────────────────────
if (VALIDATE_ONLY) {
  console.log(`✓ loop:validate — ${parts.length} part(s) + 1 parent would be written, ${ITEMS.length} item(s), every check passed.`)
  console.log(`  items      ${path.relative(ROOT, ITEMS_DIR)} — ${itemFiles.length} file(s): ${itemFiles.join(', ')}`)
  for (const p of parts) console.log(`  ${p.name}  ${p.size} bytes  ${p.items.length} item(s)  ${span(p.items)}  keeps worktree: ${p.keepWorktree}`)
  console.log(`  parent     ${parent.size} bytes (meta.name ${LOOP_NAME})`)
  console.log('  NOTHING WAS WRITTEN (--validate-only).')
  process.exit(0)
}
fs.mkdirSync(GEN_DIR, { recursive: true })
const keepNames = new Set(parts.map((p) => path.basename(p.file)))
for (const stale of fs.readdirSync(GEN_DIR).filter((f) => /^part-\d+\.mjs$/.test(f) && !keepNames.has(f))) {
  fs.rmSync(path.join(GEN_DIR, stale))
  console.log(`  removed stale ${path.relative(ROOT, path.join(GEN_DIR, stale))}`)
}
for (const p of parts) fs.writeFileSync(p.file, p.src)
fs.writeFileSync(PARENT_OUT, parent.src)

// ── 7. REPORT ───────────────────────────────────────────────────────────────
const rel = (p) => path.relative(ROOT, p)
const num = (n) => n.toLocaleString('en-US')
console.log(`✓ loop:build wrote ${parts.length} part(s) + 1 parent — ${ITEMS.length} items, cap ${num(WORKFLOW_MAX_BYTES)} bytes/file`)
console.log(`  loop       ${LOOP_NAME} (build mode first, all items; review is a separate pass)`)
console.log(`  template   ${rel(TEMPLATE)}`)
console.log(`  items      ${itemFiles.length} file(s): ${itemFiles.join(', ')}`)
console.log('')
console.log(`  ${'part'.padEnd(10)}${'bytes'.padStart(9)}${'items'.padStart(7)}  ${'first..last'.padEnd(24)}keeps worktree`)
console.log(`  ${'─'.repeat(10)}${'─'.repeat(9)}${'─'.repeat(7)}  ${'─'.repeat(24)}${'─'.repeat(14)}`)
for (const p of parts) {
  console.log(`  ${p.name.padEnd(10)}${num(p.size).padStart(9)}${String(p.items.length).padStart(7)}  ${span(p.items).padEnd(24)}${p.keepWorktree ? 'yes' : 'no (tears down)'}`)
}
console.log(`  ${'parent'.padEnd(10)}${num(parent.size).padStart(9)}${String(0).padStart(7)}  ${'runs every part'.padEnd(24)}n/a`)
console.log('')
for (const p of parts) console.log(`  ${p.name}  ${rel(p.file)}  ←  ${p.files.join(', ')}`)
console.log(`  parent runs each part from the pinned checkout: ${RUN_ROOT}/scripts/loop/generated/`)
console.log(`  parent    ${rel(PARENT_OUT)}  (meta.name ${LOOP_NAME})`)
console.log(`  gated      ${ITEMS.filter((i) => i.gated === 'panel').map((i) => i.id).join(', ') || 'none'}`)
console.log(
  `  lanes      A: ${ITEMS.filter((i) => laneById.get(i.id) === 0).length} item(s) from ${itemFiles.filter((f, idx) => idx % 2 === 0).length} file(s)` +
    `  ·  B: ${ITEMS.filter((i) => laneById.get(i.id) === 1).length} item(s) from ${itemFiles.filter((f, idx) => idx % 2 === 1).length} file(s)`
)
console.log(
  `  validated  meta-first-statement, pure-literal meta, wrapped-parse, banned-strings, wrong-repo/wrong-npm-script drift, opus-only seats, phase()⊆meta.phases, unique ids, unique branches (${BRANCH_PREFIX}*), item interpolation whitelist, CONTRACT fields, size≤${num(WORKFLOW_MAX_BYTES)}, no nested workflow(), parent-phases=parts, args passed through, item union == all items in order, no phase() in the NO-PHASE region, parallel() only at the review barrier, every agent() declares a phase, Promise.all wraps exactly the builder lanes + the review chains, MAX_AGENTS=3 with every review dispatch inside a withAgents() lease, CI_MODE defaults to local with both gate branches present, GATE_CMDS is the single source of the gate`
)
console.log(`  NOTE       ${rel(PARENT_OUT)} and ${rel(GEN_DIR)}/ are generated — do not hand-edit them; re-run \`npm run loop:build\`.`)
