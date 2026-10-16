/**
 * scripts/loop/seat-models.test.mjs — seat model tiering (loop/model-tiering, 2026-10-16; port of
 * drivia-consulting 888ba2b13). The repo root has no node_modules, so this runs on node:test:
 *     node --test scripts/loop/seat-models.test.mjs
 *
 * Two halves: the build-time validator/stamper (seat-models.mjs) against the REAL template and map,
 * and the runtime agentR block sliced out of template.mjs and run against a stubbed agentRRetry.
 */
import { test } from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { seatModelProblems, stampSeatModels, OPUS_FLOOR } from './seat-models.mjs'

const HERE = import.meta.dirname
const TEMPLATE = fs.readFileSync(path.join(HERE, 'template.mjs'), 'utf8')
const MAP_FILE = path.join(HERE, 'seat-models.json')
const REAL_MAP = JSON.parse(fs.readFileSync(MAP_FILE, 'utf8'))
const STAMP_LINE = /^const SEAT_MODELS = \{\}.*$/m

/** The tiering block: from the SEAT TIERING banner up to (not including) `const HALT_PATTERNS`. */
function sliceBlock(template) {
  const start = template.indexOf('// ── SEAT TIERING')
  const end = template.indexOf('const HALT_PATTERNS')
  assert.ok(start > 0 && end > start, 'template must hold the SEAT TIERING block before const HALT_PATTERNS')
  return template.slice(start, end)
}

/** Run the block with `map` stamped in, a stubbed agentRRetry that answers from `answers`. */
function load(map, { args, answers = [], isStalled } = {}) {
  const block = sliceBlock(TEMPLATE).replace(STAMP_LINE, `const SEAT_MODELS = ${JSON.stringify(map)}`)
  const calls = []
  const logs = []
  const agentRRetry = async (prompt, opts, maxAttempts = 3) => {
    calls.push({ prompt, opts, maxAttempts })
    return answers.length ? answers.shift() : null
  }
  const mod = new Function('args', 'log', 'agentRRetry', 'isStalled', block + '\nreturn { agentR, seatTier, tierProblem }')(
    args,
    (m) => logs.push(m),
    agentRRetry,
    isStalled
  )
  return { ...mod, calls, logs }
}

function withTempMap(map, fn) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'seat-models-'))
  const file = path.join(dir, 'seat-models.json')
  try {
    if (map !== undefined) fs.writeFileSync(file, JSON.stringify(map))
    return fn(file)
  } finally {
    fs.rmSync(dir, { recursive: true, force: true })
  }
}

const MAP = {
  enabled: true,
  escalate: { model: 'opus', effort: 'high' },
  seats: {
    acceptance: { model: 'sonnet', effort: 'medium', require: ['exits', 'allZero'] },
    'record-failure': { model: 'haiku', effort: 'low', require: ['boardWritten'] },
  },
}
const OPUS_OPTS = { model: 'opus', effort: 'low', phase: 'x', label: 'acceptance:A:yap24-01' }

test('1. the real map validates against the real template and stamps exactly once', () => {
  assert.deepEqual(seatModelProblems(REAL_MAP, TEMPLATE), [])
  const r = stampSeatModels(TEMPLATE, MAP_FILE)
  assert.deepEqual(r.problems, [])
  assert.equal((r.template.match(/^const SEAT_MODELS = \{"enabled":true/gm) || []).length, 1)
  assert.equal((r.template.match(/^const SEAT_MODELS = \{\}/gm) || []).length, 0)
  assert.match(r.summary, /^6 mechanical seat\(s\) tiered \(.*\), escalate→opus$/)
  // The stamped block still evaluates.
  const block = sliceBlock(r.template)
  assert.doesNotThrow(() => new Function('args', 'log', 'agentRRetry', 'isStalled', block + '\nreturn { agentR }'))
})

test('2. floor seats can never be tiered, even when dropped from opusOnly', () => {
  for (const seat of ['build', 'review-opus', 'review-opus2', 'fix-land', 'drain', 'land-sweeper']) {
    assert.ok(OPUS_FLOOR.includes(seat), `${seat} is in OPUS_FLOOR`)
    const map = { enabled: true, escalate: { model: 'opus' }, opusOnly: [], seats: { [seat]: { model: 'sonnet' } } }
    const problems = seatModelProblems(map, TEMPLATE)
    assert.ok(problems.some((p) => p.includes(`"${seat}" is an opus-only seat`)), `${seat}: ${problems.join(' | ')}`)
  }
})

test('3. a non-opus escalate, a gemini model and a typo seat are refused', () => {
  const esc = seatModelProblems({ ...MAP, escalate: { model: 'sonnet' } }, TEMPLATE)
  assert.ok(esc.some((p) => p.includes('escalate.model must be "opus"')))
  const gem = seatModelProblems({ ...MAP, seats: { reflect: { model: 'gemini' } } }, TEMPLATE)
  assert.ok(gem.some((p) => p.includes('seats.reflect.model must be one of opus|sonnet|haiku')))
  const typo = seatModelProblems({ ...MAP, seats: { reflekt: { model: 'sonnet' } } }, TEMPLATE)
  assert.ok(typo.some((p) => p.includes('seat "reflekt" matches no agent label in the template')))
})

test('4. a missing map stamps nothing; a template without the line fails', () => {
  withTempMap(undefined, (file) => {
    const r = stampSeatModels(TEMPLATE, file)
    assert.deepEqual(r.problems, [])
    assert.equal(r.template, TEMPLATE)
    assert.match(r.summary, /every seat opus/)
  })
  withTempMap(MAP, (file) => {
    const r = stampSeatModels(TEMPLATE.replace(STAMP_LINE, '// no stamp line'), file)
    assert.ok(r.problems.some((p) => p.includes('exactly once to be stamped (found 0)')))
  })
})

test('5. an untiered seat passes through with its options untouched', async () => {
  const m = load(MAP, { answers: [{ ok: true }] })
  const opts = { model: 'opus', effort: 'high', phase: 'p', label: 'build:A:yap24-01' }
  const r = await m.agentR('BUILD PROMPT', opts)
  assert.deepEqual(r, { ok: true })
  assert.equal(m.calls.length, 1)
  assert.equal(m.calls[0].prompt, 'BUILD PROMPT')
  assert.equal(m.calls[0].opts, opts)
  assert.equal(m.calls[0].maxAttempts, 3)
})

test('6. a tiered seat runs once on its small model with the contract appended', async () => {
  const ok = { exits: [{ command: 'npm test', exit: 0 }], allZero: true, output: 'ok' }
  const m = load(MAP, { answers: [ok] })
  const r = await m.agentR('RUN THESE', OPUS_OPTS)
  assert.equal(r, ok)
  assert.equal(m.calls.length, 1)
  assert.equal(m.calls[0].opts.model, 'sonnet')
  assert.equal(m.calls[0].opts.effort, 'medium')
  assert.equal(m.calls[0].opts.label, OPUS_OPTS.label)
  assert.equal(m.calls[0].maxAttempts, 1)
  assert.ok(m.calls[0].prompt.startsWith('RUN THESE'))
  assert.match(m.calls[0].prompt, /# MECHANICAL SEAT CONTRACT \(acceptance on sonnet — loop\/model-tiering\)/)
  assert.equal(m.logs.length, 0)
})

test('7. NEEDS_OPUS re-runs the SAME prompt on opus without the contract', async () => {
  const opusAnswer = { exits: [{ command: 'npm test', exit: 1 }], allZero: false, output: 'red' }
  const m = load(MAP, { answers: [{ firstFailure: 'NEEDS_OPUS: dirty tree', exits: [], allZero: false }, opusAnswer] })
  const r = await m.agentR('RUN THESE', OPUS_OPTS)
  assert.equal(r, opusAnswer)
  assert.equal(m.calls.length, 2)
  assert.equal(m.calls[1].prompt, 'RUN THESE')
  assert.equal(m.calls[1].opts.model, 'opus')
  assert.equal(m.calls[1].opts.effort, 'high')
  assert.equal(m.calls[1].maxAttempts, 3)
  assert.match(m.logs[0], /^\[tier\] acceptance:A:yap24-01: sonnet attempt 1\/2 returned NEEDS_OPUS — escalating to opus$/)
})

test('8. a result missing a require field escalates', async () => {
  const m = load(MAP, { answers: [{ pr: 12, labelled: true }, { pr: 12, boardWritten: true }] })
  const r = await m.agentR('RECORD IT', { model: 'opus', effort: 'low', phase: 'p', label: 'record-failure:A:yap24-01' })
  assert.deepEqual(r, { pr: 12, boardWritten: true })
  assert.equal(m.calls.length, 2)
  assert.equal(m.calls[0].opts.model, 'haiku')
  assert.equal(m.calls[1].opts.model, 'opus')
  assert.equal(m.calls[1].prompt, 'RECORD IT')
  assert.match(m.logs[0], /result is missing "boardWritten" — escalating to opus/)
})

test('9. two nulls run sonnet, sonnet, then opus (maxAttempts 3); three nulls return null', async () => {
  const ok = { exits: [{ command: 'x', exit: 0 }], allZero: true, output: '' }
  const m = load(MAP, { answers: [null, null, ok] })
  assert.equal(await m.agentR('RUN', OPUS_OPTS), ok)
  assert.deepEqual(m.calls.map((c) => [c.opts.model, c.maxAttempts]), [['sonnet', 1], ['sonnet', 1], ['opus', 3]])
  assert.match(m.logs[0], /attempt 1\/2 null result — retrying on sonnet$/)
  assert.match(m.logs[1], /attempt 2\/2 null result — escalating to opus$/)
  const n = load(MAP, { answers: [null, null, null] })
  assert.equal(await n.agentR('RUN', OPUS_OPTS), null)
  assert.equal(n.calls.length, 3)
})

test('10. a stalled small seat is returned after exactly one call', async () => {
  const stalled = { stalled: true }
  const m = load(MAP, { answers: [stalled], isStalled: (r) => !!(r && r.stalled) })
  assert.equal(await m.agentR('RUN', OPUS_OPTS), stalled)
  assert.equal(m.calls.length, 1)
})

test('11. both kill switches make seatTier return null', () => {
  assert.ok(load(MAP).seatTier('acceptance:A:yap24-01'))
  assert.equal(load({ ...MAP, enabled: false }).seatTier('acceptance:A:yap24-01'), null)
  assert.equal(load(MAP, { args: { seatModels: false } }).seatTier('acceptance:A:yap24-01'), null)
})
