// scripts/loop/land-gate-fmt.test.mjs — node --test. HERMETIC (no cargo, no network).
// Asserts the Y0-A ledger rule in the LAND gate: rustfmt is ADVISORY (it runs and its hunk count is
// reported, but its exit code never fails the gate), clippy stays BLOCKING. Root cause it guards:
// review pass w7sv7tozp (2026-10-26) turned every PASS/PASS PR ci-red because the land gate required
// `cargo fmt --all -- --check`, which exits 1 on unmodified main. yap-fmt-sweep-2026-10-26 flips this
// back to blocking — that PR must update this test in the same diff.
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'

const HERE = path.dirname(new URL(import.meta.url).pathname)
const TEMPLATE = fs.readFileSync(path.join(HERE, 'template.mjs'), 'utf8')
const SMOKE = fs.readFileSync(path.join(HERE, '..', 'loop-smoke.sh'), 'utf8')
const gateCmds = (TEMPLATE.match(/const GATE_CMDS = \(dir, target\) => `([\s\S]*?)\n`\n/) || [])[1] || ''

test('GATE_CMDS is found', () => assert.ok(gateCmds.length > 200, 'could not extract GATE_CMDS from template.mjs'))

test('fmt still runs in the land gate and is marked ADVISORY with a hunk count', () => {
  const fmt = gateCmds.split('\n').find((l) => l.includes('cargo fmt --all -- --check'))
  assert.ok(fmt, 'fmt command missing from GATE_CMDS — it must still RUN')
  assert.match(fmt, /fmt=\$\? \(ADVISORY\)/)
  assert.match(fmt, /fmtHunks=/)
  assert.match(gateCmds, /fmt IS ADVISORY/)
  assert.match(gateCmds, /NEVER let its exit code fail the gate/)
})

test('the gate no longer demands all eleven at 0', () => {
  assert.doesNotMatch(gateCmds, /ALL ELEVEN MUST EXIT 0/)
  assert.match(gateCmds, /THE TEN BLOCKING COMMANDS MUST EXIT 0/)
})

test('clippy stays BLOCKING in the land gate', () => {
  const clippy = gateCmds.split('\n').find((l) => l.includes('cargo clippy'))
  assert.ok(clippy, 'clippy missing from GATE_CMDS')
  assert.doesNotMatch(clippy, /\|\|\s*true|ADVISORY/i)
  assert.match(clippy, /echo "clippy=\$\?"$/)
  assert.match(gateCmds, /clippy STAYS BLOCKING/)
})

test('land outcomes: a non-zero fmt alone is never ci-red; the gate comment reports fmt as advisory', () => {
  assert.match(TEMPLATE, /any BLOCKING command non-zero -> this is a red gate \(a non-zero fmt ALONE is NEVER ci-red\)/)
  assert.match(TEMPLATE, /every BLOCKING command 0 \(fmt is advisory and excluded\)/)
  assert.match(TEMPLATE, /fmt: advisory, <fmtHunks> hunks/)
})

// loop-smoke's stage_fmt, executed for real against a stub `cargo` that reports 2 hunks and exits 1.
const stageFmt = (SMOKE.match(/^stage_fmt\(\) \{[\s\S]*?^\}/m) || [])[0]
const runStageFmt = (env = {}) => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'fmt-stub-'))
  fs.writeFileSync(path.join(dir, 'cargo'), '#!/bin/sh\necho "Diff in /x/a.rs at line 1:"\necho "Diff in /x/b.rs at line 9:"\nexit 1\n', { mode: 0o755 })
  const script = `set -euo pipefail\nSRC_TAURI="${dir}"\n${stageFmt}\nstage_fmt`
  const r = spawnSync('bash', ['-c', script], { env: { ...process.env, PATH: `${dir}:${process.env.PATH}`, ...env }, encoding: 'utf8' })
  fs.rmSync(dir, { recursive: true, force: true })
  return r
}

test('loop-smoke stage_fmt is advisory by default (exit 0, hunks reported)', () => {
  assert.ok(stageFmt, 'stage_fmt not found in loop-smoke.sh')
  const r = runStageFmt({ YAP_SMOKE_FMT_STRICT: '0' })
  assert.equal(r.status, 0, r.stdout + r.stderr)
  assert.match(r.stdout, /fmt: exit 1, 2 hunks/)
  assert.match(r.stdout, /ADVISORY/)
})

test('loop-smoke stage_fmt gates when YAP_SMOKE_FMT_STRICT=1', () => {
  const r = runStageFmt({ YAP_SMOKE_FMT_STRICT: '1' })
  assert.equal(r.status, 1, r.stdout + r.stderr)
})

test('GATE ONCE PER HEAD: GATE_CMDS ends by writing a sha-bound ledger record with all ten blocking codes', () => {
  const rec = gateCmds.split('\n').find((l) => l.includes('gate-ledger.mjs record'))
  assert.ok(rec, 'GATE_CMDS must write the ledger record')
  assert.match(rec, /--sha "\$\(git rev-parse HEAD\)" --worktree \$\{dir\}/)
  for (const k of ['typecheck', 'test', 'build', 'polish', 'diarize', 'stage', 'polishtest', 'diarizetest', 'clippy', 'cargotest']) assert.match(rec, new RegExp(`\\b${k}=<c>`), k)
  assert.match(rec, /echo "ledger=\$\?"$/)
})

test('GATE ONCE PER HEAD: the local land gate reads the ledger first; github mode lets green CI stand in', () => {
  assert.match(TEMPLATE, /CHECK THE LEDGER FIRST/)
  assert.match(TEMPLATE, /exit 0 -> the gate is satisfied[\s\S]{0,200}do NOT run the commands below/)
  assert.match(TEMPLATE, /GREEN CI ON THE EXACT HEAD IS THE GATE — NO LOCAL RUN/)
  assert.doesNotMatch(TEMPLATE, /a github-mode merge still owes the local/)
  assert.match(TEMPLATE, /exit 1 \(failure\) -> label ci-red\. ATTRIBUTE IT/)
  assert.match(TEMPLATE, /THE ONE FULL RUN OF THIS ROUND/)
})
