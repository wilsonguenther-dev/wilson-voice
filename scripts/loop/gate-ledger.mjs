#!/usr/bin/env node
/**
 * scripts/loop/gate-ledger.mjs — THE FULL GATE RUNS ONCE PER HEAD (harness throughput, 2026-10-27).
 *
 * Why: the review chain landed about one PR an hour because every PR ran the full local gate (tsc, vitest, vite
 * build, both sidecars build+stage+tests, clippy, ~1,700 cargo tests; 15–30 min) four to seven times SERIALLY —
 * once per reviewer, once per fix-land, again per re-review, once at land — on the same sha. A gate is a fact about
 * a tree, and a tree is named by its sha, so the second run on one sha measures nothing new.
 *
 * THE LEDGER — append-only JSON lines at ~/code/wilson-voice-loop/gate-ledger.json (override: LOOP_GATE_LEDGER):
 *     {"v":1,"sha":"<40 hex>","codes":{"typecheck":0,...,"fmt":1,...},"runner":"fix-land:Y5-F","worktree":"<dir>",
 *      "stamp":"<ISO, from the clock>","green":true}
 * RULES
 *   - sha-bound: a record answers for its FULL 40-hex sha and nothing else. No prefix match, no "close enough" —
 *     any push (rebase, fix, amend, empty commit) makes a new sha, which has no record, which means "run the gate".
 *   - complete or nothing: a record counts only if EVERY blocking command (BLOCKING_GATE_CMDS) has an integer
 *     code. fmt is advisory (Y0-A ledger) and never decides green.
 *   - newest complete record for the sha wins (a re-run after a flake supersedes the red one).
 *   - `record` refuses unless the named worktree's HEAD IS the sha and it has no modified tracked files — a gate
 *     measured on a dirty or moved tree is a gate of a tree nobody will merge. That check, plus the runner,
 *     worktree and stamp fields, is what lets a later seat trust a record it did not write.
 *   - `ci` writes the github-ci record only after reading the CI job's check run on that exact head_sha itself
 *     (conclusion success). An agent cannot hand it a conclusion.
 *
 * CLI (exit codes are the contract; read them bare):
 *   gate-ledger.mjs check  --sha S                                 0 green record · 1 RED record · 3 none · 64 usage
 *   gate-ledger.mjs record --sha S --runner R --worktree W --codes "typecheck=0 test=0 ..."   0 written · 64 refused
 *   gate-ledger.mjs ci     --sha S [--repo O/R]                    0 recorded from github-ci · 5 pending/none · 1 red
 */
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { pathToFileURL } from 'node:url'

export const DEFAULT_LEDGER = path.join(os.homedir(), 'code', 'wilson-voice-loop', 'gate-ledger.json')
export const DEFAULT_REPO = 'wilsonguenther-dev/wilson-voice'
/** The CI workflow job whose check run stands in for the local gate in github mode (.github/workflows/ci.yml). */
export const CI_JOB_NAME = 'Tauri build (Rust + frontend)'
/** The ten BLOCKING commands of GATE_CMDS (template.mjs). fmt is advisory and never decides green. */
export const BLOCKING_GATE_CMDS = ['typecheck', 'test', 'build', 'polish', 'diarize', 'stage', 'polishtest', 'diarizetest', 'clippy', 'cargotest']
export const ADVISORY_GATE_CMDS = ['fmt']
export const GITHUB_CI_RUNNER = 'github-ci'
export const EXIT_RED = 1
export const EXIT_NONE = 3
export const EXIT_PENDING = 5
export const EXIT_USAGE = 64
const SHA_RE = /^[0-9a-f]{40}$/
const sha9 = (s) => String(s || '').slice(0, 9) || '(none)'
const ledgerOf = (o) => (o && o.ledger) || process.env.LOOP_GATE_LEDGER || DEFAULT_LEDGER
const normSha = (s) => String(s || '').trim().toLowerCase()

export const isComplete = (rec) => Boolean(rec && rec.codes) && BLOCKING_GATE_CMDS.every((k) => Number.isInteger(rec.codes[k]))
export const isGreen = (rec) => isComplete(rec) && BLOCKING_GATE_CMDS.every((k) => rec.codes[k] === 0)
export const failedCmds = (rec) => BLOCKING_GATE_CMDS.filter((k) => !rec || !rec.codes || rec.codes[k] !== 0)

/** "typecheck=0 test=0, fmt=1 (ADVISORY)" -> {typecheck:0, test:0, fmt:1}. Non-numeric values are dropped. */
export function parseCodes(text) {
  const out = {}
  for (const m of String(text || '').matchAll(/([A-Za-z]+)\s*=\s*(-?\d+)/g)) out[m[1]] = Number(m[2])
  return out
}

/** Append one record. Throws on a sha that is not 40 hex or a missing runner. Returns the record written. */
export function recordGate(sha, codes, { runner, worktree = '', stamp = new Date().toISOString() } = {}, opts = {}) {
  const s = normSha(sha)
  if (!SHA_RE.test(s)) throw new Error(`sha must be the full 40-hex head sha, got "${String(sha).slice(0, 48)}"`)
  if (!String(runner || '').trim()) throw new Error('runner is required (the seat label that ran the gate)')
  const clean = {}
  for (const [k, v] of Object.entries(codes || {})) if (Number.isInteger(v)) clean[k] = v
  const rec = { v: 1, sha: s, codes: clean, runner: String(runner).trim(), worktree: String(worktree || ''), stamp: String(stamp) }
  rec.green = isGreen(rec)
  const file = ledgerOf(opts)
  fs.mkdirSync(path.dirname(file), { recursive: true })
  fs.appendFileSync(file, JSON.stringify(rec) + '\n') // one short O_APPEND write per record: concurrent chains do not interleave
  return rec
}

/** Every parseable record in the ledger, oldest first. A torn or foreign line is skipped, never fatal. */
export function readLedger(opts = {}) {
  let text = ''
  try { text = fs.readFileSync(ledgerOf(opts), 'utf8') } catch { return [] }
  const out = []
  for (const line of text.split('\n')) {
    if (!line.trim()) continue
    try { const r = JSON.parse(line); if (r && typeof r === 'object') out.push(r) } catch { /* torn line */ }
  }
  return out
}

/** The NEWEST complete record for exactly this sha, or null. Incomplete records and other shas never answer. */
export function gateFor(sha, opts = {}) {
  const s = normSha(sha)
  if (!SHA_RE.test(s)) return null
  const hits = readLedger(opts).filter((r) => normSha(r.sha) === s && isComplete(r))
  return hits.length ? hits[hits.length - 1] : null
}

export const satisfiedLine = (rec) => `gate: satisfied by record ${rec.stamp} from ${rec.runner}${rec.worktree ? ` (worktree ${rec.worktree})` : ''} on ${sha9(rec.sha)}`

const gitCli = (args) => {
  const r = spawnSync('git', args, { encoding: 'utf8' })
  if (r.status !== 0) throw new Error(`git ${args.join(' ')} -> ${r.status}: ${String(r.stderr || '').split('\n')[0]}`)
  return r.stdout
}
const ghCli = (args) => {
  const r = spawnSync('gh', args, { encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 })
  if (r.status !== 0) throw new Error(`gh ${args.join(' ')} -> ${r.status}: ${String(r.stderr || '').split('\n')[0]}`)
  return r.stdout
}

/** The worktree must sit ON the sha with no modified tracked files (untracked build outputs are gitignored). */
export function worktreeProblem(worktree, sha, git = gitCli) {
  if (!worktree) return 'no --worktree: a local record must name the tree it measured'
  let head, dirty
  try {
    head = normSha(git(['-C', worktree, 'rev-parse', 'HEAD']))
    dirty = String(git(['-C', worktree, 'status', '--porcelain', '--untracked-files=no'])).trim()
  } catch (err) { return `cannot read ${worktree}: ${String(err && err.message).split('\n')[0]}` }
  if (head !== normSha(sha)) return `${worktree} is at ${sha9(head)}, not ${sha9(sha)} — the gate measured another tree`
  if (dirty) return `${worktree} has modified tracked files (${dirty.split('\n').length}) — commit first; a record binds a COMMITTED tree`
  return ''
}

/** The CI job's check run on exactly this head_sha -> {state: 'success'|'pending'|'failure'|'none', run}. */
export function ciCheckFor({ sha, repo = DEFAULT_REPO, gh = ghCli }) {
  const runs = (JSON.parse(gh(['api', `repos/${repo}/commits/${normSha(sha)}/check-runs?per_page=100`])).check_runs || [])
    .filter((r) => r && r.name === CI_JOB_NAME && normSha(r.head_sha) === normSha(sha))
    .sort((a, b) => String(a.started_at || '').localeCompare(String(b.started_at || '')))
  const run = runs[runs.length - 1]
  if (!run) return { state: 'none', run: null }
  if (run.status !== 'completed') return { state: 'pending', run }
  if (run.conclusion === 'success') return { state: 'success', run }
  if (['skipped', 'neutral', 'stale'].includes(run.conclusion)) return { state: 'none', run } // not evidence either way
  return { state: 'failure', run }
}

function flags(argv) {
  const out = { _: [] }
  for (let i = 0; i < argv.length; i++) {
    if (!argv[i].startsWith('--')) { out._.push(argv[i]); continue }
    const k = argv[i].slice(2)
    out[k] = argv[i + 1] !== undefined && !argv[i + 1].startsWith('--') ? argv[++i] : true
  }
  return out
}

export function cli({ argv, gh = ghCli, git = gitCli, env = process.env, now = () => new Date().toISOString() }) {
  const f = flags(argv)
  const cmd = f._[0]
  const opts = { ledger: f.ledger || env.LOOP_GATE_LEDGER || DEFAULT_LEDGER }
  const sha = normSha(f.sha === true ? '' : f.sha)
  if (!['check', 'record', 'ci'].includes(cmd)) return { code: EXIT_USAGE, out: 'usage: gate-ledger.mjs check|record|ci --sha <40-hex> [...] (see the header)' }
  if (!SHA_RE.test(sha)) return { code: EXIT_USAGE, out: `usage: --sha must be the full 40-hex head sha, got "${String(f.sha || '').slice(0, 48)}"` }
  if (cmd === 'check') {
    const rec = gateFor(sha, opts)
    if (!rec) return { code: EXIT_NONE, out: `gate: no complete record for ${sha9(sha)} — run the full gate on this head` }
    if (!isGreen(rec)) return { code: EXIT_RED, out: `gate: RED record ${rec.stamp} from ${rec.runner} on ${sha9(sha)} — failed: ${failedCmds(rec).join(', ')}` }
    return { code: 0, out: satisfiedLine(rec) }
  }
  if (cmd === 'record') {
    if (String(f.runner || '').trim() === GITHUB_CI_RUNNER) return { code: EXIT_USAGE, out: `record refused: runner ${GITHUB_CI_RUNNER} is written only by "ci", which reads the check run itself` }
    const why = worktreeProblem(f.worktree === true ? '' : f.worktree, sha, git)
    if (why) return { code: EXIT_USAGE, out: `record refused: ${why}` }
    try {
      const rec = recordGate(sha, parseCodes(f.codes), { runner: f.runner === true ? '' : f.runner, worktree: f.worktree, stamp: now() }, opts)
      if (!isComplete(rec)) return { code: EXIT_USAGE, out: `record written but INCOMPLETE (missing: ${BLOCKING_GATE_CMDS.filter((k) => !Number.isInteger(rec.codes[k])).join(', ')}) — it will never satisfy the gate` }
      return { code: 0, out: `gate record written: ${sha9(sha)} ${rec.green ? 'green' : `RED (${failedCmds(rec).join(', ')})`} from ${rec.runner}` }
    } catch (err) { return { code: EXIT_USAGE, out: `record refused: ${String(err && err.message).split('\n')[0]}` } }
  }
  // ci
  let c
  try { c = ciCheckFor({ sha, repo: f.repo || env.LOOP_REPO || DEFAULT_REPO, gh }) } catch (err) {
    return { code: EXIT_PENDING, out: `ci: unreadable (${String(err && err.message).split('\n')[0]}) — no record written` }
  }
  if (c.state === 'success') {
    const codes = Object.fromEntries(BLOCKING_GATE_CMDS.map((k) => [k, 0]))
    const rec = recordGate(sha, codes, { runner: GITHUB_CI_RUNNER, worktree: `check-run ${c.run.id || '?'} ${c.run.html_url || ''}`.trim(), stamp: now() }, opts)
    return { code: 0, out: satisfiedLine(rec) }
  }
  if (c.state === 'failure') return { code: EXIT_RED, out: `ci: "${CI_JOB_NAME}" concluded ${c.run.conclusion} on ${sha9(sha)} ${c.run.html_url || ''} — no record written; run the local gate to attribute it`.trim() }
  if (c.state === 'pending') return { code: EXIT_PENDING, out: `ci: "${CI_JOB_NAME}" is ${c.run.status} on ${sha9(sha)} — ci-pending, no record written` }
  return { code: EXIT_PENDING, out: `ci: no conclusive "${CI_JOB_NAME}" check run on ${sha9(sha)} — no record written` }
}

// ═══ THE PROMPT CONTRACT — build.mjs validation (5h) ═════════════════════════════════════════════════════════════
export const LEDGER_CHECK_NEEDLE = 'gate-ledger.mjs check'
export const LEDGER_RECORD_NEEDLE = 'gate-ledger.mjs record'
/** The sentence only GATE_CMDS carries: a prompt containing it is handed the full 15–30 min gate. */
export const FULL_GATE_NEEDLE = 'THE TEN BLOCKING COMMANDS MUST EXIT 0'

/** units: promptUnits() output. Reviewer seats verify the record and are never handed the full gate; every prompt
 *  handed the full gate also writes its record (else the next seat on that head runs it again). */
export function ledgerPromptProblems(units, reviewerSeats = ['review-opus', 'review-opus2']) {
  const problems = []
  for (const u of units || []) {
    const where = `${u.label || u.seat} prompt (line ${u.line ?? '?'})`
    const t = String(u.text || '')
    if (reviewerSeats.includes(u.seat)) {
      if (!t.includes(LEDGER_CHECK_NEEDLE)) problems.push(`${where} (seat ${u.seat}) does not contain "${LEDGER_CHECK_NEEDLE}" — a reviewer verifies the gate record for its head instead of re-running the gate`)
      if (t.includes(FULL_GATE_NEEDLE)) problems.push(`${where} (seat ${u.seat}) is handed the FULL gate — reviewers read and run targeted tests only (the gate ran 4–7× per PR before 2026-10-27)`)
    }
    if (t.includes(FULL_GATE_NEEDLE) && !t.includes(LEDGER_RECORD_NEEDLE)) problems.push(`${where} runs the full gate but never writes "${LEDGER_RECORD_NEEDLE}" — the next seat on that head would run it again`)
  }
  return problems
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const r = cli({ argv: process.argv.slice(2) })
  console.log(r.out)
  process.exitCode = r.code
}
