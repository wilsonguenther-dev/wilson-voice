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
