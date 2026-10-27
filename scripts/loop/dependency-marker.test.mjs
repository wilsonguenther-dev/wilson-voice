// node --test scripts/loop/dependency-marker.test.mjs
// loop-dependency-marker-stdout-only (walk D7): the harness's dependency hold reads the marker ONLY as a line
// an EXECUTED pre-flight command printed to stdout — never the echoed command string, never the runner
// seat's own prose. dependencyMarkerPrinted lives in template.mjs (a Workflow script body, not a node
// module: build.mjs wraps it in a function, so it cannot carry an `export`), so it is lifted out of the
// real template source and run here, the same way review-gate.test.mjs runs isHeld.
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'

const HERE = path.dirname(new URL(import.meta.url).pathname)
const TPL = fs.readFileSync(path.join(HERE, 'template.mjs'), 'utf8')
const MARK = ['DEPENDENCY', 'NOT', 'ON', 'MAIN'].join('-')

function lift() {
  const m = TPL.match(/\nconst dependencyMarkerPrinted = (\(pre\) => \{[\s\S]*?\n\})\n/)
  assert.ok(m, 'template.mjs defines `const dependencyMarkerPrinted = (pre) => { ... }`')
  return new Function(`return ${m[1]}`)()
}

// The REAL runner return from the run that skipped yap-p0-perm-repair on main 3569ce4 (workflow
// wf_9ac3986e, 2026-10-19T02:54Z), verbatim. The command printed NOTHING; the marker text sat in
// exits[0].command (a then-literal echo) and in the seat's own prose in `output`.
const REAL_3569CE4 = {
  kind: 'preflight',
  itemId: 'yap-p0-perm-repair',
  headSha: '3569ce4',
  exits: [
    {
      command: `( M=0; test -f desktop/src-tauri/tests/hotkey_no_tcc.rs || M=1; grep -q 'DICTATE_TOGGLE.shortcut()' desktop/src-tauri/src/lib.rs || M=1; grep -q 'HotkeySource::Carbon' desktop/src-tauri/src/lib.rs || M=1; if [ "$M" = 1 ]; then echo "${MARK} ..."; exit 0; fi; test -f desktop/src-tauri/src/perm_probe.rs && grep -q 'StaleIdentity' desktop/src-tauri/src/perm_probe.rs && grep -q 'pub fn repair_argv' desktop/src-tauri/src/perm_probe.rs && grep -q 'repair-plan' scripts/qa/perm-probe.sh )`,
      exit: 0,
    },
  ],
  allZero: true,
  firstFailure: '',
  output: `git fetch origin exit=0; git checkout --detach origin/main exit=0 (HEAD is now at 3569ce4).\nCommand printed nothing, exit=0. No ${MARK} message printed. Note: HEAD already contains the perm-repair item (#228) on origin/main.`,
}

test('dependencyMarkerPrinted: marker line in an executed command\'s stdout -> true', () => {
  const f = lift()
  const pre = { allZero: true, exits: [{ command: 'true', exit: 0, stdout: '' }, { command: '( ... )', exit: 0, stdout: `${MARK}: yap-lic-00 has not landed — land it, then the build pass again\n` }], output: '' }
  assert.equal(f(pre), true)
  // a later line of multi-line stdout still counts (it is a LINE the command printed)
  assert.equal(f({ exits: [{ command: 'x', exit: 0, stdout: `checking...\n${MARK}: dep missing` }] }), true)
  // a JSON string return (agent returns may be strings) is parsed, not regexed whole
  assert.equal(f(JSON.stringify(pre)), true)
})

test('dependencyMarkerPrinted: marker only in a notes/summary field -> false', () => {
  const f = lift()
  for (const field of ['output', 'firstFailure', 'notes', 'summary']) {
    const pre = { allZero: true, exits: [{ command: 'true', exit: 0, stdout: '' }], [field]: `${MARK}: printed by nobody\nNo ${MARK} message printed` }
    assert.equal(f(pre), false, field)
  }
  // the real 3569ce4 payload: the false hold that skipped yap-p0-perm-repair
  assert.equal(f(REAL_3569CE4), false)
  assert.equal(f(JSON.stringify(REAL_3569CE4)), false)
})

test('dependencyMarkerPrinted: marker text inside the echoed command string -> false', () => {
  const f = lift()
  const pre = { allZero: true, exits: [{ command: `echo "${MARK}: x"; exit 0`, exit: 0, stdout: '' }], output: '' }
  assert.equal(f(pre), false)
  // stdout that merely QUOTES the marker mid-line (not a line it starts) is not the marker either
  assert.equal(f({ exits: [{ command: 'x', exit: 0, stdout: `+ echo "${MARK}: x"` }] }), false)
  // the bare marker without its colon is not the marker
  assert.equal(f({ exits: [{ command: 'x', exit: 0, stdout: MARK }] }), false)
})

test('dependencyMarkerPrinted: malformed returns are never a hold', () => {
  const f = lift()
  for (const v of [null, undefined, '', 'not json', `${MARK}: raw string`, {}, { exits: 'nope' }, { exits: [null, { stdout: 7 }] }]) {
    assert.equal(f(v), false, JSON.stringify(v))
  }
})

test('the OLD whole-JSON test is what tripped on 3569ce4 (documents the root cause)', () => {
  assert.equal(new RegExp(MARK).test(JSON.stringify(REAL_3569CE4)), true)
  assert.equal(new RegExp(MARK).test(REAL_3569CE4.exits[0].command), true)
  assert.equal(new RegExp(MARK).test(REAL_3569CE4.output), true)
})

test('runBuild uses dependencyMarkerPrinted(pre), the whole-JSON test is gone, and the runner schema carries per-command stdout', () => {
  assert.ok(TPL.includes('if (runPassed(pre) && dependencyMarkerPrinted(pre)) {'))
  assert.ok(!TPL.includes(`/${MARK}/.test(asText(pre))`))
  assert.ok(!new RegExp(`/${MARK}/\\.test\\(`).test(TPL), 'no regex test of the marker anywhere but inside the helper')
  assert.match(TPL, /properties: \{ command: \{ type: 'string' \}, exit: \{ type: 'number' \}, stdout: \{ type: 'string' \} \},\n\s*required: \['command', 'exit', 'stdout'\]/)
  assert.ok(TPL.includes('exits: [{command, exit, stdout}, ...]'), 'the runner prompt asks for per-command stdout')
})

// ── PR #266 fix round 1 (independent review BLOCKING 1): the hold must FAIL CLOSED ─────────────────────
// A hold exits 0, so a false from dependencyMarkerPrinted used to fall straight into the already-done
// branch — a terminal "win" for an item whose dependency is missing. dependencyHoldInconclusive turns
// every return that cannot PROVE the marker absent into a non-terminal skip instead.
function liftInconclusive() {
  const m = TPL.match(/\nconst dependencyHoldInconclusive = (\(pre, preflight\) => \{[\s\S]*?\n\})\n/)
  assert.ok(m, 'template.mjs defines `const dependencyHoldInconclusive = (pre, preflight) => { ... }`')
  return new Function(`return ${m[1]}`)()
}
// The REAL hold-carrying preflight of yap-p0-perm-repair (items/07-yap-p0-broken-2026-10-17.mjs), read
// from the item file so the test tracks what the loop actually runs.
const ITEM07 = fs.readFileSync(path.join(HERE, 'items', '07-yap-p0-broken-2026-10-17.mjs'), 'utf8')
const HOLD_PREFLIGHT = ITEM07.split('\n').find((l) => l.includes('DEPENDENCY-NOT-ON-$(printf MAIN): yap-p0-hotkeys-no-permission'))
const LINE = `${MARK}: yap-p0-hotkeys-no-permission has not landed — run the review pass to land it, then the build pass again`

// Mirror of runBuild's branch order on a PASSING preflight (runPassed(pre) === true for every case).
function disposition(pre, preflight) {
  if (lift()(pre)) return 'skipped: dependency not on main'
  if (liftInconclusive()(pre, preflight)) return 'skipped: dependency hold inconclusive'
  return 'already-done'
}

test('fail-closed: every case the independent reviewer probed (dependency MISSING) is never already-done', () => {
  assert.ok(HOLD_PREFLIGHT, 'item 07 still carries the assembled hold')
  const base = { kind: 'preflight', itemId: 'yap-p0-perm-repair', allZero: true, firstFailure: '' }
  const cases = {
    'A stdout omitted (pre-#266 shape), marker in output': { ...base, exits: [{ command: '( ... )', exit: 0 }], output: LINE },
    'B stdout "" , marker in output': { ...base, exits: [{ command: '( ... )', exit: 0, stdout: '' }], output: LINE },
    'C prompt prefix on the marker line': { ...base, exits: [{ command: '( ... )', exit: 0, stdout: `$ ${LINE}` }], output: '' },
    'D 4-space indent': { ...base, exits: [{ command: '( ... )', exit: 0, stdout: `    ${LINE}` }], output: '' },
    'E non-JSON string return': `allZero: true\n${LINE}`,
  }
  for (const [name, pre] of Object.entries(cases)) {
    assert.notEqual(disposition(pre, HOLD_PREFLIGHT), 'already-done', name)
    assert.equal(disposition(pre, HOLD_PREFLIGHT), 'skipped: dependency hold inconclusive', name)
  }
  // F verbatim stdout: the real hold
  assert.equal(disposition({ ...base, exits: [{ command: '( ... )', exit: 0, stdout: `${LINE}\n` }], output: LINE }, HOLD_PREFLIGHT), 'skipped: dependency not on main')
  // malformed returns of a hold-carrying item are inconclusive, never a win
  for (const v of [null, undefined, '', 'not json', {}, { exits: 'nope' }, { exits: [] }, { exits: [null] }, { exits: [{ command: 'x', exit: 0, stdout: 7 }] }]) {
    assert.equal(disposition(v, HOLD_PREFLIGHT), 'skipped: dependency hold inconclusive', JSON.stringify(v))
  }
})

test('fail-closed keeps the 3569ce4 fix: dependency PRESENT, verbatim empty stdout -> already-done', () => {
  // The same false-hold payload as REAL_3569CE4, in the new schema (stdout: "" because the command printed
  // nothing). The marker sits in exits[0].command (excluded) and in prose WITHOUT its colon.
  const fixed = { ...REAL_3569CE4, exits: REAL_3569CE4.exits.map((e) => ({ ...e, stdout: '' })) }
  assert.equal(disposition(fixed, HOLD_PREFLIGHT), 'already-done')
  assert.equal(disposition(JSON.stringify(fixed), HOLD_PREFLIGHT), 'already-done')
})

test('fail-closed never touches an item without a hold in its preflight', () => {
  const f = liftInconclusive()
  for (const pf of ['test -f desktop/src-tauri/src/lib.rs', '', undefined, null]) {
    for (const v of [null, 'not json', { exits: [{ command: 'x', exit: 0 }] }, { exits: [{ command: 'x', exit: 0, stdout: `  ${LINE}` }] }]) {
      assert.equal(f(v, pf), false, `${pf} / ${JSON.stringify(v)}`)
    }
  }
})

test('runBuild checks the inconclusive hold AFTER the dependency hold and BEFORE already-done', () => {
  const dep = TPL.indexOf('if (runPassed(pre) && dependencyMarkerPrinted(pre)) {')
  const inc = TPL.indexOf('if (runPassed(pre) && dependencyHoldInconclusive(pre, item.preflight)) {')
  const done = TPL.indexOf("return { itemId: item.id, status: 'already-done', lane: tag, preflight: pre }")
  assert.ok(dep > 0 && inc > dep && done > inc, `order dep=${dep} inc=${inc} done=${done}`)
  assert.ok(TPL.includes("status: 'skipped: dependency hold inconclusive'"))
})

// Fix round 2 (review BLOCKING): the item must RETIRE once it lands. runBuild retires an item only when its
// EXECUTED preflight passes, so this runs the item's real preflight (lifted from its item file) against this
// tree, from the repo root, exactly as the runner seat does. Before round 2 the preflight grepped
// review-gate.test.mjs, which never names the helper, so it exited 1 on the merged tree and every build pass
// re-dispatched a builder to an already-landed item.
test('loop-dependency-marker-stdout-only preflight passes on this tree (the item retires after it merges)', async () => {
  const { spawnSync } = await import('node:child_process')
  const ROOT = path.resolve(HERE, '..', '..')
  const src = fs.readFileSync(path.join(HERE, 'items', '07b-yap-walk-defects-2026-10-19.mjs'), 'utf8')
  const m = src.match(/id: 'loop-dependency-marker-stdout-only'[\s\S]*?preflight: `([\s\S]*?)`/)
  assert.ok(m, 'the item and its preflight are found in 07b')
  const preflight = m[1].trim()
  assert.ok(preflight.length > 0)
  const r = spawnSync('sh', ['-c', preflight], { cwd: ROOT, encoding: 'utf8' })
  assert.equal(r.status, 0, `preflight exited ${r.status}: ${preflight}\n${r.stdout}${r.stderr}`)
  // Every file the preflight greps must exist (a grep on a missing file is exit 2, not a retirement).
  for (const f of preflight.match(/scripts\/loop\/[\w./-]+\.mjs/g) || []) {
    assert.ok(fs.existsSync(path.join(ROOT, f)), `${f} exists`)
  }
})
