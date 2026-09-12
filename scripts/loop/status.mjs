#!/usr/bin/env node
/**
 * scripts/loop/status.mjs — the live STATUS BOARD of the Yap overhaul CI/CD loop.
 *
 *     node scripts/loop/status.mjs <itemId> <lane:stage> ["note"] [PR] [--open N]
 *     node scripts/loop/status.mjs --part part-01 --total 6 --current none
 *
 * Every agent in the loop calls this at the end of its LOGLINE block, so Wilson can read ONE file
 * and know where every item is without opening a transcript. It UPSERTS ONE ROW PER ITEM — calling
 * it fifty times for one item is correct and never duplicates a row — and keeps the rows in the
 * order the items first appeared, which is rollout order.
 *
 * THE STAGE IS LANE-PREFIXED, and that is what makes the board safe to write from two lanes at once:
 *     A:build          lane A (worktree wilson-voice-loop/lane-a, port 5273)
 *     B:preflight      lane B (worktree wilson-voice-loop/lane-b, port 5274)
 *     review:merge     the deferred review pass
 *     harness:done     recon / drain / reflect; never overwrites a row's lane
 * Stages: preflight | build | already-done | review-r<n> | fix-r<n> | fixed | merge | merged |
 *         ci-pending | ci-red | needs-rebase | needs-human | closed-superseded | failed | halted |
 *         done | skipped | recon | drain | reflect
 * The review-mode stages (fixed, ci-pending, ci-red, needs-rebase, needs-human, closed-superseded)
 * are terminal-ish outcomes of review mode v2's fix+land agent and its sweeper.
 *
 * Two builder lanes write this file CONCURRENTLY, so every write takes a directory lock and re-reads
 * inside it. A lost update here is exactly the kind of quiet miscount this loop's telemetry has been
 * burned by before.
 *
 * The timestamp comes from `date -u` on purpose: this is a plain node script and may use it, unlike
 * the generated Workflow scripts, where Date.now()/new Date() are banned outright.
 */
import fs from 'node:fs'
import path from 'node:path'
import { execFileSync } from 'node:child_process'

const DEFAULT_BOARD = '/Users/wilsonguenther/Obsidian/Wilson-Brain/Projects/Loop-Logs/STATUS-yap.md'
const TITLE = '# Loop status — Yap (wilson-voice) overhaul'
const HEADER_RE =
  /^part\s+(\S+)\s+·\s+items\s+(\d+)\/(\d+)\s+done\s+·\s+open\s+loop-build\s+PRs\s+(\S+)\s+·\s+lane A\s+(.*?)\s+·\s+lane B\s+(.*?)\s+·\s+updated\s+(\S+)\s*$/
const ROW_RE = /^\|([^|]*)\|([^|]*)\|([^|]*)\|([^|]*)\|([^|]*)\|([^|]*)\|\s*$/
const TERMINAL = new Set(['merged', 'already-done', 'done'])
const LANES = new Set(['A', 'B', 'review', 'harness'])
const STAGE_RE =
  /^(preflight|build|already-done|review-r\d+|fix-r\d+|fixed|merge|merged|ci-pending|ci-red|needs-rebase|needs-human|closed-superseded|failed|halted|done|skipped|recon|drain|reflect)$/

const nowUtc = () => {
  try {
    return execFileSync('date', ['-u', '+%Y-%m-%dT%H:%M:%SZ'], { encoding: 'utf8' }).trim()
  } catch {
    return 'unknown'
  }
}

// ── args ────────────────────────────────────────────────────────────────────
const argv = process.argv.slice(2)
const flags = {}
const positional = []
for (let i = 0; i < argv.length; i++) {
  if (argv[i].startsWith('--')) {
    const key = argv[i].slice(2)
    flags[key] = argv[i + 1] !== undefined && !argv[i + 1].startsWith('--') ? argv[++i] : 'true'
  } else positional.push(argv[i])
}
const BOARD = flags.board || process.env.LOOP_STATUS_BOARD || DEFAULT_BOARD
const [itemId, rawStage, note, pr] = positional

if (!itemId && !flags.part && !flags.total && !flags.current && !flags.open) {
  console.error('usage: node scripts/loop/status.mjs <itemId> <lane:stage> ["note"] [PR] [--open N]')
  console.error('       node scripts/loop/status.mjs --part part-01 --total 6 --current none')
  process.exit(2)
}
if (itemId && !rawStage) {
  console.error(`status.mjs: <lane:stage> is required with an item (got item ${itemId} and nothing else)`)
  process.exit(2)
}

// A stage this script does not recognise is RECORDED, never fatal: a run must not die because an
// agent invented a word. Say so on stderr and write it through.
let lane = '-'
let stage = rawStage
if (rawStage && rawStage.includes(':')) {
  const [l, ...rest] = rawStage.split(':')
  stage = rest.join(':')
  if (LANES.has(l)) lane = l
  else console.error(`status.mjs: WARNING unknown lane ${JSON.stringify(l)} — recorded as-is`)
  if (!LANES.has(l)) lane = l
}
if (stage && !STAGE_RE.test(stage)) console.error(`status.mjs: WARNING unknown stage ${JSON.stringify(stage)} — recorded as-is`)

// ── cells: a note is free text and may contain the table's own delimiter ────
const cell = (s, max = 150) => {
  const flat = String(s === undefined || s === null ? '' : s)
    .replace(/[\r\n]+/g, ' ')
    .replace(/\|/g, '\\|')
    .trim()
  return flat.length > max ? `${flat.slice(0, max - 1)}…` : flat
}
const prCell = (s) => {
  const flat = cell(s, 24).replace(/^#/, '')
  return flat ? (/^\d+$/.test(flat) ? `#${flat}` : flat) : ''
}

// ── the lock: two builder lanes write this file at the same time ────────────
const lockDir = `${BOARD}.lock`
const acquire = () => {
  for (let attempt = 0; attempt < 100; attempt++) {
    try {
      fs.mkdirSync(lockDir)
      return true
    } catch {
      try {
        // A lock older than 30s belonged to a writer that crashed, not to a live one.
        const age = execFileSync('/bin/sh', ['-c', `echo $(( $(date -u +%s) - $(stat -f %m ${JSON.stringify(lockDir)}) ))`], {
          encoding: 'utf8',
        }).trim()
        if (Number(age) > 30) fs.rmSync(lockDir, { recursive: true, force: true })
      } catch {}
      try {
        execFileSync('/bin/sleep', ['0.1'])
      } catch {}
    }
  }
  return false
}

// ── read ────────────────────────────────────────────────────────────────────
const parse = () => {
  const state = { part: '?', total: null, open: '?', laneA: null, laneB: null, rows: [] }
  if (!fs.existsSync(BOARD)) return state
  for (const line of fs.readFileSync(BOARD, 'utf8').split('\n')) {
    const h = line.match(HEADER_RE)
    if (h) {
      state.part = h[1]
      state.total = Number(h[3])
      state.open = h[4]
      state.laneA = h[5] === '—' ? null : h[5]
      state.laneB = h[6] === '—' ? null : h[6]
      continue
    }
    const r = line.match(ROW_RE)
    if (!r) continue
    const cols = r.slice(1, 7).map((c) => c.trim())
    if (cols[0] === 'item' || /^-+$/.test(cols[0]) || cols[0] === '') continue
    state.rows.push({ item: cols[0], lane: cols[1], stage: cols[2], updated: cols[3], note: cols[4], pr: cols[5] })
  }
  return state
}

const render = (state) => {
  const counted = state.rows.filter((r) => !r.item.startsWith('_'))
  const done = counted.filter((r) => TERMINAL.has(r.stage)).length
  const total = state.total !== null && state.total >= counted.length ? state.total : counted.length
  const cur = (l) => {
    const live = state.rows.filter((r) => r.lane === l && !TERMINAL.has(r.stage) && r.stage !== 'skipped')
    return live.length ? `${live[live.length - 1].item} (${live[live.length - 1].stage})` : '—'
  }
  return [
    TITLE,
    '',
    `part ${state.part} · items ${done}/${total} done · open loop-build PRs ${state.open} · ` +
      `lane A ${cur('A')} · lane B ${cur('B')} · updated ${nowUtc()}`,
    '',
    '| item | lane | stage | last update (UTC) | note | PR |',
    '| --- | --- | --- | --- | --- | --- |',
    ...state.rows.map((r) => `| ${r.item} | ${r.lane} | ${r.stage} | ${r.updated} | ${r.note} | ${r.pr} |`),
    '',
    `<!-- upserted by scripts/loop/status.mjs — one row per item, rollout order. Terminal stages: ${[...TERMINAL].join(', ')}. -->`,
    '',
  ].join('\n')
}

// ── write ───────────────────────────────────────────────────────────────────
const locked = acquire()
try {
  const state = parse()
  if (flags.part) state.part = flags.part
  if (flags.total) state.total = Number(flags.total)
  if (flags.open) state.open = String(flags.open)
  if (flags.current === 'none') {
    state.laneA = null
    state.laneB = null
  }

  if (itemId) {
    const id = cell(itemId, 40)
    const at = state.rows.findIndex((r) => r.item === id)
    const prev = at >= 0 ? state.rows[at] : null
    const row = {
      item: id,
      // The harness lanes (recon/drain/reflect) report ON an item without owning it, so they never
      // relabel which builder lane actually built it.
      lane: lane === 'harness' && prev && prev.lane && prev.lane !== '-' ? prev.lane : lane,
      stage: cell(stage, 24),
      updated: nowUtc(),
      note: cell(note || ''),
      // Keep a PR number an earlier stage already learned when this call did not pass one.
      pr: prCell(pr || '') || (prev ? prev.pr : ''),
    }
    if (at >= 0) state.rows[at] = row
    else state.rows.push(row)
  }

  fs.mkdirSync(path.dirname(BOARD), { recursive: true })
  const tmp = `${BOARD}.tmp-${process.pid}`
  fs.writeFileSync(tmp, render(state))
  fs.renameSync(tmp, BOARD)

  const counted = state.rows.filter((r) => !r.item.startsWith('_'))
  const done = counted.filter((r) => TERMINAL.has(r.stage)).length
  console.log(
    `status: ${itemId ? `${itemId} → ${lane}:${stage}` : `part ${state.part}`} · ${done}/${
      state.total !== null && state.total >= counted.length ? state.total : counted.length
    } done · ${BOARD}${locked ? '' : ' (WARNING: wrote without the lock)'}`
  )
} finally {
  if (locked) {
    try {
      fs.rmSync(lockDir, { recursive: true, force: true })
    } catch {}
  }
}
