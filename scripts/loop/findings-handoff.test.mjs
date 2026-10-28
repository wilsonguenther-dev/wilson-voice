// scripts/loop/findings-handoff.test.mjs — node --test. HERMETIC (no gh, no agents).
// Guards the reviewer -> fixer findings handoff. Root cause it pins (2026-10-27, review pass wf_f3b32723): agent()
// returns seats as JSON TEXT, and blockingOf / recordHead read `v.blocking` / `v.recordHead` only on objects, so the
// PARK seat for Y5-F (#280) was handed {primary:[],lens:[]} and the recordHead checks compared nothing.
// Non-empty findings must pass through verbatim; an empty BLOCK must take the re-ask -> loop-review-error path.
import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import path from 'node:path'

const HERE = path.dirname(new URL(import.meta.url).pathname)
const TEMPLATE = fs.readFileSync(path.join(HERE, 'template.mjs'), 'utf8')
const REPO = 'wilsonguenther-dev/wilson-voice'
const helpersSrc = (TEMPLATE.match(/\/\/ ── verdict helpers[^\n]*\n([\s\S]*?)\nconst SHA40 = /) || [])[1] || ''
const H = new Function('REPO', `${helpersSrc}\nreturn { asObj, blockingOf, recordHeadOf, isMalformedBlock, findingsFor, FINDINGS_REASK }`)(REPO)

const HEAD = '4dcae1b79d6e76797506ec77e1024c81880fe25f'
const B1 = 'BLOCKING: The hallucination-gate rejection now shows the wrong toast line. In desktop/src-tauri/src/lib.rs around line 3101 ... "quoted" \\ path'
const B2 = 'BLOCKING 1 (claims vs diff): the clipboard plugin\'s raw error still reaches the toast'

test('the verdict helpers are found in template.mjs', () => assert.ok(helpersSrc.includes('const findingsFor'), 'could not extract the verdict helpers'))

test('non-empty findings pass through unchanged — object return', () => {
  const v = { verdict: 'BLOCK', blocking: [B1, B2], recordHead: HEAD }
  assert.equal(H.blockingOf(v), v.blocking)
  assert.deepEqual(H.findingsFor(v, 280), [B1, B2])
  assert.equal(H.isMalformedBlock(v), false)
})

test('non-empty findings pass through unchanged — JSON-text return (what agent() actually hands back)', () => {
  const v = JSON.stringify({ status: 'reviewed', verdict: 'BLOCK', blocking: [B1, B2], advisory: ['A1'], recordHead: HEAD, recordPosted: true })
  assert.deepEqual(H.blockingOf(v), [B1, B2])
  assert.deepEqual(H.findingsFor(v, 280), [B1, B2])
  assert.equal(H.isMalformedBlock(v), false)
  assert.equal(H.recordHeadOf(v), HEAD)
  // fenced / prefixed text still parses
  assert.deepEqual(H.blockingOf('Result:\n```json\n' + v + '\n```'), [B1, B2])
})

test('a PASS hands the fixer nothing and is never malformed', () => {
  const v = JSON.stringify({ verdict: 'PASS', blocking: [], advisory: ['x'] })
  assert.deepEqual(H.findingsFor(v, 280), [])
  assert.equal(H.isMalformedBlock(v), false)
})

test('an empty BLOCK is malformed (empty or missing blocking[]), object or text', () => {
  assert.equal(H.isMalformedBlock(JSON.stringify({ verdict: 'BLOCK', blocking: [] })), true)
  assert.equal(H.isMalformedBlock(JSON.stringify({ verdict: 'BLOCK', notes: 'see comment' })), true)
  assert.equal(H.isMalformedBlock({ verdict: 'BLOCK' }), true)
  // unparsable prose keeps the old rule: malformed only when it literally shows "blocking": []
  assert.equal(H.isMalformedBlock('"verdict": "BLOCK" and "blocking": ["x" (truncated'), false)
  assert.equal(H.isMalformedBlock('"verdict": "BLOCK", "blocking": [] (truncated'), true)
})

test('an empty BLOCK falls back to the reviewer record comment — by recordCommentId when present', () => {
  const byId = H.findingsFor(JSON.stringify({ verdict: 'BLOCK', blocking: [], recordCommentId: 6440051505 }), 280)
  assert.equal(byId.length, 1)
  assert.match(byId[0], new RegExp(`gh api repos/${REPO}/issues/comments/6440051505 --jq \\.body`))
  const noId = H.findingsFor({ verdict: 'BLOCK', blocking: [] }, 280)
  assert.match(noId[0], new RegExp(`gh api repos/${REPO}/issues/280/comments`))
  assert.match(noId[0], /test\("\^seat: review-opus2\?\$"; "m"\)/)
  assert.doesNotMatch(noId[0], /loop-review/i) // the fixer prompt never carries the marker word
})

test('the review loop re-asks an empty BLOCK once, then marks loop-review-error before any fix round', () => {
  const at = (s) => { const i = TEMPLATE.indexOf(s); assert.ok(i >= 0, `missing: ${s}`); return i }
  const reaskA = at("agentR(primaryPrompt(round, prior) + FINDINGS_REASK, { model: 'opus', effort: 'high', phase: itemId, label: `review-opus:${itemId}` })")
  const reaskB = at("agentR(lensPrompt(round, prior) + FINDINGS_REASK, { model: 'opus', effort: 'high', phase: itemId, label: `review-opus2:${itemId}` })")
  const mark = at("agentR(reviewErrorPrompt(why), { model: 'opus', effort: 'high', phase: itemId, label: `review-error:${itemId}` })")
  const ret = at("status: 'loop-review-error'")
  const fix = at("fixLandPrompt('fix', round + 1)")
  const park = at("fixLandPrompt('park', round)")
  assert.ok(reaskA < mark && reaskB < mark && mark < ret && ret < fix && ret < park, 're-ask -> mark -> return must precede the fix and park dispatches')
  assert.match(H.FINDINGS_REASK, /RETURN THE FINDINGS ARRAY/)
  assert.match(H.FINDINGS_REASK, /do NOT post another/)
})

test('the review-error seat labels loop-review-error and never starts a comment line with the record marker', () => {
  const mode = (TEMPLATE.match(/function reviewErrorPrompt\(why\) \{([\s\S]*?)\n  \}\n/) || [])[1] || ''
  assert.ok(mode, 'reviewErrorPrompt missing from template.mjs')
  assert.match(mode, /--add-label loop-review-error/)
  assert.match(mode, /status: "loop-review-error"/)
  assert.doesNotMatch(mode, /--body "loop-review/)
  assert.match(mode, /--body "Harness error, not a verdict:/)
  assert.doesNotMatch(mode, /gh pr merge|git push|review-gate\.mjs record/)
})

test('every fixer/parker/re-review handoff reads findings through the string-safe helper', () => {
  assert.doesNotMatch(TEMPLATE, /Array\.isArray\(v\.blocking\)/)
  assert.doesNotMatch(TEMPLATE, /typeof v === 'object' && v\.recordHead/)
  assert.match(TEMPLATE, /OPEN BLOCKING FINDINGS: \$\{JSON\.stringify\(\{ primary: findingsFor\(primary, pr\), lens: findingsFor\(lens, pr\) \}\)\}/)
  assert.match(TEMPLATE, /OPEN BLOCKING FINDINGS, VERBATIM: \$\{JSON\.stringify\(\{ primary: findingsFor\(primary, pr\), lens: findingsFor\(lens, pr\) \}\)\}/)
  assert.match(TEMPLATE, /primaryBlocking: findingsFor\(primary, pr\),\n\s+lensBlocking: findingsFor\(lens, pr\),/)
  assert.match(TEMPLATE, /recordCommentId/)
})
