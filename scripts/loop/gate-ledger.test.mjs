// scripts/loop/gate-ledger.test.mjs — node --test. HERMETIC (temp ledger, stub git/gh; no cargo, no network).
// Pins the gate-once-per-head rule (2026-10-27): the full gate ran 4–7× per PR because nothing remembered that a sha
// had already been gated. A record answers for its exact 40-hex sha only, and only when every blocking command is in it.
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import {
  recordGate, gateFor, readLedger, isComplete, isGreen, parseCodes, cli, worktreeProblem, ciCheckFor, ledgerPromptProblems,
  BLOCKING_GATE_CMDS, EXIT_NONE, EXIT_RED, EXIT_PENDING, EXIT_USAGE, GITHUB_CI_RUNNER, CI_JOB_NAME, FULL_GATE_NEEDLE,
} from './gate-ledger.mjs'

const SHA = 'a'.repeat(40)
const OTHER = 'b'.repeat(40)
const tmpLedger = () => path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'gate-ledger-')), 'gate-ledger.json')
const allZero = () => Object.fromEntries(BLOCKING_GATE_CMDS.map((k) => [k, 0]))
const CODES = 'typecheck=0 test=0 build=0 polish=0 diarize=0 stage=0 polishtest=0 diarizetest=0 fmt=1 clippy=0 cargotest=0'
const cleanGit = (head = SHA, dirty = '') => (args) => (args.includes('rev-parse') ? `${head}\n` : dirty)

test('record then lookup: the newest complete record for the exact sha answers', () => {
  const ledger = tmpLedger()
  assert.equal(gateFor(SHA, { ledger }), null, 'empty ledger -> none')
  recordGate(SHA, { ...allZero(), cargotest: 101 }, { runner: 'fix-land:X', worktree: '/w', stamp: 't1' }, { ledger })
  assert.equal(isGreen(gateFor(SHA, { ledger })), false, 'a red record is found and is red')
  recordGate(SHA, { ...allZero(), fmt: 1 }, { runner: 'fix-land:X', worktree: '/w', stamp: 't2' }, { ledger })
  const rec = gateFor(SHA, { ledger })
  assert.equal(rec.stamp, 't2')
  assert.equal(rec.runner, 'fix-land:X')
  assert.equal(rec.worktree, '/w')
  assert.equal(isGreen(rec), true, 'fmt is advisory: fmt=1 alone stays green')
  assert.equal(readLedger({ ledger }).length, 2, 'append-only: both lines kept')
  assert.equal(fs.readFileSync(ledger, 'utf8').trim().split('\n').length, 2, 'JSON lines')
})

test('invalid when a blocking command is missing', () => {
  const ledger = tmpLedger()
  for (const k of BLOCKING_GATE_CMDS) {
    const codes = allZero(); delete codes[k]
    recordGate(SHA, codes, { runner: 'r', stamp: k }, { ledger })
    assert.equal(isComplete({ codes }), false, `${k} missing`)
  }
  assert.equal(gateFor(SHA, { ledger }), null, 'no incomplete record ever satisfies the gate')
})

test('sha mismatch -> none: other sha, short prefix, garbage', () => {
  const ledger = tmpLedger()
  recordGate(SHA, allZero(), { runner: 'r', stamp: 's' }, { ledger })
  assert.equal(gateFor(OTHER, { ledger }), null)
  assert.equal(gateFor(SHA.slice(0, 9), { ledger }), null, 'no prefix match')
  assert.equal(gateFor('', { ledger }), null)
  assert.equal(gateFor(SHA.toUpperCase(), { ledger }).sha, SHA, 'case-insensitive on the full sha')
  assert.throws(() => recordGate('abc123', allZero(), { runner: 'r' }, { ledger }), /40-hex/)
  assert.throws(() => recordGate(SHA, allZero(), { runner: '' }, { ledger }), /runner/)
})

test('a torn line is skipped, never fatal', () => {
  const ledger = tmpLedger()
  fs.writeFileSync(ledger, '{"sha":"' + SHA + '","codes":{"typecheck"\n')
  recordGate(SHA, allZero(), { runner: 'r', stamp: 'ok' }, { ledger })
  assert.equal(gateFor(SHA, { ledger }).stamp, 'ok')
})

test('parseCodes reads the gate echo lines', () => {
  assert.deepEqual(parseCodes('typecheck=0\ntest=1, fmt=1 (ADVISORY)\nfmtHunks=65'), { typecheck: 0, test: 1, fmt: 1, fmtHunks: 65 })
})

test('CLI record: refuses a worktree off the sha or dirty; writes on a clean tree; check reports it', () => {
  const ledger = tmpLedger()
  const base = ['--sha', SHA, '--runner', 'fix-land:X', '--worktree', '/w', '--codes', CODES, '--ledger', ledger]
  assert.equal(cli({ argv: ['record', ...base], git: cleanGit(OTHER) }).code, EXIT_USAGE, 'HEAD moved')
  const dirty = cli({ argv: ['record', ...base], git: cleanGit(SHA, ' M desktop/src/x.ts\n') })
  assert.equal(dirty.code, EXIT_USAGE); assert.match(dirty.out, /modified tracked files/)
  assert.equal(cli({ argv: ['record', '--sha', SHA, '--runner', 'r', '--codes', CODES, '--ledger', ledger], git: cleanGit() }).code, EXIT_USAGE, 'no worktree')
  assert.equal(cli({ argv: ['record', ...base.slice(0, 2), '--runner', GITHUB_CI_RUNNER, ...base.slice(4)], git: cleanGit() }).code, EXIT_USAGE, 'github-ci is ci-only')
  assert.equal(cli({ argv: ['check', '--sha', SHA, '--ledger', ledger] }).code, EXIT_NONE, 'refusals wrote nothing')
  const ok = cli({ argv: ['record', ...base], git: cleanGit(), now: () => '2026-10-27T14:00:00Z' })
  assert.equal(ok.code, 0, ok.out)
  const chk = cli({ argv: ['check', '--sha', SHA, '--ledger', ledger] })
  assert.equal(chk.code, 0)
  assert.equal(chk.out, `gate: satisfied by record 2026-10-27T14:00:00Z from fix-land:X (worktree /w) on ${SHA.slice(0, 9)}`)
  assert.equal(cli({ argv: ['check', '--sha', OTHER, '--ledger', ledger] }).code, EXIT_NONE, 'a new push (new sha) has no record')
  assert.equal(cli({ argv: ['check', '--sha', 'abc', '--ledger', ledger] }).code, EXIT_USAGE)
})

test('CLI check: a red record exits 1 and names the command; an incomplete record write is flagged', () => {
  const ledger = tmpLedger()
  const red = cli({ argv: ['record', '--sha', SHA, '--runner', 'land', '--worktree', '/w', '--codes', CODES.replace('clippy=0', 'clippy=101'), '--ledger', ledger], git: cleanGit() })
  assert.equal(red.code, 0); assert.match(red.out, /RED \(clippy\)/)
  const chk = cli({ argv: ['check', '--sha', SHA, '--ledger', ledger] })
  assert.equal(chk.code, EXIT_RED); assert.match(chk.out, /failed: clippy/)
  const part = cli({ argv: ['record', '--sha', OTHER, '--runner', 'land', '--worktree', '/w', '--codes', 'typecheck=0', '--ledger', ledger], git: cleanGit(OTHER) })
  assert.equal(part.code, EXIT_USAGE); assert.match(part.out, /INCOMPLETE/)
  assert.equal(cli({ argv: ['check', '--sha', OTHER, '--ledger', ledger] }).code, EXIT_NONE)
})

const checkRuns = (runs) => (args) => {
  assert.match(args.join(' '), /^api repos\/wilsonguenther-dev\/wilson-voice\/commits\/[0-9a-f]{40}\/check-runs/)
  return JSON.stringify({ check_runs: runs })
}
const run = (o) => ({ id: 7, name: CI_JOB_NAME, head_sha: SHA, status: 'completed', conclusion: 'success', started_at: '2026-10-27T13:00:00Z', html_url: 'https://x/7', ...o })

test('CLI ci: green CI on the exact head writes a github-ci record and satisfies check', () => {
  const ledger = tmpLedger()
  const r = cli({ argv: ['ci', '--sha', SHA, '--ledger', ledger], gh: checkRuns([run()]), now: () => 'T' })
  assert.equal(r.code, 0, r.out); assert.match(r.out, /from github-ci/)
  const rec = gateFor(SHA, { ledger })
  assert.equal(rec.runner, GITHUB_CI_RUNNER); assert.equal(isGreen(rec), true)
})

test('CLI ci: pending, failure, wrong sha, other job, skipped -> no record', () => {
  const ledger = tmpLedger()
  const ci = (runs) => cli({ argv: ['ci', '--sha', SHA, '--ledger', ledger], gh: checkRuns(runs) })
  assert.equal(ci([run({ status: 'in_progress', conclusion: null })]).code, EXIT_PENDING)
  assert.equal(ci([]).code, EXIT_PENDING)
  assert.equal(ci([run({ head_sha: OTHER })]).code, EXIT_PENDING, 'green on another sha proves nothing')
  assert.equal(ci([run({ name: 'release' })]).code, EXIT_PENDING, 'another job proves nothing')
  assert.equal(ci([run({ conclusion: 'skipped' })]).code, EXIT_PENDING, 'skipped is not evidence')
  assert.equal(ci([run({ conclusion: 'failure' })]).code, EXIT_RED)
  assert.equal(ci([run({ conclusion: 'success', started_at: 'a' }), run({ conclusion: 'failure', started_at: 'b' })]).code, EXIT_RED, 'newest attempt decides')
  assert.equal(cli({ argv: ['ci', '--sha', SHA, '--ledger', ledger], gh: () => { throw new Error('gh down') } }).code, EXIT_PENDING)
  assert.equal(gateFor(SHA, { ledger }), null)
  assert.equal(ciCheckFor({ sha: SHA, gh: checkRuns([run()]) }).state, 'success')
})

test('worktreeProblem fails closed when git cannot read the tree', () => {
  assert.match(worktreeProblem('/nope', SHA, () => { throw new Error('not a git repo') }), /cannot read/)
  assert.equal(worktreeProblem('/w', SHA, cleanGit()), '')
})

test('ledgerPromptProblems: reviewer needs the check and never the full gate; a full-gate prompt writes its record', () => {
  const ok = [
    { seat: 'review-opus', text: 'node x/gate-ledger.mjs check --sha H' },
    { seat: 'fix-land', text: `${FULL_GATE_NEEDLE} ... gate-ledger.mjs record --sha` },
  ]
  assert.deepEqual(ledgerPromptProblems(ok), [])
  assert.equal(ledgerPromptProblems([{ seat: 'review-opus2', text: 'read the diff' }]).length, 1)
  assert.equal(ledgerPromptProblems([{ seat: 'review-opus', text: `gate-ledger.mjs check ${FULL_GATE_NEEDLE} gate-ledger.mjs record` }]).length, 1)
  assert.equal(ledgerPromptProblems([{ seat: 'land-sweeper', text: FULL_GATE_NEEDLE }]).length, 1)
})
