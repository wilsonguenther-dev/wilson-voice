// scripts/loop/review-gate.test.mjs — node --test. HERMETIC: gh is a stub fed from fixture JSON through the
// injectable runner (cli({gh})), never the network. Cases (a)–(i) are the yap-land-gate-pass-required spec.
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import {
  cli, parseRecord, recordBody, headAt, securityClassOf, promptUnits, promptGateProblems,
  EXIT_REVIEW, EXIT_USAGE, EXIT_BAD_FLAG, DEFAULT_REPO, RECORD_MARKER,
} from './review-gate.mjs'

const HERE = path.dirname(new URL(import.meta.url).pathname)
const HEAD = 'a'.repeat(40)
const OLD = 'b'.repeat(40)
const T0 = '2026-10-17T10:00:00Z'
let nextId = 1

/** A fixture PR. files default to a standard (non-security) frontend file. */
const fixture = ({ head = HEAD, files = ['desktop/src/App.tsx'], labels = [], title = 'yap-x: standard change', comments = [], commits = [{ oid: head, committedDate: '2026-10-17T09:00:00Z' }] } = {}) =>
  ({ head, files, labels, title, comments, commits, ci: 'success', mergeable: 'MERGEABLE' })

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
    if (/^pr view \d+ -R \S+ --json labels,headRefOid,title,commits$/.test(a)) return JSON.stringify({ labels: fx.labels.map((name) => ({ name })), headRefOid: fx.head, title: fx.title, commits: fx.commits })
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
  assert.equal(parseRecord(short.body).kind, 'unparsable')
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
  assert.match(sec.out, /BLOCK on aaaaaaaaa by review-opus2 \(comment \d+; edited\)/)
  const std = gate(fixture({ comments: comments() }))
  assert.equal(std.code, EXIT_REVIEW, std.out)
  assert.match(std.out, /BLOCK/)
})

test('CLEARING: a BLOCK is FINAL for its head — a later PASS from the same seat (fresh, unedited) or any seat never clears it', () => {
  const same = gate(fixture({ comments: [rec({ seat: 'review-opus2', verdict: 'BLOCK', blocking: 1, edited: true }), rec({ seat: 'review-opus2', at: LATER })] }))
  assert.equal(same.code, EXIT_REVIEW, same.out)
  assert.match(same.out, /final for this head/)
  const unedited = gate(fixture({ ...SEC, comments: [rec({ seat: 'review-opus', verdict: 'BLOCK', blocking: 1 }), rec({ at: LATER }), rec({ seat: 'review-opus2', at: LATER })] }))
  assert.equal(unedited.code, EXIT_REVIEW, unedited.out)
  // an edited PASS reads as a BLOCK on its head
  const ep = gate(fixture({ comments: [rec({ edited: true }), rec({ seat: 'review-opus2', at: LATER })] }))
  assert.equal(ep.code, EXIT_REVIEW, ep.out)
  assert.match(ep.out, /edited PASS/)
})

test('CLEARING: a new push + a complete fresh round on the new head lands past an old-head BLOCK', () => {
  const commits = [{ oid: OLD, committedDate: '2026-10-17T09:00:00Z' }, { oid: HEAD, committedDate: '2026-10-17T11:00:00Z' }]
  const round = [rec({ seat: 'review-opus2', head: OLD, verdict: 'BLOCK', blocking: 2 }), rec({ at: '2026-10-17T12:00:00Z' }), rec({ seat: 'review-opus2', at: '2026-10-17T12:01:00Z' })]
  assert.equal(gate(fixture({ ...SEC, commits, comments: round })).code, 0)
  // a half round (security PR, one seat) does not
  assert.equal(gate(fixture({ ...SEC, commits, comments: round.slice(0, 2) })).code, EXIT_REVIEW)
})

test('B2: a BLOCK under any seat name counts — review-opus PASS + newer review-opus-h1 BLOCK -> 6; both PASSes + h1 BLOCK (security) -> 6', () => {
  const std = gate(fixture({ comments: [rec(), rec({ seat: 'review-opus-h1', verdict: 'BLOCK', blocking: 2, at: LATER })] }))
  assert.equal(std.code, EXIT_REVIEW, std.out)
  assert.match(std.out, /^review: needs-human — BLOCK on aaaaaaaaa from outside seat\(s\) review-opus-h1 \(comment \d+\)/)
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

const body = (b) => ({ ...rec({ at: LATER }), body: b })
const PASS2 = () => rec({ seat: 'review-opus2' })

test('RECOGNITION: bold / spaced / fenced / blank-line / notes / FAIL / unparsable all read as BLOCK', () => {
  const shapes = {
    bold: `**loop-review (Opus)**\n**seat:** review-opus\n**head:** ${HEAD}\n**verdict:** BLOCK blocking=1`,
    spaced: `loop-review (Opus)\nseat: review-opus\nhead: ${HEAD}\nVerdict : BLOCK`,
    fenced: `independent review\n\`\`\`\nloop-review (Opus)\nseat: review-opus\nhead: ${HEAD}\nverdict: BLOCK blocking=2\n\`\`\``,
    blankLine: `loop-review (Opus)\nseat: review-opus\n\nhead: ${HEAD}\n\nverdict: BLOCK blocking=1`,
    inNotes: `loop-review (Opus)\nseat: review-opus\nlens: x\nhead: ${HEAD}\nverdict: PASS blocking=0\n\nsecond look — verdict: BLOCK, the updater path`,
    fail: `loop-review (Opus)\nseat: review-opus\nlens: x\nhead: ${HEAD}\nverdict: FAIL blocking=1`,
    nonPassWord: `loop-review (Opus)\nseat: review-opus\nlens: x\nhead: ${HEAD}\nverdict: REJECT`,
    unparsable: 'loop-review: not ready, see below',
  }
  for (const [name, b] of Object.entries(shapes)) {
    const r = gate(fixture({ comments: [PASS2(), body(b)] }))
    assert.equal(r.code, EXIT_REVIEW, `${name}: ${r.out}`)
    assert.match(r.out, /BLOCK on aaaaaaaaa/, name)
  }
  assert.equal(parseRecord(shapes.fail).kind, 'unparsable')
  assert.equal(parseRecord(shapes.fenced).kind, 'block')
  assert.match(gate(fixture({ comments: [PASS2(), body(shapes.unparsable)] })).out, /unparsable record/)
  assert.equal(parseRecord('ordinary comment, no marker'), null)
  // a foreign author's BLOCK is still not a record
  assert.equal(gate(fixture({ comments: [PASS2(), { ...body(shapes.bold), user: { login: 'someone-else', type: 'User' } }] })).code, 0)
  // the record subcommand refuses a PASS whose notes would read as a BLOCK
  assert.throws(() => recordBody({ seat: 'review-opus', lens: 'x', head: HEAD, verdict: 'PASS', blocking: 0, notes: 'verdict: BLOCK earlier' }), /verdict:/)
})

test('SCOPE: a headless BLOCK applies to the head current at its created_at, never to a later head', () => {
  const commits = [{ oid: OLD, committedDate: '2026-10-17T09:00:00Z' }, { oid: HEAD, committedDate: '2026-10-17T11:00:00Z' }]
  const headless = (at) => ({ ...rec({ at }), body: `loop-review (Opus)\nseat: review-opus\nverdict: BLOCK blocking=1` })
  assert.equal(headAt(commits, '2026-10-17T10:00:00Z'), OLD)
  assert.equal(headAt(commits, '2026-10-17T08:00:00Z'), null)
  // posted at 10:00 → on OLD; head is now HEAD with a full round → lands
  const fullRound = [rec({ at: '2026-10-17T12:00:00Z' }), rec({ seat: 'review-opus2', at: '2026-10-17T12:01:00Z' })]
  assert.equal(gate(fixture({ ...SEC, commits, comments: [headless('2026-10-17T10:00:00Z'), ...fullRound] })).code, 0)
  // posted at 11:30 → on HEAD → refused, and the same seat's later PASS does not clear it
  const now = gate(fixture({ ...SEC, commits, comments: [headless('2026-10-17T11:30:00Z'), ...fullRound] }))
  assert.equal(now.code, EXIT_REVIEW, now.out)
  // while OLD was still the head, the headless BLOCK refused it
  assert.equal(gate(fixture({ head: OLD, commits: commits.slice(0, 1), comments: [headless('2026-10-17T10:00:00Z'), rec({ head: OLD, at: LATER })] })).code, EXIT_REVIEW)
  // an unreadable commit list fails closed
  const fx = fixture({ comments: [rec()] }); fx.commits = null
  assert.equal(gate(fx).code, EXIT_REVIEW)
})

test('ROUTING: an outside-seat BLOCK prints needs-human with the comment id; a reviewer-seat BLOCK does not', () => {
  const h1 = rec({ seat: 'review-opus-h1', verdict: 'BLOCK', blocking: 1, at: LATER })
  const r = gate(fixture({ comments: [rec(), h1] }))
  assert.equal(r.code, EXIT_REVIEW)
  assert.ok(r.out.startsWith('review: needs-human'), r.out)
  assert.ok(r.out.includes(`comment ${h1.id}`), r.out)
  const inside = gate(fixture({ comments: [rec(), rec({ seat: 'review-opus2', verdict: 'BLOCK', blocking: 1, at: LATER })] }))
  assert.ok(inside.out.startsWith('review: BLOCK'), inside.out)
  const tpl = fs.readFileSync(path.join(HERE, 'template.mjs'), 'utf8')
  assert.ok(tpl.includes('exit 6 and the last line starts "review: needs-human"'))
  assert.ok(tpl.includes('--add-label needs-human') && tpl.includes('needs-human: outside-seat BLOCK, comment id(s)'))
})

test('TRIAGE: needs-human PRs are skipped (prompt, schema and the JS filter) and listed as held', () => {
  const tpl = fs.readFileSync(path.join(HERE, 'template.mjs'), 'utf8')
  assert.ok(tpl.includes('SKIP every PR labelled needs-human'))
  assert.match(tpl, /held: \{\s*type: 'array'/)
  assert.ok(tpl.includes("REVIEW_QUEUE = extractArray(triage, 'queue').filter((q) => !isHeld(q))"))
  assert.ok(tpl.includes('list them as HELD'))
  // the filter itself, lifted out of the template and run
  const src = tpl.match(/const isHeld = (\(q\) => [^\n]+)/)[1]
  const isHeld = new Function(`return ${src}`)()
  const queue = [{ pr: 1, labels: ['loop-build'] }, { pr: 2, labels: ['needs-human'] }, { pr: 3, labels: [{ name: 'needs-human' }] }, { pr: 4 }]
  assert.deepEqual(queue.filter((q) => !isHeld(q)).map((q) => q.pr), [1, 4])
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

// ── round 3: marker at line start; refute-only fix round; LAND BLOCK → needs-human (review-opus2 B1 on #222) ──
test('MARKER: a mid-sentence mention is not a record; a line-start marker (markdown allowed) still is and fails closed', () => {
  // verdict-less mid-sentence mention: prose, not a record
  assert.equal(parseRecord('the finding quotes the loop-review marker mid-sentence and nothing else'), null)
  assert.equal(gate(fixture({ comments: [rec(), body('as the earlier loop-review said, the updater is fine')] })).code, 0)
  for (const b of ['loop-review (Opus)\nverdict: BLOCK', '  **loop-review** — verdict: FAIL', '> loop-review (Opus)\n> seat: x', '```\nloop-review (Opus)\n```']) {
    assert.notEqual(parseRecord(b), null, b)
    assert.equal(gate(fixture({ comments: [rec(), body(b)] })).code, EXIT_REVIEW, b)
  }
})

/** Runs the GENERATED review pass (part-01, args.mode=review) with a scripted agent(): one PR, seats answered by `script`. */
async function runChain(script) {
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
  const generic = () => new Proxy({ status: 'ok', ok: true, halted: false, exits: [0], allZero: true, verdict: 'PASS', blocking: [], advisory: [], prs: [], items: [], result: {} },
    { get: (t, p) => (p in t ? t[p] : p === 'then' || p === 'toJSON' || typeof p === 'symbol' ? undefined : benign()) })
  const seats = []
  const logs = []
  const entry = { pr: 7, item: 'X-1', title: 'X-1: a change', headRefName: 'loop/x-1', klass: 'standard', ci: 'none', mergeable: 'MERGEABLE', files: 1, additions: 1 }
  const agent = async (prompt, opts = {}) => {
    const label = String(opts.label || '')
    if (label === 'review-triage') return { queue: [entry], closed: [], held: [] }
    if (label.endsWith(':X-1')) {
      const role = label.startsWith('review-opus') ? label.split(':')[0] : (String(prompt).match(/You are the (FIX|LAND|HOLD|PARK) agent/) || [])[1] || '?'
      seats.push(role)
      return script(role, seats.filter((r) => r === role).length)
    }
    return generic()
  }
  const FIXED = 1757808000000
  class FrozenDate extends Date { constructor(...a) { super(...(a.length ? a : [FIXED])) } static now() { return FIXED } }
  const boom = () => { throw new Error('no outside world in a chain test') }
  const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor
  const fn = new AsyncFunction('agent', 'parallel', 'pipeline', 'phase', 'log', 'workflow', 'args', 'budget', 'Date', 'Math', 'fetch', 'process', 'require', src)
  await fn(agent, async (fs_) => Promise.all(fs_.map((f) => f())), async (fs_) => { const o = []; for (const f of fs_) o.push(await f()); return o },
    () => {}, (m) => logs.push(String(m)), async () => ({ halted: false, parts: [], items: [] }), { mode: 'review' },
    new Proxy({ remaining: 1e9, used: 0, limit: 1e9 }, { get: (t, p) => (p in t ? t[p] : benign()) }), FrozenDate, Object.create(Math, { random: { value: () => 0.42 } }),
    boom, new Proxy({}, { get: boom }), boom)
  return { seats, logs, outcome: logs.filter((l) => l.startsWith('PR#7 ')) }
}
const NEW = 'c'.repeat(40)
const BLOCKED = { status: 'reviewed', verdict: 'BLOCK', blocking: ['the updater path is unsigned'], advisory: [], recordHead: HEAD, recordPosted: true }
const PASSED = { status: 'reviewed', verdict: 'PASS', blocking: [], advisory: [], recordHead: HEAD, recordPosted: true }
const REFUTED = [{ finding: 'the updater path is unsigned', disposition: 'refuted', evidence: 'signed at build.rs:12' }]

test('CHAIN: a refute-only fix round (no new head) -> needs-human and ZERO further seats (no re-review, no LAND)', async () => {
  const r = await runChain((role) => role === 'review-opus' ? BLOCKED : role === 'FIX' ? { status: 'no-new-head', headBefore: HEAD, head: HEAD, blockingDispositions: REFUTED } : { status: 'merged' })
  assert.deepEqual(r.seats, ['review-opus', 'FIX'], r.logs.join('\n'))
  assert.ok(r.outcome.some((l) => /no new head .*-> needs-human/.test(l)), r.outcome.join('\n'))
  // a fixer that says "fixed" but left the head where the reviewer read it: one HOLD seat labels it, still no re-review
  const claimed = await runChain((role) => role === 'review-opus' ? BLOCKED : role === 'FIX' ? { status: 'fixed', headBefore: HEAD, head: HEAD, blockingDispositions: REFUTED } : { status: 'needs-human' })
  assert.deepEqual(claimed.seats, ['review-opus', 'FIX', 'HOLD'], claimed.logs.join('\n'))
  // an unreadable new head fails closed the same way
  const blind = await runChain((role) => role === 'review-opus' ? BLOCKED : role === 'FIX' ? { status: 'fixed', blockingDispositions: REFUTED } : { status: 'needs-human' })
  assert.deepEqual(blind.seats, ['review-opus', 'FIX', 'HOLD'], blind.logs.join('\n'))
  // control: a real new head IS re-reviewed, then landed
  const moved = await runChain((role, n) => role === 'review-opus' ? (n === 1 ? BLOCKED : { ...PASSED, recordHead: NEW }) : role === 'FIX' ? { status: 'fixed', headBefore: HEAD, head: NEW, blockingDispositions: REFUTED } : { status: 'merged', landed: true })
  assert.deepEqual(moved.seats, ['review-opus', 'FIX', 'review-opus', 'LAND'], moved.logs.join('\n'))
})

test('CHAIN: LAND refused by a BLOCK on the head this pass reviewed -> needs-human, never review-pending', async () => {
  const line = 'review: BLOCK on aaaaaaaaa by review-opus2 (comment 9) (standard) — refused; a BLOCK is final for this head'
  const r = await runChain((role) => role === 'review-opus' ? PASSED : role === 'LAND' ? { status: 'review-pending', landed: false, reviewGate: line, reviewGateExit: 6 } : { status: 'needs-human' })
  assert.deepEqual(r.seats, ['review-opus', 'LAND', 'HOLD'], r.logs.join('\n'))
  assert.ok(r.outcome.some((l) => /-> needs-human$/.test(l)), r.outcome.join('\n'))
  assert.ok(!r.outcome.some((l) => /-> review-pending$/.test(l)))
  // a lander that already labelled it needs-human: no extra seat
  const self = await runChain((role) => role === 'review-opus' ? PASSED : role === 'LAND' ? { status: 'needs-human', landed: false, reviewGate: line } : { status: 'x' })
  assert.deepEqual(self.seats, ['review-opus', 'LAND'])
  assert.ok(self.outcome.some((l) => /-> needs-human$/.test(l)), self.outcome.join('\n'))
  // a "missing" gate line is still review-pending (nothing final on that head)
  const missing = await runChain((role) => role === 'review-opus' ? PASSED : role === 'LAND' ? { status: 'review-pending', reviewGate: 'review: missing on aaaaaaaaa (standard): no review-opus or review-opus2 PASS bound to aaaaaaaaa — refused' } : { status: 'x' })
  assert.ok(missing.outcome.some((l) => /-> review-pending$/.test(l)), missing.outcome.join('\n'))
})

test('TEMPLATE: no empty-commit escape; the fixer prompt and fix comment never carry the marker word', () => {
  const tpl = fs.readFileSync(path.join(HERE, 'template.mjs'), 'utf8')
  assert.ok(tpl.includes('NEVER make an empty commit'))
  assert.doesNotMatch(tpl, /git commit[^\n]*--allow-empty/)
  const fixFn = tpl.slice(tpl.indexOf('function fixLandPrompt('), tpl.indexOf("const land = await withAgents(1, () => agentR(fixLandPrompt('land'"))
  assert.ok(fixFn.length > 1000)
  assert.doesNotMatch(fixFn, /loop-review/i)
})

// ── round 4: BLOCK wherever the marker sits; line-start only for verdict-less records; reviewer-head compare ──
test('RECOGNITION r4: all seven marker positions with a BLOCK verdict line -> BLOCK', () => {
  const tail = `\nseat: review-opus\nhead: ${HEAD}\nverdict: BLOCK blocking=1`
  const formats = {
    numbered: `1. loop-review (Opus)${tail}`,
    emoji: `🔴 loop-review (Opus)${tail}`,
    table: `| loop-review | Opus |\n|---|---|${tail}`,
    reply: `Re: loop-review (Opus)${tail}`,
    html: `<b>loop-review</b>${tail}`,
    diffPlus: `+ loop-review (Opus)${tail}`,
    midSentence: `An independent pass of the loop-review kind found one defect.\nverdict: BLOCK blocking=1`,
  }
  for (const [name, b] of Object.entries(formats)) {
    const r = parseRecord(b)
    assert.ok(r && r.verdict === 'BLOCK' && r.kind === 'block', `${name}: ${JSON.stringify(r)}`)
    const g = gate(fixture({ comments: [rec(), body(b)] }))
    assert.equal(g.code, EXIT_REVIEW, `${name}: ${g.out}`)
    assert.match(g.out, /BLOCK on aaaaaaaaa/, name)
  }
})

test('RECOGNITION r4: marker with no verdict line — mid-sentence is not a record, line-start is an unparsable BLOCK', () => {
  assert.equal(parseRecord('we should rerun the loop-review seat tomorrow'), null)
  assert.equal(gate(fixture({ comments: [rec(), body('we should rerun the loop-review seat tomorrow')] })).code, 0)
  for (const b of ['loop-review (Opus)\nseat: review-opus\nlooks risky', '**loop-review** notes only', '> loop-review']) {
    assert.equal(parseRecord(b).kind, 'unparsable', b)
    const g = gate(fixture({ comments: [rec(), body(b)] }))
    assert.equal(g.code, EXIT_REVIEW, g.out)
    assert.match(g.out, /unparsable record/)
  }
  // marker mid-sentence + a non-BLOCK, non-clean verdict line → unparsable BLOCK (fails closed)
  assert.equal(parseRecord('per the loop-review rules\nverdict: FAIL').kind, 'unparsable')
  // PASS unchanged: strict, marker first
  assert.equal(parseRecord(rec().body).kind, 'pass')
  assert.equal(parseRecord(`note\n${rec().body}`).kind, 'unparsable')
})

test('CHAIN r4: the re-review recorded a head an earlier round read -> needs-human, no LAND (whatever the fixer reported)', async () => {
  // the fixer claims a new head NEW, but the re-review read HEAD again
  const r = await runChain((role, n) => role === 'review-opus' ? (n === 1 ? BLOCKED : PASSED) : role === 'FIX' ? { status: 'fixed', headBefore: HEAD, head: NEW, blockingDispositions: REFUTED } : { status: 'merged', landed: true })
  assert.deepEqual(r.seats, ['review-opus', 'FIX', 'review-opus', 'HOLD'], r.logs.join('\n'))
  assert.ok(r.outcome.some((l) => /already reviewed — no new head.*-> needs-human/.test(l)), r.outcome.join('\n'))
})

test('CHAIN r4: LAND BLOCK naming a head no reviewer recorded -> needs-human too (fails closed), annotated', async () => {
  const line = 'review: BLOCK on ddddddddd by review-opus2 (comment 9) (standard) — refused'
  const r = await runChain((role) => role === 'review-opus' ? PASSED : role === 'LAND' ? { status: 'review-pending', reviewGate: line } : { status: 'needs-human' })
  assert.deepEqual(r.seats, ['review-opus', 'LAND', 'HOLD'], r.logs.join('\n'))
  assert.ok(r.outcome.some((l) => /-> needs-human$/.test(l)), r.outcome.join('\n'))
  const tpl = fs.readFileSync(path.join(HERE, 'template.mjs'), 'utf8')
  assert.ok(tpl.includes("one NO reviewer in this pass recorded"))
})
