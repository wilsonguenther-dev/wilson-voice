/**
 * scripts/loop/stall.mjs — THE STALL RULES, the source of truth (item yap-loop-stall-detector).
 *
 * Audit 2026-10-26: the land gate required `cargo fmt` while main was red on it, so every PASS/PASS
 * PR came back ci-red for the same command for two hours and nothing noticed. These pure functions
 * are how the loop notices next time:
 *   1. sameReasonTwice — one PR, the same land status + reason twice in a row -> loop-stalled
 *   2. gateRedOnMain   — a BLOCKING gate command red (or unreadable) on main -> halt before review
 *   3. noProgress      — the last 3 land results are gate failures on 3 distinct PRs -> stop pulling
 *
 * The generated workflow is ONE script with no imports, so template.mjs cannot import this file.
 * Instead it carries a BYTE-IDENTICAL copy of everything between the STALL-RULES-BEGIN /
 * STALL-RULES-END marker lines below; stall-detector.test.mjs fails on any drift. Edit here, then
 * paste the block into template.mjs. Pure: no I/O and no imports, so it runs in the Workflow sandbox.
 *
 * The two flag arrays are declared OUTSIDE the block: template.mjs declares them immediately above
 * GATE_CMDS (whose echo labels must be exactly these names), and the test checks both copies agree.
 */
export const GATE_BLOCKING = ['typecheck', 'test', 'build', 'polish', 'diarize', 'stage', 'polishtest', 'diarizetest', 'clippy', 'cargotest']
export const GATE_ADVISORY = ['fmt']

// STALL-RULES-BEGIN — byte-identical in scripts/loop/stall.mjs and scripts/loop/template.mjs
/** An exit code is readable only as an integer (a number, or a string of digits from a text payload). */
function exitCodeOf(v) {
  if (typeof v === 'number' && Number.isInteger(v)) return v
  if (typeof v === 'string' && /^-?\d+$/.test(v.trim())) return Number(v.trim())
  return null
}
/** The first non-zero BLOCKING command in gateExits, in GATE_BLOCKING order, or null. */
function firstBlockingRed(gateExits) {
  if (!gateExits || typeof gateExits !== 'object') return null
  for (const cmd of GATE_BLOCKING) {
    const code = exitCodeOf(gateExits[cmd])
    if (code !== null && code !== 0) return cmd
  }
  return null
}
/**
 * Rule 1 input. landResult carries the fix-land status the harness already derived, plus whatever the
 * LAND seat returned. For ci-red the reason is the failing BLOCKING command; otherwise the reason field.
 */
function stallReason(landResult) {
  const r = landResult && typeof landResult === 'object' ? landResult : {}
  const status = typeof r.status === 'string' && r.status ? r.status : 'unknown'
  const text = (v) => (typeof v === 'string' && v.trim() ? v.trim() : null)
  if (status === 'ci-red') {
    return { status, reason: firstBlockingRed(r.gateExits) || text(r.failingCommand) || text(r.reason) || 'unknown' }
  }
  return { status, reason: text(r.reason) || 'none' }
}
/** Rule 1. True when the most recent prior entry for the SAME pr has the same status and reason (never 'merged'). */
function sameReasonTwice(history, next) {
  if (!next || next.status === 'merged') return false
  const list = Array.isArray(history) ? history : []
  for (let i = list.length - 1; i >= 0; i--) {
    const h = list[i]
    if (!h || String(h.pr) !== String(next.pr)) continue
    return h.status === next.status && h.reason === next.reason
  }
  return false
}
/** Rule 2. Blocking non-zero -> red; blocking missing / non-integer -> unreadable (fail closed); advisory non-zero -> advisoryRed. */
function gateRedOnMain(gateExits, blocking, advisory) {
  const exits = gateExits && typeof gateExits === 'object' ? gateExits : {}
  const red = []
  const unreadable = []
  const advisoryRed = []
  for (const cmd of blocking || []) {
    const code = exitCodeOf(exits[cmd])
    if (code === null) unreadable.push(cmd)
    else if (code !== 0) red.push(cmd)
  }
  for (const cmd of advisory || []) {
    const code = exitCodeOf(exits[cmd])
    if (code !== null && code !== 0) advisoryRed.push(cmd)
  }
  return { red, unreadable, advisoryRed }
}
/** Rule 3 input. A land outcome is a gate failure when it is ci-red or carries a non-zero BLOCKING exit. */
function isGateFailure(landResult) {
  const r = landResult && typeof landResult === 'object' ? landResult : {}
  return r.status === 'ci-red' || firstBlockingRed(r.gateExits) !== null
}
/** Rule 3. The LAST n land outcomes are n DISTINCT PRs, none merged, every one a gate failure. */
function noProgress(results, n = 3) {
  const list = Array.isArray(results) ? results : []
  if (list.length < n) return false
  const last = list.slice(-n)
  if (new Set(last.map((r) => String(r && r.pr))).size !== n) return false
  return last.every((r) => r && r.status !== 'merged' && r.gateFailure === true)
}
// STALL-RULES-END

export { exitCodeOf, firstBlockingRed, stallReason, sameReasonTwice, gateRedOnMain, isGateFailure, noProgress }
