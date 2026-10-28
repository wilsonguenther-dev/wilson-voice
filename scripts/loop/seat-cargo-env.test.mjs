// scripts/loop/seat-cargo-env.test.mjs — node --test. HERMETIC (no cargo, no network).
// Every seat shell that exports CARGO_TARGET_DIR also exports CARGO_INCREMENTAL=0. Root cause it
// guards: reviewer seats filled target-r1/r2/debug/incremental with GBs (2026-10-26) even though
// ~/code/wilson-voice-loop/.cargo/config.toml sets build.incremental=false. The env var beats every
// cargo config layer and every tool (tauri dev/build) that spawns cargo.
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'

const HERE = path.dirname(new URL(import.meta.url).pathname)
const TEMPLATE = fs.readFileSync(path.join(HERE, 'template.mjs'), 'utf8')
const SMOKE = fs.readFileSync(path.join(HERE, '..', 'loop-smoke.sh'), 'utf8')

test('every CARGO_TARGET_DIR export in the template also sets CARGO_INCREMENTAL=0', () => {
  const exports = TEMPLATE.split('\n').filter((l) => /export\s+[^\n]*CARGO_TARGET_DIR=/.test(l))
  assert.ok(exports.length >= 7, `expected >=7 seat export lines, found ${exports.length}`)
  for (const l of exports) assert.match(l, /export CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=/, l.trim())
})

test('the land gate (GATE_CMDS) runs cargo with CARGO_INCREMENTAL=0', () => {
  const gate = (TEMPLATE.match(/const GATE_CMDS = \(dir, target\) => `([\s\S]*?)\n`\n/) || [])[1] || ''
  assert.ok(gate.length > 200, 'could not extract GATE_CMDS')
  const firstCargo = gate.indexOf('cargo ')
  const env = gate.indexOf('CARGO_INCREMENTAL=0')
  assert.ok(env >= 0 && env < firstCargo, 'CARGO_INCREMENTAL=0 must be exported before the first cargo command')
})

test('loop-smoke.sh exports CARGO_INCREMENTAL=0 before any stage', () => {
  const env = SMOKE.indexOf('export CARGO_INCREMENTAL=0')
  assert.ok(env >= 0, 'loop-smoke.sh must export CARGO_INCREMENTAL=0')
  assert.ok(env < SMOKE.indexOf('stage_test_polish()'), 'export must precede the stage definitions')
})
