// scripts/loop/build-only-lanes.test.mjs — node --test. HERMETIC (no gh, no agents, no network).
// Pins the 2026-10-27 harness change (Wilson: "put all those three build agents back to the rest of the items ...
// merge them all ... Just add telemetry where we need to so we can update"):
//   1. args.lanes: 3 — lane C exists; lanes are split PER PART at run time (splitLanes, 2026-10-27 follow-up: the
//      per-file round-robin left lane C idle for part-01): round-robin in file order, pins honoured, ±1 per lane,
//      and an idle lane STEALS the next unstarted unpinned item from the lane with the most left.
//   2. args.landBar: 'ci' — BUILD-ONLY: the drain is the lander (gh pr merge on CI green, loop-ci-red after one
//      fix round) and carries no review chain / parking label; the default drain is unchanged.
//   3. the TELEMETRY (Wilson 2026-10-27) block reaches the opus builder prompt and the Sonnet builder preamble.
// It renders template.mjs with the four markers filled in-test and RUNS it against stubbed globals (the same
// technique as build.mjs's dry run), capturing every dispatched prompt.
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'

const HERE = path.dirname(new URL(import.meta.url).pathname)
const TEMPLATE = fs.readFileSync(path.join(HERE, 'template.mjs'), 'utf8')
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor

// Six item "files": ordinals 0..4 round-robin, plus one item pinned to lane A that sits at ordinal 4.
const ITEM_DEFS = [
  { id: 'T-F0a', ord: 0 }, { id: 'T-F0b', ord: 0 },
  { id: 'T-F1', ord: 1 }, { id: 'T-F2', ord: 2 }, { id: 'T-F3', ord: 3 },
  { id: 'T-PIN', ord: 4, lane: 'A' }, { id: 'T-F4', ord: 4 },
]
const itemSrc = ITEM_DEFS.map((d) => `ITEMS.push(${JSON.stringify({
  id: d.id, prompt: d.id, branch: `loop/${d.id.toLowerCase()}`, title: `${d.id} title`, gated: null,
  preflight: 'false', spec: `spec of ${d.id}`, acceptance: 'true', ...(d.id === 'T-F1' ? { builder: 'sonnet' } : {}),
})})`).join('\n')
const lanesMap = JSON.stringify(Object.fromEntries(ITEM_DEFS.map((d) => [d.id, d.lane || d.ord])))
const META = `export const meta = { name: "t", description: "t", phases: [{ title: "Recon" }, { title: "Drain" }, { title: "Reflect" }] }`

function render({ keep = false } = {}) {
  return TEMPLATE.replace('/*__META__*/', () => META)
    .replace('/*__ITEMS__*/', () => itemSrc)
    .replace('/*__KEEP_WORKTREE__*/', () => (keep ? 'true' : 'false'))
    .replace('/*__LANES__*/', () => lanesMap)
    .replace('const SEAT_MODELS = {}', () => `const SEAT_MODELS = ${JSON.stringify({ enabled: true, seats: {}, builders: { model: 'sonnet', effort: 'high', require: { build: ['status'], fix: ['fixed'] } }, escalate: { model: 'opus' } })}`)
}

async function run(argsIn, opts = {}) {
  const prompts = []
  const done = new Set(opts.alreadyDone || [])
  const agent = async (prompt, o) => {
    const label = (o && o.label) || ''
    prompts.push({ label, prompt: String(prompt) })
    if (label.startsWith('preflight:') && done.has(label.split(':')[2])) return { allZero: true, exits: [{ command: 'true', exit: 0 }] }
    if (label.startsWith('preflight:')) return { allZero: false, exits: [{ command: 'false', exit: 1 }], firstFailure: 'false' }
    if (label.startsWith('acceptance')) return { allZero: true, exits: [{ command: 'true', exit: 0 }] }
    if (label.startsWith('build:')) return { itemId: 'x', status: 'built', prNumber: 1 }
    return { ok: true, ciMode: 'local' }
  }
  const noop = () => {}
  const body = (opts.src || render(opts)).replace(/^export const meta/m, 'const meta')
  const fn = new AsyncFunction('agent', 'parallel', 'pipeline', 'phase', 'log', 'workflow', 'args', 'budget', body)
  const out = await fn(agent, async (f) => Promise.all(f.map((x) => x())), async () => [], noop, noop, async () => ({}), argsIn, {})
  return { out, prompts }
}

const buildersOn = (prompts, tag) => prompts.filter((p) => p.label.startsWith(`build:${tag}:`)).map((p) => p.label.split(':')[2])
const buildOf = (prompts, id) => prompts.find((p) => p.label.startsWith('build:') && p.label.endsWith(`:${id}`)).prompt

test('lanes:3 — per-part round-robin in file order, the lane:A pin wins and the round-robin skips past it (±1)', async () => {
  const { out, prompts } = await run({ mode: 'build', lanes: 3 })
  // pins are counted first: A starts at 1, so the first two unpinned items go to B and C
  assert.deepEqual(out.lanes.A, ['T-F1', 'T-PIN', 'T-F4'])
  assert.deepEqual(out.lanes.B, ['T-F0a', 'T-F2'])
  assert.deepEqual(out.lanes.C, ['T-F0b', 'T-F3'])
  assert.ok(buildersOn(prompts, 'A').includes('T-PIN'), 'the pinned item is built on lane A')
  assert.ok(!out.steals.some((x) => x.itemId === 'T-PIN'), 'a pinned item is never stolen')
  const c = prompts.find((p) => p.label === 'build:C:T-F0b').prompt
  assert.match(c, /wilson-voice-loop\/lane-c/)
  assert.match(c, /CARGO_TARGET_DIR=\/Users\/wilsonguenther\/code\/wilson-voice-loop\/target-c/)
  assert.match(c, /PREVIEW PORT\s+5275/)
  const recon = prompts.find((p) => p.label === 'recon' || /^RECON for the Yap/.test(p.prompt)).prompt
  assert.match(recon, /git worktree add \/Users\/wilsonguenther\/code\/wilson-voice-loop\/lane-c -b yap\/recon-c origin\/main/)
})

test('lanes:2 (default) is balanced ±1 and creates no lane C; lanes:1 puts every item on lane A', async () => {
  const two = await run({ mode: 'build' })
  assert.deepEqual(two.out.lanes.A, ['T-F0b', 'T-F2', 'T-PIN', 'T-F4'])
  assert.deepEqual(two.out.lanes.B, ['T-F0a', 'T-F1', 'T-F3'])
  assert.deepEqual(two.out.lanes.C, [])
  assert.ok(!two.prompts.some((p) => /^(build|preflight):C:/.test(p.label)), 'no lane C dispatch at lanes:2 (lane C never steals)')
  assert.equal(two.out.landBar, 'review')
  const recon = two.prompts.find((p) => /^RECON for the Yap/.test(p.prompt)).prompt
  assert.doesNotMatch(recon, /lane-c -b yap\/recon-c/)
  const one = await run({ mode: 'build', lanes: 1 })
  assert.deepEqual(one.out.lanes.A, ITEM_DEFS.map((d) => d.id))
  assert.deepEqual(one.out.lanes.B, [])
  assert.deepEqual(one.out.lanes.C, [])
  assert.deepEqual(one.out.steals, [])
})

test('STEAL: a lane whose items are all already-done takes the NEXT UNSTARTED unpinned item of the longest lane', async () => {
  const { out, prompts } = await run({ mode: 'build', lanes: 3 }, { alreadyDone: ['T-F0b', 'T-F3'] })
  assert.ok(out.steals.length >= 1, 'lane C ran dry and must steal')
  const s = out.steals.find((x) => x.to === 'C')
  assert.ok(s, 'lane C stole something')
  const plan = out.lanes[s.from]
  const at = prompts.findIndex((p) => p.label === `preflight:C:${s.itemId}`)
  assert.ok(at > 0, 'the stolen item is pre-flighted and built on lane C')
  const startedBefore = (id) => prompts.slice(0, at).some((p) => p.label.endsWith(`:${id}`))
  const stealable = plan.filter((id) => id !== 'T-PIN')
  const k = stealable.indexOf(s.itemId)
  assert.ok(k >= 0)
  assert.ok(stealable.slice(0, k).every(startedBefore), 'every earlier item of the victim lane had started')
  assert.ok(!startedBefore(s.itemId), 'the stolen item had not started anywhere')
  // every runnable item ran exactly once
  const pre = prompts.filter((p) => p.label.startsWith('preflight:')).map((p) => p.label.split(':')[2]).sort()
  assert.deepEqual(pre, ITEM_DEFS.map((d) => d.id).sort())
})

test('REAL item set: every generated part is balanced ±1 per lane for lanes 2 and 3 (with and without the panel)', async () => {
  const GEN = path.join(HERE, 'generated')
  const files = fs.readdirSync(GEN).filter((f) => /^part-\d+\.mjs$/.test(f)).sort()
  assert.equal(files.length, 5)
  let total = 0
  for (const f of files) {
    const src = fs.readFileSync(path.join(GEN, f), 'utf8')
    const ids = Object.keys(JSON.parse(src.match(/^const LANE_BY_ID = (.*)$/m)[1]))
    total += ids.length
    for (const extra of [{}, { panelApproved: ids }]) {
      for (const lanes of [2, 3]) {
        const { out } = await run({ mode: 'build', lanes, ...extra }, { src })
        const counts = ['A', 'B', 'C'].slice(0, lanes).map((t) => out.lanes[t].length)
        assert.ok(Math.max(...counts) - Math.min(...counts) <= 1, `${f} lanes:${lanes} ${JSON.stringify(extra).slice(0, 20)} split ${counts.join('/')}`)
        if (out.lanes.A.length + out.lanes.B.length + out.lanes.C.length && ids.includes('yap-telemetry-diagnostics') && extra.panelApproved) {
          assert.ok(out.lanes.A.includes('yap-telemetry-diagnostics'), 'the lane:A pin is honoured on the real set')
        }
      }
    }
  }
  assert.equal(total, 177)
})

test("landBar:'ci' — the drain is the lander: gh pr merge on CI green, loop-ci-red, no review chain, no parking", async () => {
  const { out, prompts } = await run({ mode: 'build', lanes: 3, landBar: 'ci' })
  assert.equal(out.landBar, 'ci')
  const drain = prompts.find((p) => p.label === 'drain').prompt
  assert.match(drain, /gh pr merge -R wilsonguenther-dev\/wilson-voice <n> --squash --delete-branch/)
  assert.match(drain, /--match-head-commit <headRefOid>/)
  assert.match(drain, /gate-ledger\.mjs ci --sha <headRefOid>/)
  assert.match(drain, /loop-ci-red/)
  assert.match(drain, /--force-with-lease/)
  assert.match(drain, /wilson-voice-loop\/pre-review/)
  assert.match(drain, /OLDEST PR NUMBER FIRST/)
  for (const banned of ['review-opus', 'needs-human', 'review-gate.mjs', 'THE TEN BLOCKING COMMANDS MUST EXIT 0']) {
    assert.ok(!drain.includes(banned), `the ci drain prompt must not contain "${banned}"`)
  }
  // builders merge only CI-green PRs and never hand out the review gate in this mode
  const b = buildOf(prompts, 'T-F0a')
  assert.match(b, /LAND FIRST — CI-GREEN ONLY/)
  assert.ok(!b.includes('review-gate.mjs gate'), 'ci-mode builder LAND must not run the review gate')
  // the last part tears down lane C too
  assert.match(drain, /rm -rf \S+target-a \S+target-b \S+target-c/)
})

test('the default (review bar) drain is unchanged in kind: it still runs the review gate and never says loop-ci-red', async () => {
  const { prompts } = await run({ mode: 'build' })
  const drain = prompts.find((p) => p.label === 'drain').prompt
  assert.match(drain, /review-gate\.mjs gate/)
  assert.ok(!drain.includes('loop-ci-red'))
  const b = buildOf(prompts, 'T-F0a')
  assert.ok(!b.includes('CI-GREEN ONLY'))
})

test('the TELEMETRY (Wilson 2026-10-27) block is in the opus builder prompt and the Sonnet builder preamble', async () => {
  const { prompts } = await run({ mode: 'build', lanes: 3, landBar: 'ci' })
  const opus = buildOf(prompts, 'T-F0a')
  const sonnet = buildOf(prompts, 'T-F1')
  assert.match(sonnet, /^# SONNET BUILDER CONTRACT/)
  for (const p of [opus, sonnet]) {
    assert.match(p, /# TELEMETRY \(Wilson 2026-10-27\)/)
    assert.match(p, /desktop\/src-tauri\/src\/logging\.rs/)
    assert.match(p, /toast\(\) in\s+desktop\/src\/appShell\.ts/)
    assert.match(p, /## Telemetry/)
  }
  const preamble = sonnet.slice(0, sonnet.indexOf('You are a SENIOR RUST'))
  assert.match(preamble, /TELEMETRY \(Wilson 2026-10-27\)/, 'the Sonnet preamble itself must name the telemetry block (it says "no scope beyond the spec")')
})

test('the telemetry item exists, is pinned to lane A, opus-built and ungated', () => {
  const src = fs.readFileSync(path.join(HERE, 'items', '00b-yap-telemetry-2026-10-27.mjs'), 'utf8')
  const ITEMS = []
  new Function('ITEMS', src)(ITEMS)
  const it = ITEMS.find((i) => i.id === 'yap-telemetry-diagnostics')
  assert.ok(it)
  assert.equal(it.lane, 'A')
  assert.equal(it.gated, null)
  assert.equal(it.builder, undefined)
  assert.match(it.preflight, /diagnostics\.rs/)
  assert.match(it.acceptance, /cargo test --features custom-protocol --lib diagnostics/)
})
