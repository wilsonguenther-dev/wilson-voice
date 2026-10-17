// scripts/loop/review-gate.test.mjs — node --test. HERMETIC: gh is a stub fed from fixture JSON through the
// injectable runner (cli({gh})), never the network. Cases (a)–(i) are the yap-land-gate-pass-required spec.
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import {
  cli, parseRecord, recordBody, securityClassOf, promptUnits, promptGateProblems,
  EXIT_REVIEW, EXIT_USAGE, EXIT_BAD_FLAG, DEFAULT_REPO, RECORD_MARKER,
} from './review-gate.mjs'

const HERE = path.dirname(new URL(import.meta.url).pathname)
const HEAD = 'a'.repeat(40)
const OLD = 'b'.repeat(40)
const T0 = '2026-10-17T10:00:00Z'
let nextId = 1

/** A fixture PR. files default to a standard (non-security) frontend file. */
const fixture = ({ head = HEAD, files = ['desktop/src/App.tsx'], labels = [], title = 'yap-x: standard change', comments = [] } = {}) =>
  ({ head, files, labels, title, comments, ci: 'success', mergeable: 'MERGEABLE' })

/** The record a seat posts, as GitHub returns it in issues/N/comments. */
const rec = ({ seat = 'review-opus', head = HEAD, verdict = 'PASS', blocking = 0, at = T0, edited = false, login = 'wilsonguenther-dev', type = 'User' } = {}) => ({
  id: nextId++,
  user: { login, type },
  created_at: at,
  updated_at: edited ? '2026-10-17T11:00:00Z' : at,
  body: `${RECORD_MARKER}\nseat: ${seat}\nlens: test lens\nhead: ${head}\nverdict: ${verdict} blocking=${blocking}\n\nnotes`,
})

/** The stub gh: answers exactly the calls review-gate.mjs makes, from the fixture; anything else throws. */
function stubGh(fx) {
  const calls = []
  const gh = (args) => {
    calls.push(args)
    const a = args.join(' ')
    if (/^pr view \d+ -R \S+ --json labels,headRefOid,title$/.test(a)) return JSON.stringify({ labels: fx.labels.map((name) => ({ name })), headRefOid: fx.head, title: fx.title })
    if (/^pr view \d+ -R \S+ --json headRefOid$/.test(a)) return JSON.stringify({ headRefOid: fx.head })
    if (/^api repos\/\S+\/pulls\/\d+\/files\?per_page=100 --paginate --jq \.\[\]\.filename$/.test(a)) return fx.files.join('\n') + '\n'
    if (/^api repos\/\S+\/issues\/\d+\/comments\?per_page=100&page=1$/.test(a)) return JSON.stringify(fx.comments)
    if (/^pr comment \d+ -R \S+ --body-file \S+$/.test(a)) { fx.posted = fs.readFileSync(args[args.length - 1], 'utf8'); return '' }
    throw new Error(`stub gh: unexpected call: gh ${a}`)
  }
  return { gh, calls }
}

const gate = (fx, head = fx.head) => { const { gh } = stubGh(fx); return cli({ argv: ['gate', '--pr', '7', '--head', head], gh, env: {} }) }

test('(a) CI green, MERGEABLE, no record on any head -> 6 (there is no unreviewed exception)', () => {
  const fx = fixture()
  assert.equal(fx.ci, 'success'); assert.equal(fx.mergeable, 'MERGEABLE')
  const r = gate(fx)
  assert.equal(r.code, EXIT_REVIEW, r.out)
  assert.match(r.out, /missing/)
})

test('(b) standard PR, one review-opus PASS on the head -> 0', () => {
  const r = gate(fixture({ comments: [rec()] }))
  assert.equal(r.code, 0, r.out)
  assert.match(r.out, /PASS on aaaaaaaaa by review-opus \(standard\)/)
})

test('(b2) standard PR, one review-opus2 PASS on the head -> 0 (either seat)', () => {
  assert.equal(gate(fixture({ comments: [rec({ seat: 'review-opus2' })] })).code, 0)
})

test('(c) PASS on an older head only (the #196 shape) -> 6', () => {
  const r = gate(fixture({ comments: [rec({ head: OLD })] }))
  assert.equal(r.code, EXIT_REVIEW, r.out)
  assert.match(r.out, /older head/)
})

test('(d) security PR: review-opus only -> 6; two review-opus PASSes -> 6; review-opus + review-opus2 -> 0', () => {
  const sec = { files: ['desktop/src-tauri/capabilities/default.json'] }
  const one = gate(fixture({ ...sec, comments: [rec()] }))
  assert.equal(one.code, EXIT_REVIEW, one.out)
  assert.match(one.out, /no review-opus2 PASS/)
  const twice = gate(fixture({ ...sec, comments: [rec(), rec({ at: '2026-10-17T10:05:00Z' })] }))
  assert.equal(twice.code, EXIT_REVIEW, twice.out)
  const both = gate(fixture({ ...sec, comments: [rec(), rec({ seat: 'review-opus2' })] }))
  assert.equal(both.code, 0, both.out)
  assert.match(both.out, /security/)
})

test('(e) a fix-land seat record saying PASS on the head (the #178 shape) -> 6', () => {
  const r = gate(fixture({ comments: [rec({ seat: 'fix-land' })] }))
  assert.equal(r.code, EXIT_REVIEW, r.out)
  // and every other non-reviewer seat is not a record either
  for (const seat of ['fix', 'drain', 'land-sweeper', 'land', 'build', 'whoever']) {
    assert.equal(gate(fixture({ comments: [rec({ seat })] })).code, EXIT_REVIEW, seat)
  }
})

test('(f) newest review-opus2 record on the head is BLOCK -> 6 (even with an older PASS from it and a review-opus PASS)', () => {
  const r = gate(fixture({ comments: [rec(), rec({ seat: 'review-opus2' }), rec({ seat: 'review-opus2', verdict: 'BLOCK', blocking: 2, at: '2026-10-17T10:30:00Z' })] }))
  assert.equal(r.code, EXIT_REVIEW, r.out)
  assert.match(r.out, /BLOCK on aaaaaaaaa by review-opus2/)
})

test('(g) an edited PASS is void -> 6', () => {
  const r = gate(fixture({ comments: [rec({ edited: true })] }))
  assert.equal(r.code, EXIT_REVIEW, r.out)
})

test('(g2) not a record: a bot author, a foreign author, PASS with blocking>0, a short head', () => {
  assert.equal(gate(fixture({ comments: [rec({ login: 'codex[bot]', type: 'Bot' })] })).code, EXIT_REVIEW)
  assert.equal(gate(fixture({ comments: [rec({ login: 'someone-else' })] })).code, EXIT_REVIEW)
  assert.equal(gate(fixture({ comments: [rec({ blocking: 1 })] })).code, EXIT_REVIEW)
  const short = rec(); short.body = short.body.replace(`head: ${HEAD}`, 'head: aaaaaaa')
  assert.equal(gate(fixture({ comments: [short] })).code, EXIT_REVIEW)
  assert.deepEqual(parseRecord(short.body).malformed !== undefined, true)
})

test('gate: --head that is not the current head (pushed after the PASS) -> 6, fails closed on a gh error', () => {
  const r = gate(fixture({ comments: [rec({ head: OLD })] }), OLD)
  assert.equal(r.code, EXIT_REVIEW, r.out)
  assert.match(r.out, /head moved/)
  const broken = cli({ argv: ['gate', '--pr', '7'], gh: () => { throw new Error('HTTP 502') }, env: {} })
  assert.equal(broken.code, EXIT_REVIEW)
  assert.match(broken.out, /fails closed/)
})

const SEC = { files: ['desktop/src-tauri/capabilities/default.json'] }
const LATER = '2026-10-17T10:30:00Z'

test('B1: an edited BLOCK fails closed — both PASSes + a newer edited review-opus2 BLOCK -> 6 (security and standard)', () => {
  const comments = () => [rec(), rec({ seat: 'review-opus2' }), rec({ seat: 'review-opus2', verdict: 'BLOCK', blocking: 1, at: LATER, edited: true })]
  const sec = gate(fixture({ ...SEC, comments: comments() }))
  assert.equal(sec.code, EXIT_REVIEW, sec.out)
  assert.match(sec.out, /BLOCK on aaaaaaaaa by review-opus2 \(edited — fails closed\)/)
  const std = gate(fixture({ comments: comments() }))
  assert.equal(std.code, EXIT_REVIEW, std.out)
  assert.match(std.out, /BLOCK/)
})

test('B1: an edited BLOCK is superseded only by a fresh unedited record from the same seat on the same head', () => {
  const r = gate(fixture({ comments: [rec({ seat: 'review-opus2', verdict: 'BLOCK', blocking: 1, edited: true }), rec({ seat: 'review-opus2', at: LATER })] }))
  assert.equal(r.code, 0, r.out)
  // an edited PASS after it is void and supersedes nothing
  const v = gate(fixture({ comments: [rec({ seat: 'review-opus2', verdict: 'BLOCK', blocking: 1 }), rec({ seat: 'review-opus2', at: LATER, edited: true }), rec()] }))
  assert.equal(v.code, EXIT_REVIEW, v.out)
})

test('B2: a BLOCK under any seat name counts — review-opus PASS + newer review-opus-h1 BLOCK -> 6; both PASSes + h1 BLOCK (security) -> 6', () => {
  const std = gate(fixture({ comments: [rec(), rec({ seat: 'review-opus-h1', verdict: 'BLOCK', blocking: 2, at: LATER })] }))
  assert.equal(std.code, EXIT_REVIEW, std.out)
  assert.match(std.out, /BLOCK on aaaaaaaaa by review-opus-h1/)
  const sec = gate(fixture({ ...SEC, comments: [rec(), rec({ seat: 'review-opus2' }), rec({ seat: 'review-opus-h1', verdict: 'BLOCK', blocking: 1, at: LATER })] }))
  assert.equal(sec.code, EXIT_REVIEW, sec.out)
  // a fixer-seat BLOCK blocks too; an unlisted-seat PASS still never passes and never supersedes
  assert.equal(gate(fixture({ comments: [rec(), rec({ seat: 'fix-land', verdict: 'BLOCK', blocking: 1, at: LATER })] })).code, EXIT_REVIEW)
  assert.equal(gate(fixture({ comments: [rec({ seat: 'review-opus-h1' })] })).code, EXIT_REVIEW)
  assert.equal(gate(fixture({ comments: [rec(), rec({ seat: 'review-opus-h1', verdict: 'BLOCK', blocking: 1 }), rec({ seat: 'review-opus-h1', at: LATER })] })).code, EXIT_REVIEW)
})

test('B2: a malformed BLOCK fails closed — no blocking=, blocking=0, junk, no seat line, short head, no head', () => {
  const mk = (edit) => { const c = rec({ verdict: 'BLOCK', blocking: 1, at: LATER }); c.body = edit(c.body); return [rec({ seat: 'review-opus2' }), c] }
  const cases = {
    'no blocking=': (b) => b.replace('verdict: BLOCK blocking=1', 'verdict: BLOCK'),
    'blocking=0': (b) => b.replace('blocking=1', 'blocking=0'),
    'junk blocking': (b) => b.replace('blocking=1', 'blocking=two'),
    'lowercase': (b) => b.replace('verdict: BLOCK blocking=1', 'verdict: block'),
    'two verdicts': (b) => b.replace('verdict: BLOCK blocking=1', 'verdict: PASS blocking=0\nverdict: BLOCK blocking=1'),
    'no seat line': (b) => b.replace('seat: review-opus\n', ''),
    'short head': (b) => b.replace(`head: ${HEAD}`, 'head: aaaaaaa'),
    'no head line': (b) => b.replace(`head: ${HEAD}\n`, ''),
  }
  for (const [name, edit] of Object.entries(cases)) {
    const r = gate(fixture({ comments: mk(edit) }))
    assert.equal(r.code, EXIT_REVIEW, `${name}: ${r.out}`)
    assert.match(r.out, /BLOCK/, name)
  }
  assert.equal(parseRecord(mk(cases['no blocking='])[1].body).blocking, 1)
  // a short head binds only to the head it prefixes: a BLOCK on bbbbbbb does not block aaaa...
  assert.equal(gate(fixture({ comments: mk((b) => b.replace(`head: ${HEAD}`, 'head: bbbbbbb')) })).code, 0)
})

test('advisory: --head with no value -> 2 (never the current head); a repo not owned by the pinned owner is refused', () => {
  const fx = fixture({ comments: [rec()] })
  for (const argv of [['gate', '--pr', '7', '--head'], ['gate', '--pr', '7', '--head', '--repo', DEFAULT_REPO], ['gate', '--pr', '7', '--head', '']]) {
    const r = cli({ argv, gh: stubGh(fx).gh, env: {} })
    assert.equal(r.code, EXIT_BAD_FLAG, `${argv.join(' ')}: ${r.out}`)
  }
  assert.equal(EXIT_BAD_FLAG, 2)
  const foreign = { login: 'mallory', type: 'User' }
  const fx2 = fixture({ comments: [{ ...rec(), user: foreign }] })
  const viaFlag = cli({ argv: ['gate', '--pr', '7', '--head', HEAD, '--repo', 'mallory/wilson-voice'], gh: stubGh(fx2).gh, env: {} })
  assert.equal(viaFlag.code, EXIT_REVIEW, viaFlag.out)
  assert.match(viaFlag.out, /not owned by wilsonguenther-dev/)
  const viaEnv = cli({ argv: ['gate', '--pr', '7', '--head', HEAD], gh: stubGh(fx2).gh, env: { LOOP_REPO: 'mallory/wilson-voice' } })
  assert.equal(viaEnv.code, EXIT_REVIEW, viaEnv.out)
  assert.equal(cli({ argv: ['klass', '--pr', '7', '--repo', 'mallory/x'], gh: stubGh(fx2).gh, env: {} }).code, EXIT_USAGE)
})

test('(h) record --head with a sha that is not the current head -> 64, nothing posted', () => {
  const fx = fixture()
  const { gh, calls } = stubGh(fx)
  const r = cli({ argv: ['record', '--pr', '7', '--seat', 'review-opus', '--lens', 'primary', '--head', OLD, '--verdict', 'PASS', '--blocking', '0'], gh, env: {} })
  assert.equal(r.code, EXIT_USAGE, r.out)
  assert.match(r.out, /not the PR's current head/)
  assert.equal(calls.some((c) => c[0] === 'pr' && c[1] === 'comment'), false)
  assert.equal(fx.posted, undefined)
})

test('record on the current head posts a record the gate then accepts; bad fields -> 64', () => {
  const fx = fixture()
  const { gh } = stubGh(fx)
  const r = cli({ argv: ['record', '--pr', '7', '--seat', 'review-opus', '--lens', 'primary', '--head', HEAD, '--verdict', 'PASS', '--blocking', '0'], gh, env: {} })
  assert.equal(r.code, 0, r.out)
  assert.ok(fx.posted.startsWith(`${RECORD_MARKER}\n`))
  fx.comments = [{ id: 99, user: { login: 'wilsonguenther-dev', type: 'User' }, created_at: T0, updated_at: T0, body: fx.posted }]
  assert.equal(gate(fx).code, 0)
  for (const bad of [['--seat', 'fix-land'], ['--verdict', 'MAYBE'], ['--blocking', '1'], ['--verdict', 'BLOCK']]) {
    const argv = ['record', '--pr', '7', '--seat', 'review-opus', '--lens', 'primary', '--head', HEAD, '--verdict', 'PASS', '--blocking', '0']
    argv[argv.indexOf(bad[0]) + 1] = bad[1]
    assert.equal(cli({ argv, gh: stubGh(fixture()).gh, env: {} }).code, EXIT_USAGE, bad.join(' '))
  }
  assert.throws(() => recordBody({ seat: 'review-opus', lens: 'x', head: HEAD, verdict: 'BLOCK', blocking: 0 }))
})

test('klass: the harness, capabilities, a security id and a >1-page file list are security; a frontend file is standard', () => {
  assert.equal(securityClassOf({ files: ['scripts/loop/review-gate.mjs'] }).security, true)
  assert.equal(securityClassOf({ files: ['scripts/loop/template.mjs'] }).security, true)
  assert.equal(securityClassOf({ title: 'SEC-B: harden' }).security, true)
  assert.equal(securityClassOf({ files: Array.from({ length: 101 }, (_, i) => `desktop/src/f${i}.ts`) }).security, true)
  assert.equal(securityClassOf({ files: ['desktop/src/App.tsx'] }).security, false)
  const { gh } = stubGh(fixture({ files: ['scripts/loop/review-gate.mjs'] }))
  assert.match(cli({ argv: ['klass', '--pr', '7'], gh, env: {} }).out, /^security touches scripts\/loop\/review-gate\.mjs/)
  assert.equal(DEFAULT_REPO, 'wilsonguenther-dev/wilson-voice')
})

test('usage errors -> 64', () => {
  assert.equal(cli({ argv: ['gate'], gh: () => '', env: {} }).code, EXIT_USAGE)
  assert.equal(cli({ argv: ['merge', '--pr', '1'], gh: () => '', env: {} }).code, EXIT_USAGE)
})

// ── (i) build.mjs validation ────────────────────────────────────────────────────────────────────────────────────
const STUB_HARNESS = (fixLandExtra) => `
const LOCAL_REPO = '/x'
const GATE = \`node \${LOCAL_REPO}/scripts/loop/review-gate.mjs gate --pr <n> --head <sha> then gh pr merge --match-head-commit <sha>\`
async function run() {
  const reviewPrompt = (round) => \`review. node \${LOCAL_REPO}/scripts/loop/review-gate.mjs record --pr 1 --seat review-opus\`
  await agentR(reviewPrompt(0), { model: 'opus', phase: 'p', label: \`review-opus:x\` })
  await agentR(\`fix and land. \${GATE} ${fixLandExtra}\`, { model: 'opus', phase: 'p', label: \`fix-land:x\` })
}
`

test('(i) the prompt contract fails on a stub fix-land prompt containing "review-gate.mjs record" and passes without it', () => {
  const clean = promptGateProblems(promptUnits(STUB_HARNESS('')))
  assert.deepEqual(clean, [])
  const dirty = promptGateProblems(promptUnits(STUB_HARNESS('node /x/scripts/loop/review-gate.mjs record --pr 1 --seat review-opus --verdict PASS')))
  assert.equal(dirty.length, 1, dirty.join('\n'))
  assert.match(dirty[0], /fix-land:x prompt .*seat fix-land.*review-gate\.mjs record/)
  // and a merge path without the gate
  const nogate = promptGateProblems(promptUnits("agentR(`drain: gh pr merge -R r 1 --squash`, { label: 'drain' })"))
  assert.equal(nogate.length, 2, nogate.join('\n'))
})

test('(i) the real template satisfies the prompt contract', () => {
  const src = fs.readFileSync(path.join(HERE, 'template.mjs'), 'utf8')
  const units = promptUnits(src)
  assert.deepEqual(promptGateProblems(units), [])
  for (const seat of ['build', 'fix-land', 'land-sweeper', 'drain']) assert.ok(units.some((u) => u.seat === seat && u.text.includes('review-gate.mjs gate')), seat)
  for (const seat of ['review-opus', 'review-opus2']) assert.ok(units.some((u) => u.seat === seat && u.text.includes('review-gate.mjs record')), seat)
})

test('(i) build.mjs --validate-only FAILS when the real fix-land prompt is handed "review-gate.mjs record"', () => {
  const src = fs.readFileSync(path.join(HERE, 'template.mjs'), 'utf8')
  const anchor = 'never start it with the reviewer record marker'
  assert.equal(src.split(anchor).length, 2, 'the injection anchor must exist exactly once in the fix-land prompt')
  const bad = src.replace(anchor, `${anchor} (or just run: node \${LOCAL_REPO}/scripts/loop/review-gate.mjs record --pr \${pr} --seat review-opus)`)
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'review-gate-test-'))
  const tpl = path.join(dir, 'template.mjs')
  fs.writeFileSync(tpl, bad)
  const r = spawnSync(process.execPath, [path.join(HERE, 'build.mjs'), '--validate-only', '--template', tpl], { encoding: 'utf8' })
  fs.rmSync(dir, { recursive: true, force: true })
  const out = `${r.stdout}\n${r.stderr}`
  assert.notEqual(r.status, 0, out)
  assert.match(out, /fix-land:\$\{itemId\} prompt \(line \d+\) \(seat fix-land\) contains "review-gate\.mjs record"/)
})
