/**
 * scripts/loop/seat-models.mjs — validate scripts/loop/seat-models.json and stamp it onto the template's
 * `const SEAT_MODELS = {}` line (loop/model-tiering, 2026-10-16).
 *
 * The map moves MECHANICAL seats (fixed command list in, fixed RETURN shape out) to a smaller model.
 * The floor below is hard-coded on purpose: editing the JSON's own opusOnly list can never move a
 * builder, fixer, lander, reviewer or verdict seat off opus — build.mjs fails instead.
 */
import fs from 'node:fs'

/** Seats that judge code or product, or that rebase/resolve/merge. Never tiered, in any repo. */
export const OPUS_FLOOR = Object.freeze([
  'build', 'fix', 'fix-round', 'fix-land',
  'review-opus', 'review-opus2', 'security-seat', 'review-triage',
  'drain', 'land-sweeper', 'land-mode-sweeper', 'part-land',
  'prod-verify', 'main-verify', 'walk',
])
export const MODELS = Object.freeze(['opus', 'sonnet', 'haiku'])
export const EFFORTS = Object.freeze(['low', 'medium', 'high', 'xhigh', 'max'])
const LINE = /^const SEAT_MODELS = \{\}.*$/gm

/** Pure check of a parsed map against the template text. Returns a list of problems (empty = valid). */
export function seatModelProblems(map, template) {
  const out = []
  if (!map || typeof map !== 'object' || Array.isArray(map)) return ['seat-models.json: must be a JSON object']
  const known = new Set(['enabled', 'escalate', 'opusOnly', 'seats'])
  for (const k of Object.keys(map)) if (!k.startsWith('_') && !known.has(k)) out.push(`seat-models.json: unknown key "${k}"`)
  if (typeof map.enabled !== 'boolean') out.push('seat-models.json: "enabled" must be true or false')
  const esc = map.escalate
  if (!esc || esc.model !== 'opus') out.push('seat-models.json: escalate.model must be "opus" — the quality floor is an opus re-run')
  if (esc && esc.effort !== undefined && !EFFORTS.includes(esc.effort)) out.push(`seat-models.json: escalate.effort must be one of ${EFFORTS.join('|')}`)
  if (map.opusOnly !== undefined && (!Array.isArray(map.opusOnly) || !map.opusOnly.every((s) => typeof s === 'string'))) out.push('seat-models.json: opusOnly must be an array of seat names')
  const protectedSeats = new Set([...OPUS_FLOOR, ...(Array.isArray(map.opusOnly) ? map.opusOnly : [])])
  const seats = map.seats
  if (!seats || typeof seats !== 'object' || Array.isArray(seats)) {
    out.push('seat-models.json: "seats" must be an object of {seat: {model, effort, require}}')
    return out
  }
  for (const [seat, t] of Object.entries(seats)) {
    if (!/^[a-z][a-z0-9-]*$/.test(seat)) { out.push(`seat-models.json: seat name "${seat}" must be a label prefix (a-z, 0-9, -)`); continue }
    if (protectedSeats.has(seat)) out.push(`seat-models.json: "${seat}" is an opus-only seat (builder, fixer, lander, reviewer or verdict) — it can never be tiered`)
    if (!t || typeof t !== 'object') { out.push(`seat-models.json: seats.${seat} must be an object`); continue }
    if (!MODELS.includes(t.model)) out.push(`seat-models.json: seats.${seat}.model must be one of ${MODELS.join('|')}`)
    if (t.effort !== undefined && !EFFORTS.includes(t.effort)) out.push(`seat-models.json: seats.${seat}.effort must be one of ${EFFORTS.join('|')}`)
    if (t.require !== undefined && (!Array.isArray(t.require) || !t.require.every((k) => typeof k === 'string' && /^[A-Za-z_][A-Za-z0-9_]*$/.test(k)))) {
      out.push(`seat-models.json: seats.${seat}.require must be an array of RETURN field names`)
    }
    for (const k of Object.keys(t)) if (!['model', 'effort', 'require'].includes(k)) out.push(`seat-models.json: seats.${seat} has unknown key "${k}"`)
    // A typo'd seat would silently never match a label: prove the template dispatches it.
    if (typeof template === 'string' && !template.includes(`${seat}:`) && !template.includes(`'${seat}'`)) {
      out.push(`seat-models.json: seat "${seat}" matches no agent label in the template (labels are "<seat>:..." or '<seat>')`)
    }
  }
  return out
}

/** Stamp the validated map onto the template. Missing file = stamp nothing (every seat stays opus). */
export function stampSeatModels(template, file) {
  const n = (template.match(LINE) || []).length
  if (n !== 1) return { template, problems: [`template must declare \`const SEAT_MODELS = {}\` exactly once to be stamped (found ${n})`], summary: 'none' }
  if (!fs.existsSync(file)) return { template, problems: [], summary: 'no seat-models.json — every seat opus' }
  let map
  try {
    map = JSON.parse(fs.readFileSync(file, 'utf8'))
  } catch (e) {
    return { template, problems: [`seat-models.json: not valid JSON (${e.message})`], summary: 'none' }
  }
  const problems = seatModelProblems(map, template)
  if (problems.length) return { template, problems, summary: 'none' }
  const clean = { enabled: map.enabled, escalate: map.escalate, seats: map.seats }
  const stamped = template.replace(LINE, () => `const SEAT_MODELS = ${JSON.stringify(clean)} // stamped by build.mjs from scripts/loop/seat-models.json`)
  const tiered = Object.entries(map.seats).map(([s, t]) => `${s}→${t.model}`)
  return { template: stamped, problems: [], summary: map.enabled ? `${tiered.length} mechanical seat(s) tiered (${tiered.join(', ')}), escalate→opus` : 'disabled — every seat opus' }
}
