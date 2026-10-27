/**
 * scripts/loop/stall-detector.test.mjs — the loop notices its own stalls (item yap-loop-stall-detector).
 * Hermetic: reads the repo's template.mjs / stall.mjs, runs stall-alarm.sh in bash against a temp file.
 *     node --test scripts/loop/stall-detector.test.mjs
 */
import { test } from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import {
  GATE_BLOCKING,
  GATE_ADVISORY,
  stallReason,
  sameReasonTwice,
  gateRedOnMain,
  isGateFailure,
  noProgress,
} from './stall.mjs'

const HERE = import.meta.dirname
const TEMPLATE = fs.readFileSync(path.join(HERE, 'template.mjs'), 'utf8')
const STALL_SRC = fs.readFileSync(path.join(HERE, 'stall.mjs'), 'utf8')
const BEGIN = '// STALL-RULES-BEGIN'
const END = '// STALL-RULES-END'

/** Everything from `start` up to (not including) the first `stop` after it. */
function between(src, start, stop) {
  const s = src.indexOf(start)
  assert.ok(s >= 0, `missing ${JSON.stringify(start)}`)
  const e = src.indexOf(stop, s + start.length)
  assert.ok(e > s, `missing ${JSON.stringify(stop)} after ${JSON.stringify(start)}`)
  return src.slice(s, e)
}
function block(src) {
  return between(src, BEGIN, END) + END
}
/** The array literal a `const NAME = [...]` declares in the template, parsed. */
function templateArray(name) {
  const m = new RegExp(`^const ${name} = (\\[[^\\]]*\\])`, 'm').exec(TEMPLATE)
  assert.ok(m, `template.mjs must declare const ${name} = [...]`)
  return JSON.parse(m[1].replace(/'/g, '"'))
}
const ALL_GREEN = Object.fromEntries([...GATE_BLOCKING, ...GATE_ADVISORY].map((k) => [k, 0]))

test('gate_flags_match_gate_cmds', () => {
  // The template's two arrays are the same as stall.mjs's, and sit immediately above GATE_CMDS.
  assert.deepEqual(templateArray('GATE_BLOCKING'), GATE_BLOCKING)
  assert.deepEqual(templateArray('GATE_ADVISORY'), GATE_ADVISORY)
  assert.ok(TEMPLATE.indexOf('const GATE_BLOCKING =') < TEMPLATE.indexOf('const GATE_CMDS = (dir, target) =>'))
  // Every echo label in GATE_CMDS is classified, and every classified name is echoed: both ways.
  const cmds = between(TEMPLATE, 'const GATE_CMDS = (dir, target) => `', '\n`\n')
  const labels = [...cmds.matchAll(/echo "([A-Za-z]+)=\$\?/g)].map((m) => m[1])
  assert.ok(labels.length > 0, 'no echo "<name>=$?" labels found in GATE_CMDS — the extractor is broken')
  const classified = [...GATE_BLOCKING, ...GATE_ADVISORY]
  for (const l of labels) assert.ok(classified.includes(l), `GATE_CMDS echoes ${l}= but it is in neither GATE_BLOCKING nor GATE_ADVISORY`)
  for (const c of classified) assert.ok(labels.includes(c), `${c} is classified but GATE_CMDS never echoes ${c}=`)
  assert.equal(new Set(labels).size, labels.length, 'a GATE_CMDS label is echoed twice')
  // The BLOCKING-COMMANDS prose carries the count of GATE_BLOCKING, never a retyped number.
  assert.match(cmds, /THE \$\{GATE_BLOCKING\.length\} BLOCKING COMMANDS/)
  assert.doesNotMatch(cmds, /ALL ELEVEN MUST EXIT 0/)
  // fmt (advisory) is run but never read as a conjunct.
  assert.match(cmds, /echo "fmt=\$\? \(ADVISORY\)"/)
})

test('recon_prompts_interpolate_gate_flags', () => {
  const build = between(TEMPLATE, 'const BUILD_RECON = `', 'const REVIEW_RECON = `')
  const review = between(TEMPLATE, 'const REVIEW_RECON = `', 'const RECON_SCHEMA = {')
  for (const [name, p] of [['BUILD_RECON', build], ['REVIEW_RECON', review]]) {
    assert.ok(p.includes('${GATE_CMDS('), `${name} must interpolate GATE_CMDS(...)`)
    assert.ok(p.includes('${GATE_BLOCKING.join('), `${name} must interpolate GATE_BLOCKING, not retype the names`)
    assert.ok(p.includes('${GATE_ADVISORY.join('), `${name} must interpolate GATE_ADVISORY, not retype the names`)
    assert.match(p, /you do not decide what is informational/i)
    assert.match(p, /gateExits/)
  }
  assert.ok(!TEMPLATE.includes('Plus the informational one'), 'the recon-only fmt line is back')
  const schema = between(TEMPLATE, 'const RECON_SCHEMA = {', '\nconst recon = await')
  assert.match(schema, /gateExits: \{/)
  assert.match(schema, /required: \['ciMode', 'gateExits'\]/)
  assert.match(schema, /required: \[\.\.\.GATE_BLOCKING, \.\.\.GATE_ADVISORY\]/)
})

test('stall_rules_block_is_identical_to_stall_mjs', () => {
  assert.equal(TEMPLATE.split(BEGIN).length - 1, 1, 'template.mjs must carry exactly one STALL-RULES-BEGIN')
  assert.equal(TEMPLATE.split(END).length - 1, 1, 'template.mjs must carry exactly one STALL-RULES-END')
  assert.equal(block(TEMPLATE), block(STALL_SRC), 'the STALL-RULES block in template.mjs drifted from stall.mjs')
  assert.ok(!/^import /m.test(block(STALL_SRC)) && !/^import /m.test(STALL_SRC), 'stall.mjs must stay import-free')
  // And the rules are WIRED, not just copied: each is called outside the block.
  const outside = TEMPLATE.replace(block(TEMPLATE), '')
  for (const call of ['stallReason(', 'sameReasonTwice(landHist', 'gateRedOnMain(exits, GATE_BLOCKING, GATE_ADVISORY)', 'noProgress(landResults, 3)', 'isGateFailure(', 'if (stalled) break']) {
    assert.ok(outside.includes(call), `template.mjs never calls ${call}`)
  }
  // Rule 1 is reachable ACROSS passes (a PR is landed once per pass): the land seat leaves a "loop-land:" marker,
  // triage reads it back as lastLand, reviewOnePr seeds the history from it, and loop-stalled is HELD by triage.
  assert.ok(outside.includes('loop-land: status=<the status you return> reason='), 'the land seat never writes the loop-land marker')
  assert.ok(outside.includes('lastLand: { type:'), 'TRIAGE_SCHEMA never carries lastLand')
  assert.ok(outside.includes('entry.lastLand'), 'reviewOnePr never seeds the history from lastLand')
  const isHeld = new Function(`return ${TEMPLATE.match(/const isHeld = (\(q\) => [^\n]+)/)[1]}`)()
  assert.equal(isHeld({ pr: 5, labels: [{ name: 'loop-stalled' }] }), true, 'triage does not hold loop-stalled PRs')
  assert.equal(isHeld({ pr: 6, labels: ['loop-build'] }), false)
})

/**
 * END TO END: execute the GENERATED review pass (scripts/loop/generated/part-01.mjs) against stubbed globals, the way
 * review-gate.test.mjs's chain tests do, so the rules are proven WIRED in what the Workflow actually runs.
 * recon's gateExits, the triage queue and every LAND return are scripted; every other seat gets a benign PASS.
 */
const GREEN = { typecheck: 0, test: 0, build: 0, polish: 0, diarize: 0, stage: 0, polishtest: 0, diarizetest: 0, fmt: 0, clippy: 0, cargotest: 0 }
const HEAD = 'a'.repeat(40)
async function runGenerated({ reconExits = GREEN, queue = [], land = () => ({ status: 'merged', landed: true }) } = {}) {
  const src = fs.readFileSync(path.join(HERE, 'generated', 'part-01.mjs'), 'utf8').replace(/^export const meta/m, 'const meta')
  const benign = () => new Proxy(function () {}, {
    get(_t, p) {
      if (p === 'then' || p === 'toJSON' || p === 'length') return p === 'length' ? 0 : undefined
      if (p === Symbol.iterator) return function* () {}
      if (p === Symbol.toPrimitive) return () => ''
      if (typeof p === 'symbol') return undefined
      return benign()
    },
    apply: () => benign(),
  })
  const generic = (extra = {}) => new Proxy({ status: 'ok', ok: true, halted: false, exits: [0], allZero: true, verdict: 'PASS', blocking: [], advisory: [], prs: [], items: [], result: {}, ...extra },
    { get: (t, p) => (p in t ? t[p] : p === 'then' || p === 'toJSON' || typeof p === 'symbol' ? undefined : benign()) })
  const seats = []
  const logs = []
  const prompts = []
  const agent = async (prompt, opts = {}) => {
    const label = String(opts.label || '')
    seats.push(label)
    if (label === 'review-triage') return { queue, closed: [], held: [] }
    if (/^recon/.test(label) || /recon/.test(label)) return generic({ ciMode: 'local', gateExits: reconExits })
    if (label.startsWith('stall-writer')) { prompts.push(String(prompt)); return { logged: true } }
    if (label.startsWith('review-opus')) return { status: 'reviewed', verdict: 'PASS', blocking: [], advisory: [], recordHead: HEAD, recordPosted: true }
    if (label.startsWith('fix-land:') && /You are the LAND agent/.test(String(prompt))) return land(Number((String(prompt).match(/This PR is #(\d+)/) || [])[1]))
    return generic()
  }
  const FIXED = 1757808000000
  class FrozenDate extends Date { constructor(...a) { super(...(a.length ? a : [FIXED])) } static now() { return FIXED } }
  const boom = () => { throw new Error('no outside world in a stall e2e test') }
  const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor
  const fn = new AsyncFunction('agent', 'parallel', 'pipeline', 'phase', 'log', 'workflow', 'args', 'budget', 'Date', 'Math', 'fetch', 'process', 'require', src)
  await fn(agent, async (fs_) => Promise.all(fs_.map((f) => f())), async (fs_) => { const o = []; for (const f of fs_) o.push(await f()); return o },
    () => {}, (m) => logs.push(String(m)), async () => ({ halted: false, parts: [], items: [] }), { mode: 'review' },
    new Proxy({ remaining: 1e9, used: 0, limit: 1e9 }, { get: (t, p) => (p in t ? t[p] : benign()) }), FrozenDate, Object.create(Math, { random: { value: () => 0.42 } }),
    boom, new Proxy({}, { get: boom }), boom)
  return { seats, logs, prompts }
}
const qEntry = (pr, extra = {}) => ({ pr, item: `X-${pr}`, title: `X-${pr}: a change`, headRefName: `loop/x-${pr}`, klass: 'standard', ci: 'none', mergeable: 'MERGEABLE', files: 1, additions: 1, labels: ['loop-build'], ...extra })

const ciRed = (pr, cmd) => ({ pr, ...stallReason({ status: 'ci-red', gateExits: { ...ALL_GREEN, [cmd]: 1 } }) })

test('same_reason_twice_ci_red_same_command_stalls', async () => {
  const first = ciRed(101, 'clippy')
  assert.deepEqual(first, { pr: 101, status: 'ci-red', reason: 'clippy' })
  assert.equal(sameReasonTwice([], first), false, 'the first failure is never a stall')
  assert.equal(sameReasonTwice([first], ciRed(101, 'clippy')), true)
  // fmt is ADVISORY: a ci-red whose only non-zero is fmt falls through to failingCommand/reason, never 'fmt'.
  assert.equal(stallReason({ status: 'ci-red', gateExits: { ...ALL_GREEN, fmt: 1 }, failingCommand: 'test' }).reason, 'test')
  assert.equal(stallReason({ status: 'ci-red' }).reason, 'unknown')
  assert.equal(stallReason({ status: 'needs-rebase', reason: 'CONFLICTING' }).reason, 'CONFLICTING')
  assert.equal(stallReason({ status: 'ci-pending' }).reason, 'none')
  // END TO END, across passes: last pass's land marker said ci-red/clippy, this pass's LAND is ci-red on clippy again
  // -> ONE stall-writer seat (label, comment, log, board), outcome loop-stalled, and no further fix-land for that PR.
  const red = { status: 'ci-red', landed: false, failingCommand: 'clippy', gateExits: { ...ALL_GREEN, clippy: 101 } }
  const r = await runGenerated({ queue: [qEntry(41, { lastLand: { status: 'ci-red', reason: 'clippy' } })], land: () => red })
  assert.deepEqual(r.seats.filter((l) => l.startsWith('stall-writer')), ['stall-writer:X-41'], r.logs.join('\n'))
  assert.ok(r.logs.some((l) => l.startsWith('STALL pr=41 item=X-41 status=ci-red reason=clippy')), r.logs.join('\n'))
  assert.match(r.prompts[0], /--add-label loop-stalled/)
  assert.match(r.prompts[0], /loop-stalled: ci-red clippy twice in a row/)
  assert.match(r.prompts[0], /review:needs-human 'STALL: clippy/)
  assert.equal(r.seats.filter((l) => l === 'fix-land:X-41').length, 1, 'exactly one LAND, never a third attempt')
  // ...and a DIFFERENT prior reason (or none) never stalls.
  const ok = await runGenerated({ queue: [qEntry(42, { lastLand: { status: 'ci-red', reason: 'test' } })], land: () => red })
  assert.deepEqual(ok.seats.filter((l) => l.startsWith('stall-writer')), [], ok.logs.join('\n'))
})

test('same_reason_twice_different_command_does_not_stall', () => {
  assert.equal(sameReasonTwice([ciRed(101, 'clippy')], ciRed(101, 'cargotest')), false)
  // Only the MOST RECENT prior entry counts: clippy, cargotest, clippy is not "twice in a row".
  assert.equal(sameReasonTwice([ciRed(101, 'clippy'), ciRed(101, 'cargotest')], ciRed(101, 'clippy')), false)
})

test('same_reason_twice_different_pr_does_not_stall', () => {
  assert.equal(sameReasonTwice([ciRed(101, 'clippy')], ciRed(102, 'clippy')), false)
  assert.equal(sameReasonTwice([ciRed(101, 'clippy'), ciRed(102, 'test')], ciRed(101, 'clippy')), true, 'other PRs in between do not reset a PR')
})

test('same_reason_twice_never_on_merged', () => {
  const merged = { pr: 101, ...stallReason({ status: 'merged', reason: 'landed' }) }
  assert.equal(sameReasonTwice([merged], { ...merged }), false)
})

test('gate_red_on_main_flags_blocking_nonzero', async () => {
  const v = gateRedOnMain({ ...ALL_GREEN, clippy: 101, test: 1 }, GATE_BLOCKING, GATE_ADVISORY)
  assert.deepEqual(v, { red: ['test', 'clippy'], unreadable: [], advisoryRed: [] })
  assert.deepEqual(gateRedOnMain(ALL_GREEN, GATE_BLOCKING, GATE_ADVISORY), { red: [], unreadable: [], advisoryRed: [] })
  assert.deepEqual(gateRedOnMain({ ...ALL_GREEN, build: '2' }, GATE_BLOCKING, GATE_ADVISORY).red, ['build'], 'a numeric string from a text payload is readable')
  // END TO END: recon reports clippy red on main -> the generated review pass halts BEFORE triage, writes the stall
  // (board, log, STALL.txt) once, and dispatches no reviewer, LAND, sweeper or verify seat.
  const r = await runGenerated({ reconExits: { ...ALL_GREEN, clippy: 101 }, queue: [qEntry(43)] })
  assert.ok(!r.seats.includes('review-triage'), `triage ran on a red main: ${r.seats.join(', ')}`)
  assert.deepEqual(r.seats.filter((l) => l.startsWith('stall-writer')), ['stall-writer:_stall'])
  assert.match(r.prompts[0], /GATE RED ON MAIN: clippy/)
  assert.match(r.prompts[0], /STALL\.txt/)
  assert.ok(r.logs.some((l) => l.startsWith('review pass HALTED (gate-red-on-main): red=[clippy]')), r.logs.join('\n'))
  // advisory fmt red on main is a log line, never a halt
  const fmtRed = await runGenerated({ reconExits: { ...ALL_GREEN, fmt: 1 }, queue: [] })
  assert.ok(fmtRed.seats.includes('review-triage'))
  assert.ok(fmtRed.logs.some((l) => /advisory red on main: fmt/.test(l)), fmtRed.logs.join('\n'))
})

test('gate_red_on_main_ignores_advisory_fmt', () => {
  const v = gateRedOnMain({ ...ALL_GREEN, fmt: 1 }, GATE_BLOCKING, GATE_ADVISORY)
  assert.deepEqual(v, { red: [], unreadable: [], advisoryRed: ['fmt'] })
  // A missing advisory key is not a halt either.
  const { fmt, ...noFmt } = ALL_GREEN
  assert.deepEqual(gateRedOnMain(noFmt, GATE_BLOCKING, GATE_ADVISORY), { red: [], unreadable: [], advisoryRed: [] })
  assert.equal(fmt, 0)
})

test('gate_red_on_main_missing_key_is_unreadable_and_red', () => {
  const { clippy, ...noClippy } = ALL_GREEN
  assert.equal(clippy, 0)
  const v = gateRedOnMain({ ...noClippy, cargotest: 'n/a', test: 0.5 }, GATE_BLOCKING, GATE_ADVISORY)
  assert.deepEqual(v.red, [])
  assert.deepEqual(v.unreadable, ['test', 'clippy', 'cargotest'])
  // No payload at all (a recon seat that omitted gateExits): every blocking command is unreadable.
  assert.deepEqual(gateRedOnMain(undefined, GATE_BLOCKING, GATE_ADVISORY).unreadable, GATE_BLOCKING)
})

const landed = (pr, status, exits) => ({ pr, status, gateFailure: isGateFailure({ status, gateExits: exits }) })

test('no_progress_three_distinct_prs_gate_failures_halts', async () => {
  const results = [landed(1, 'ci-red'), landed(2, 'ci-red'), landed(3, 'ci-pending', { ...ALL_GREEN, cargotest: 101 })]
  assert.equal(results[2].gateFailure, true, 'a non-zero BLOCKING exit is a gate failure even without ci-red')
  assert.equal(noProgress(results), true)
  assert.equal(noProgress(results.slice(0, 2)), false, 'fewer than 3 results never halts')
  assert.equal(noProgress([landed(1, 'ci-red'), landed(2, 'ci-red'), landed(3, 'needs-rebase')]), false, 'a non-gate failure is not no-progress')
  assert.equal(landed(4, 'ci-pending', { ...ALL_GREEN, fmt: 1 }).gateFailure, false, 'advisory fmt is never a gate failure')
  // END TO END: five queued PRs on the two review chains, every LAND ci-red on a different command -> after the third
  // result the barrier stops pulling: the PR the OTHER chain already had in flight finishes (54), nothing new is taken
  // (55 is never landed), ONE stall-writer writes the no-progress line, and the sweeper/verify still run.
  const cmds = { 51: 'clippy', 52: 'test', 53: 'cargotest', 54: 'build', 55: 'typecheck' }
  const r = await runGenerated({
    queue: [qEntry(51), qEntry(52), qEntry(53), qEntry(54), qEntry(55)],
    land: (pr) => ({ status: 'ci-red', landed: false, failingCommand: cmds[pr], gateExits: { ...ALL_GREEN, [cmds[pr]]: 1 } }),
  })
  assert.equal(r.seats.filter((l) => l.endsWith(':X-55')).length, 0, `a PR was pulled after no-progress: ${r.seats.join(', ')}`)
  assert.ok(r.logs.some((l) => /STOPPED by the no-progress stall rule/.test(l)), r.logs.join('\n'))
  assert.deepEqual(r.seats.filter((l) => l.startsWith('stall-writer')), ['stall-writer:_stall'], r.logs.join('\n'))
  // Which three depends on how the two chains interleave; it is always three DISTINCT pr:command pairs from the script.
  const m = /STALL no-progress: last 3 land results were gate failures: (\d+:\w+), (\d+:\w+), (\d+:\w+)/.exec(r.prompts[0])
  assert.ok(m, r.prompts[0])
  const pairs = m.slice(1)
  assert.equal(new Set(pairs.map((x) => x.split(':')[0])).size, 3)
  for (const x of pairs) assert.equal(cmds[x.split(':')[0]], x.split(':')[1], x)
  assert.ok(r.seats.includes('main-verify'), `main-verify must still run: ${r.seats.join(', ')}`)
})

test('no_progress_same_pr_three_times_does_not_count', () => {
  assert.equal(noProgress([landed(7, 'ci-red'), landed(7, 'ci-red'), landed(7, 'ci-red')]), false)
  assert.equal(noProgress([landed(7, 'ci-red'), landed(8, 'ci-red'), landed(7, 'ci-red')]), false)
})

test('no_progress_a_merge_in_the_window_resets', () => {
  assert.equal(noProgress([landed(1, 'ci-red'), landed(2, 'merged'), landed(3, 'ci-red')]), false)
  assert.equal(noProgress([landed(1, 'ci-red'), landed(2, 'merged'), landed(3, 'ci-red'), landed(4, 'ci-red')]), false)
  assert.equal(noProgress([landed(1, 'ci-red'), landed(2, 'merged'), landed(3, 'ci-red'), landed(4, 'ci-red'), landed(5, 'ci-red')]), true)
})

test('stall_alarm_prints_alert_then_cleared', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'stall-alarm-'))
  const file = path.join(dir, 'STALL.txt')
  // One bash process, one stall_alert variable, four ticks separated by a marker line.
  const script = [
    `. ${JSON.stringify(path.join(HERE, 'stall-alarm.sh'))}`,
    'stall_alert=0',
    'stall_alarm_check; echo "--tick1"',
    `printf '2026-10-27T00:00:00Z\\nrule 2\\nGATE RED ON MAIN: clippy\\n' > "$STALL_FILE"`,
    'stall_alarm_check; echo "--tick2"',
    'stall_alarm_check; echo "--tick3"',
    'mv "$STALL_FILE" "$STALL_FILE.acked"',
    'stall_alarm_check; echo "--tick4"',
  ].join('\n')
  try {
    const r = spawnSync('bash', ['-c', script], { env: { ...process.env, STALL_FILE: file }, encoding: 'utf8' })
    assert.equal(r.status, 0, r.stderr)
    const [t1, t2, t3, t4] = r.stdout.split(/--tick\d\n/)
    assert.equal(t1, '', 'absent file -> no output')
    const alert = t2.trim().split('\n')
    assert.equal(alert.length, 1, `exactly one line on the rising edge, got ${JSON.stringify(t2)}`)
    assert.match(alert[0], /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ ALERT STALL 2026-10-27T00:00:00Z \| rule 2 \| GATE RED ON MAIN: clippy$/)
    assert.equal(t3, '', 'second tick with the file still there -> silent (edge-triggered)')
    assert.match(t4.trim(), /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ cleared STALL$/)
    assert.equal(t4.trim().split('\n').length, 1)
  } finally {
    fs.rmSync(dir, { recursive: true, force: true })
  }
})
