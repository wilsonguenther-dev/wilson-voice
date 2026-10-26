#!/usr/bin/env node
/**
 * scripts/loop/review-gate.mjs — THE REVIEW INPUT TO EVERY YAP LAND (yap-land-gate-pass-required, 2026-10-17).
 * Opus only, every seat. Modelled line for line on Drivia Learn's H-GATE-1 gate (Learn repo, main branch,
 * scripts/loop/review-gate.mjs), with ONE deliberate difference: there is NO unreviewed exception in Yap.
 *
 * Why: the review pass 2 audit (pass wf_d0209003-efe) found that every merged head carried only the FIXER's
 * disposition review, never a reviewer PASS: #178 96e5380, #192 45fa4cd, #195 1dfb328, #212 0b7449b and #213
 * d956c11 were BLOCKed by the reviewer on an earlier head and self-attested FIXED by the fixer on the merged head;
 * #196 16f7241 had a reviewer PASS on cb60606 only; #214 8bfd8a5 had no Opus review at all. The sweeper's gate was
 * CI on headRefOid + build/fmt/clippy. Nothing bound a reviewer's PASS to the sha that merged.
 *
 * TRUST LIMIT — read this before relying on the gate. Every seat posts through the same GitHub account, so the
 * record's seat field is self-declared: the gate CANNOT authenticate which seat wrote a record, and a seat that
 * runs "record --seat review-opus" is indistinguishable from the real reviewer. What this item enforces is (a) a
 * PASS bound to the current head exists, and (b) only reviewer prompts are ever handed the record subcommand
 * (validation in 5 — build.mjs, via promptGateProblems() below). This gate is NOT authentication and is NOT
 * unforgeable. Seat authentication (per-seat credentials, or records the controller signs) is out of scope here.
 *
 * THE RECORD — a PR comment the reviewer seat posts itself (`node scripts/loop/review-gate.mjs record ...` writes
 * it), bound to the head sha it reviewed. The first non-empty line is the marker; each header field exactly once:
 *     loop-review (Opus)
 *     seat: review-opus | review-opus2
 *     lens: <free text>
 *     head: <the 40-hex head sha the seat read>
 *     verdict: PASS blocking=0 | BLOCK blocking=<n>
 * Free-text notes follow the first blank line and are never read as header fields.
 * Not a record, ever: an author outside REVIEW_AUTHORS (the account the seats post as); a bot author.
 *
 * RECOGNITION (review-opus B3) — liberal for BLOCK, strict for PASS. From an allowed author:
 *   - a comment with "loop-review" ANYWHERE (``` / ~~~ fence lines stripped first) AND any line matching
 *     BLOCK_LINE_RE is a BLOCK — no positional rule (`1. loop-review`, `| loop-review |`, `<b>loop-review</b>`,
 *     an emoji or `Re:` before it, a mid-sentence mention above an explicit verdict line: all BLOCK);
 *   - a comment with the marker and NO verdict line at all is an unparsable BLOCK only when the marker starts a
 *     line (MARKER_RE, leading markdown allowed); a verdict-less mid-sentence mention is not a record. This is
 *     the ONLY place the line-start rule applies (review-opus round 4);
 *   - a marker anywhere plus a verdict line that is neither BLOCK nor a clean PASS (`verdict: FAIL`) is an
 *     unparsable BLOCK;
 *   - any line matching BLOCK_LINE_RE /verdict\s*:.*\bBLOCK\b/i — bold, "Verdict :", a blank line between header
 *     lines, in the notes, inside a fence — makes it a BLOCK (blocking= missing or unparsable → 1);
 *   - it is a PASS ONLY when it is a CLEAN header: first non-empty line the marker, each field once, seat in
 *     REVIEW_SEATS, a 40-hex head, `verdict: PASS blocking=0`, and the comment is unedited;
 *   - anything else carrying the marker (`verdict: FAIL`, a non-PASS word, an edited PASS, a PASS from a fixer or an
 *     unlisted seat, a short head, no verdict at all) is an "unparsable record", treated as BLOCK (fails closed).
 * SCOPE (review-opus2 a): a BLOCK applies to the head(s) it names on `head:` lines (40 hex exact, 7..39 hex as a
 *   prefix). A BLOCK naming no readable head applies to the PR head that was current when the comment was created:
 *   the newest PR commit whose committedDate <= the comment's created_at (`gh pr view --json commits`). It never
 *   applies to a later head. LIMIT: committer dates are not push times; a commit dated after the comment is read
 *   as a later head.
 * CLEARING (review-opus advisory, review-opus2 b): a BLOCK on head H is FINAL for H. No later PASS from any seat
 *   on H clears it, the same seat's included. The only way past is a NEW push (a new head) followed by a complete
 *   fresh round: an unedited PASS from every required seat on the new head and no BLOCK scoped to it. Deleting the
 *   comment removes it from the gate's input — that is outside the gate's control, and a human act.
 * ROUTING (review-opus2): a refusal caused by a BLOCK from a seat outside REVIEW_SEATS (a hand-posted independent
 *   review, or no readable seat) prints `review: needs-human — ...` with the blocking comment id(s); the merge
 *   paths label that PR needs-human (not review-pending) and triage skips needs-human PRs.
 *
 * THE DECISION — reviewDecision(), pure, against the PR's CURRENT head only:
 *   SECURITY class (securityClassOf: an item id in SECURITY_IDS, a security label, a file under SECURITY_PATHS, or
 *   a file list longer than one page): a PASS from review-opus AND a PASS from review-opus2 on the head — two
 *   DIFFERENT seats; two PASSes from one seat are one review.
 *   STANDARD: one PASS from either seat on the head.
 *   NO record on the head refuses. There is no unreviewed exception in Yap (Learn's "build-mode LAND" pass-through
 *   is deliberately NOT ported). ANY BLOCK scoped to the head refuses, whatever was posted after it.
 *
 *   node scripts/loop/review-gate.mjs gate --pr N [--head SHA] [--repo o/r]
 *        last stdout line `review: ...`; exit 0 may land · 6 refused (missing, blocked, head moved, unreadable,
 *        repo owner not PINNED_OWNER) · 2 `--head` given with no value (never defaults to the current head) · 64 usage
 *   The author allowlist is the pinned REVIEW_AUTHORS only; a --repo / LOOP_REPO whose owner is not PINNED_OWNER is
 *   refused, so a user-supplied repo string can never widen whose comments count.
 *   node scripts/loop/review-gate.mjs record --pr N --seat review-opus|review-opus2 --lens "<lens>" --head SHA
 *        --verdict PASS|BLOCK --blocking N [--notes-file f] [--repo o/r]
 *        posts the record; refused (exit 64) when SHA is not the PR's current head or the fields are malformed
 *   node scripts/loop/review-gate.mjs klass --pr N [--repo o/r]     → `security <why>` | `standard`
 *
 * Every merge path (build-mode LAND, fix-land, the land sweeper, the drain) runs
 *     node scripts/loop/review-gate.mjs gate --pr N --head <headRefOid>
 * immediately before `gh pr merge --match-head-commit <the same sha>`, and does not merge on a non-zero exit.
 */
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { pathToFileURL } from 'node:url'

export const DEFAULT_REPO = 'wilsonguenther-dev/wilson-voice'
export const RECORD_MARKER = 'loop-review (Opus)'
export const PRIMARY_SEAT = 'review-opus'
export const SECURITY_LENS_SEAT = 'review-opus2'
export const REVIEW_SEATS = [PRIMARY_SEAT, SECURITY_LENS_SEAT]
export const FIXER_SEAT_RE = /^fix/i
export const EXIT_REVIEW = 6
export const EXIT_USAGE = 64
/** `--head` present with no value: a hard error, never "use the current head". */
export const EXIT_BAD_FLAG = 2
/** The repo owner every gated repo must have; the author allowlist is pinned, never derived from --repo. */
export const PINNED_OWNER = 'wilsonguenther-dev'
/** The seat key of a BLOCK with no single readable seat line. */
export const UNNAMED_SEAT = '(no seat)'
export const PAGE_CAP = 20
/** One page of `pulls/N/files` is 100 rows: a PR whose file list does not fit one page is security class. */
export const FILE_PAGE = 100
export const SECURITY_LABEL_RE = /^(security|sec)$/i
/** The ONLY logins whose records count: the account every Opus seat posts as (gh api user → wilsonguenther-dev). */
export const REVIEW_AUTHORS = ['wilsonguenther-dev']
/**
 * A SUPERSET of template.mjs's SECURITY_PATHS (the gate is the enforcement, so it may never call standard what the
 * review pass seated as security): the capability files, tauri.conf.json, the updater / signing path, the local
 * SQLite history and its migrations, the polish_protocol / sidecar IPC, any path matching the trust keywords (licensing /
 * entitlement included), and
 * the loop harness itself (scripts/loop/, this file included) and CI.
 */
export const SECURITY_PATHS = [
  /^desktop\/src-tauri\/capabilities\//, /^desktop\/src-tauri\/tauri(?:\.[\w-]+)?\.conf\.json$/, /^desktop\/src-tauri\/src\/db/,
  /migration/i, /polish_protocol|sidecar/i,
  /updater|sign|notariz|keychain|licens|entitle|permission|accessibility|microphone|network|http|upload|telemetry/i,
  /^scripts\/loop\//, /^scripts\/cicd-loop-all\.mjs$/, /^\.github\/workflows\//,
]
/**
 * template.mjs SECURITY_IDS ('SEC-|UPD-|PRIV-|DB-|PERM-|YAP-LIC-'): an item id at the start of the PR title. YAP-LIC- (the
 * licensing client: token store, activation, entitlement) is here so its two-seat class never rests on a file name.
 */
export const SECURITY_ID_RE = /^\s*(?:SEC-|UPD-|PRIV-|DB-|PERM-|YAP-LIC-)/i
const SHA_RE = /^[0-9a-f]{40}$/
/** A BLOCK verdict, anywhere in a record (bold, spaced, fenced, in notes). */
export const BLOCK_LINE_RE = /verdict\s*:.*\bBLOCK\b/i
/** The marker at a LINE START (markdown decoration allowed); a mid-sentence mention is not a record. */
const MARKER_RE = /^[\s*_>#`~-]*loop-review/i
const MARKER_ANYWHERE_RE = /loop-review/i
const VERDICT_LINE_RE = /verdict\s*:/i
const FENCE_RE = /^(?:```|~~~)/
const HEAD_LINE_RE = /\bhead\W*:\W*([0-9a-f]{7,40})(?![0-9a-f])/gi
const SEAT_LINE_RE = /^\W*seat\W*:\W*([\w.-]+)/i
const sha9 = (s) => String(s || '').slice(0, 9) || '(none)'
const isBot = (u) => Boolean(u) && (u.type === 'Bot' || /\[bot\]$/i.test(String(u.login || '')))

/** True when the PR is SECURITY class: a security item id, a security label, a security path, or more files than one page lists. */
export function securityClassOf({ files = [], filesTruncated = false, labels = [], title = '' }) {
  if (SECURITY_ID_RE.test(String(title || ''))) return { security: true, why: `item id ${String(title).trim().split(/[\s:]/)[0]}` }
  const lab = (labels || []).map((l) => (typeof l === 'string' ? l : l && l.name)).find((l) => SECURITY_LABEL_RE.test(String(l || '').trim()))
  if (lab) return { security: true, why: `label ${lab}` }
  if (filesTruncated || (files || []).length > FILE_PAGE) return { security: true, why: 'more files than one page lists' }
  const hit = (files || []).find((f) => SECURITY_PATHS.some((re) => re.test(String(f || ''))))
  return hit ? { security: true, why: `touches ${hit}` } : { security: false, why: '' }
}

/** The strict PASS parse: {kind: 'pass', seat, lens, head, verdict, blocking} | {malformed: why}. */
function cleanPass(lines) {
  if (lines.find(Boolean) !== RECORD_MARKER) return { malformed: 'the first non-empty line is not the marker' }
  // Fields are read from the HEADER only: the marker line through the first blank line after it.
  const start = lines.indexOf(RECORD_MARKER) + 1
  const blank = lines.indexOf('', start)
  const header = lines.slice(start, blank === -1 ? lines.length : blank)
  const field = (k) => header.filter((l) => l.toLowerCase().startsWith(`${k}:`)).map((l) => l.slice(k.length + 1).trim())
  const one = {}
  for (const k of ['seat', 'lens', 'head', 'verdict']) {
    const v = field(k)
    if (v.length !== 1) return { malformed: `${v.length ? 'more than one' : 'no'} ${k}: line` }
    one[k] = v[0]
  }
  const head = one.head.toLowerCase()
  if (!SHA_RE.test(head)) return { malformed: `head "${one.head.slice(0, 50)}" is not a full 40-hex sha` }
  if (!/^PASS\s+blocking=0$/.test(one.verdict)) return { malformed: `verdict "${one.verdict.slice(0, 50)}" is not "PASS blocking=0"` }
  if (!REVIEW_SEATS.includes(one.seat)) return { malformed: `seat ${one.seat} is not a reviewer seat (${REVIEW_SEATS.join(', ')})` }
  return { kind: 'pass', seat: one.seat, lens: one.lens, head, verdict: 'PASS', blocking: 0 }
}

/**
 * One comment body → null (no marker: not a record) | a clean PASS {kind: 'pass', seat, lens, head, ...} |
 * a BLOCK {kind: 'block' | 'unparsable', seat, heads: [hex] | null, verdict: 'BLOCK', blocking >= 1, why}.
 * Every marker comment that is not a clean PASS is a BLOCK (fails closed).
 */
export function parseRecord(body) {
  const lines = String(body || '').split('\n').map((l) => l.trim())
  const text = lines.filter((l) => !FENCE_RE.test(l))
  if (!text.some((l) => MARKER_ANYWHERE_RE.test(l))) return null
  const explicit = text.some((l) => BLOCK_LINE_RE.test(l))
  const clean = explicit ? null : cleanPass(lines)
  if (clean && clean.kind === 'pass') return clean
  // Verdict-less: a record only when the marker starts a line; a mid-sentence mention with no verdict is prose.
  if (!explicit && !text.some((l) => VERDICT_LINE_RE.test(l)) && !text.some((l) => MARKER_RE.test(l))) return null
  const seats = [...new Set(text.map((l) => l.match(SEAT_LINE_RE)).filter(Boolean).map((m) => m[1]))]
  const heads = [...new Set([...text.join('\n').matchAll(HEAD_LINE_RE)].map((m) => m[1].toLowerCase()))]
  const bm = explicit ? text.filter((l) => BLOCK_LINE_RE.test(l)).map((l) => l.match(/blocking\W*=\W*(\d+)/i)).find(Boolean) : null
  const blocking = Math.max(1, bm ? Number(bm[1]) : 1)
  const why = explicit ? (bm && Number(bm[1]) >= 1 ? '' : 'blocking= missing, unparsable or 0 — treated as blocking=1') : `unparsable record (${clean.malformed}) — treated as BLOCK`
  return { kind: explicit ? 'block' : 'unparsable', seat: seats.length === 1 ? seats[0] : UNNAMED_SEAT, heads: heads.length ? heads : null, verdict: 'BLOCK', blocking, why }
}

/** The PR head current at `at`: the newest commit with committedDate <= at → its oid, or null (every commit is later). */
export function headAt(commits, at) {
  const t = Date.parse(at)
  let best = null
  for (const c of commits || []) {
    const d = Date.parse(c && c.committedDate)
    if (Number.isFinite(d) && Number.isFinite(t) && d <= t && (!best || d >= best.d)) best = { d, oid: String(c.oid || '').toLowerCase() }
  }
  return best ? best.oid : null
}

/**
 * Issue comments [{user, body, created_at, updated_at, id}] + the PR's commits [{oid, committedDate}] →
 * {records: [...], ignored: [why]} (oldest first). A headless BLOCK gets postedHead = headAt(commits, created_at);
 * without a commit list (a library caller) it is '*' — every head (fails closed).
 */
export function recordsFrom(comments, { authors = REVIEW_AUTHORS, commits } = {}) {
  const records = []
  const ignored = []
  const allowed = new Set((authors || []).map((a) => String(a || '').trim().toLowerCase()).filter(Boolean))
  for (const c of comments || []) {
    let r = parseRecord(c && c.body)
    if (r === null) continue
    if (isBot(c.user)) { ignored.push(`comment ${c.id}: bot author`); continue }
    if (!allowed.has(String((c.user && c.user.login) || '').toLowerCase())) { ignored.push(`comment ${c.id}: author ${(c.user && c.user.login) || '(none)'} is not a review seat account`); continue }
    const at = String(c.created_at || '')
    const edited = Boolean(c.updated_at && c.created_at && c.updated_at !== c.created_at)
    if (r.kind === 'pass' && edited) r = { kind: 'unparsable', seat: r.seat, heads: [r.head], verdict: 'BLOCK', blocking: 1, why: 'unparsable record (edited PASS — re-post, never rebind) — treated as BLOCK' }
    if (r.kind === 'pass') { records.push({ ...r, edited: false, at, id: c.id }); continue }
    const postedHead = r.heads ? null : Array.isArray(commits) ? headAt(commits, at) : '*'
    records.push({ ...r, edited, postedHead, at, id: c.id })
  }
  records.sort((a, b) => a.at.localeCompare(b.at))
  return { records, ignored }
}

/**
 * THE DECISION. Pure. head: the PR's current head. security: securityClassOf(...).security. records: recordsFrom().records.
 * → {state: 'pass'|'missing'|'blocked'|'needs-human', block, passedBy, missing: [why], blockedBy: [desc], needsHuman: [comment id], line}
 * A BLOCK scoped to the head is FINAL for it; no record on the head refuses (no unreviewed exception).
 */
export function reviewDecision({ head, security = false, records = [] }) {
  const h = String(head || '').toLowerCase()
  const klass = security ? 'security' : 'standard'
  const none = { passedBy: [], missing: [], blockedBy: [], needsHuman: [] }
  if (!SHA_RE.test(h)) return { ...none, state: 'missing', block: true, missing: ['no full head sha to bind a review to'], line: `review: refused — no full head sha (${klass})` }
  const scoped = (r) => r.kind === 'pass' ? r.head === h
    : r.heads ? r.heads.some((x) => x === h || (x.length < 40 && h.startsWith(x)))
    : r.postedHead === '*' || r.postedHead === h
  const blocks = records.filter((r) => r.kind !== 'pass' && scoped(r))
  const passedBy = [...new Set(records.filter((r) => r.kind === 'pass' && scoped(r)).map((r) => r.seat))]
  const desc = (r) => `${r.seat} (comment ${r.id}${r.edited ? '; edited' : ''}${r.why ? `; ${r.why}` : ''})`
  if (blocks.length) {
    const outside = blocks.filter((r) => !REVIEW_SEATS.includes(r.seat))
    const blockedBy = blocks.map(desc)
    const final = 'a BLOCK is final for this head: push a fix, then a complete fresh round on the new head'
    if (outside.length) return { ...none, state: 'needs-human', block: true, passedBy, blockedBy, needsHuman: outside.map((r) => r.id), line: `review: needs-human — BLOCK on ${sha9(h)} from outside seat(s) ${outside.map(desc).join(', ')} (${klass}) — refused; ${final}` }
    return { ...none, state: 'blocked', block: true, passedBy, blockedBy, line: `review: BLOCK on ${sha9(h)} by ${blockedBy.join(', ')} (${klass}) — refused; ${final}` }
  }
  const older = [...new Set(records.filter((r) => r.kind === 'pass' && r.head !== h).map((r) => sha9(r.head)))]
  const hist = older.length ? `; records only on older head(s) ${older.join(', ')} — a push since the review needs a re-review` : ''
  const need = security ? REVIEW_SEATS.filter((s) => !passedBy.includes(s)) : passedBy.length ? [] : ['one Opus PASS']
  if (!need.length) return { ...none, state: 'pass', block: false, passedBy, line: `review: PASS on ${sha9(h)} by ${passedBy.join(' + ')} (${klass})` }
  const missing = need.map((s) => (s === 'one Opus PASS' ? `no review-opus or review-opus2 PASS bound to ${sha9(h)}` : `no ${s} PASS bound to ${sha9(h)}`))
  return { ...none, state: 'missing', block: true, passedBy, missing, line: `review: missing on ${sha9(h)} (${klass}): ${missing.join('; ')}${passedBy.length ? ` (passed: ${passedBy.join(', ')})` : ''}${hist} — refused` }
}

export const ghCli = (args) => {
  const r = spawnSync('gh', args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 })
  if (r.status !== 0) throw new Error(String(r.stderr || r.stdout || `gh exit ${r.status}`).trim() || `gh exit ${r.status}`)
  return r.stdout
}

/** Every issue comment, page by page; a list longer than PAGE_CAP pages throws (a partial record is never read as whole). */
export function fetchComments({ gh, repo, pr }) {
  const out = []
  for (let page = 1; page <= PAGE_CAP; page++) {
    const rows = JSON.parse(gh(['api', `repos/${repo}/issues/${pr}/comments?per_page=100&page=${page}`]))
    if (!Array.isArray(rows)) throw new Error('comments are not a list')
    out.push(...rows)
    if (rows.length < 100) return out
  }
  throw new Error(`more than ${PAGE_CAP} pages of comments`)
}

/** The PR's class (title, labels, files), its head and its commits [{oid, committedDate}] (a headless BLOCK's scope). */
export function fetchClass({ gh, repo, pr }) {
  const v = JSON.parse(gh(['pr', 'view', String(pr), '-R', repo, '--json', 'labels,headRefOid,title,commits']))
  if (!Array.isArray(v.commits)) throw new Error('the PR commit list is unreadable')
  const files = String(gh(['api', `repos/${repo}/pulls/${pr}/files?per_page=100`, '--paginate', '--jq', '.[].filename'])).split('\n').map((s) => s.trim()).filter(Boolean)
  const commits = v.commits.map((c) => ({ oid: String((c && c.oid) || ''), committedDate: String((c && c.committedDate) || '') }))
  return { headRefOid: String(v.headRefOid || ''), commits, cls: securityClassOf({ files, labels: v.labels || [], title: v.title || '' }) }
}

/**
 * THE GATE the merge paths call. Reads the PR's class, current head and comments; any read error FAILS CLOSED.
 * → {block, state, line, security}
 */
export function reviewGate({ gh = ghCli, repo = DEFAULT_REPO, pr, head }) {
  const owner = String(repo || '').split('/')[0].toLowerCase()
  if (owner !== PINNED_OWNER) return { block: true, state: 'repo', security: null, line: `review: refused — repo "${String(repo).slice(0, 80)}" is not owned by ${PINNED_OWNER} (the author allowlist is pinned)` }
  if (head === true || head === '') return { block: true, state: 'head', security: null, line: 'review: refused — --head given with no value (never defaults to the current head)' }
  if (head !== undefined && !SHA_RE.test(String(head).toLowerCase())) {
    return { block: true, state: 'head', security: null, line: `review: refused — --head "${String(head).slice(0, 50)}" is not a full 40-hex sha` }
  }
  let c, comments
  try { c = fetchClass({ gh, repo, pr }); comments = fetchComments({ gh, repo, pr }) } catch (err) {
    return { block: true, state: 'error', security: null, line: `review: record unreadable (${String(err && err.message).split('\n')[0].slice(0, 140)}) — refused (fails closed)` }
  }
  const current = String(c.headRefOid || '').toLowerCase()
  const h = head === undefined ? current : String(head).toLowerCase()
  if (current !== h) return { block: true, state: 'head', security: c.cls.security, line: `review: head moved ${sha9(h)}->${sha9(current)} — refused` }
  const { records } = recordsFrom(comments, { authors: REVIEW_AUTHORS, commits: c.commits })
  const d = reviewDecision({ head: h, security: c.cls.security, records })
  return { ...d, security: c.cls.security, line: c.cls.security ? `${d.line} [security: ${c.cls.why}]` : d.line }
}

/** The canonical record body. Throws on a malformed field (the CLI turns that into exit 64). */
export function recordBody({ seat, lens, head, verdict, blocking, notes = '' }) {
  const n = String(blocking ?? '')
  if (!/^\d+$/.test(n)) throw new Error(`--blocking must be a non-negative integer, got "${n.slice(0, 20)}"`)
  if (verdict === 'BLOCK' && Number(n) === 0) throw new Error('BLOCK with blocking=0 — a BLOCK must name at least one blocking finding')
  const body = `${RECORD_MARKER}\nseat: ${seat}\nlens: ${String(lens || '').replace(/\s+/g, ' ').trim()}\nhead: ${String(head || '').toLowerCase()}\nverdict: ${verdict} blocking=${n}\n`
  if (!REVIEW_SEATS.includes(seat)) throw new Error(`seat must be one of ${REVIEW_SEATS.join(', ')}`)
  if (!String(lens || '').trim()) throw new Error('--lens is required')
  const full = notes ? `${body}\n${String(notes).replace(/^loop-review \(Opus\)$/gm, '(quoted record marker)')}\n` : body
  // The posted body must read back as exactly what was asked for: a PASS whose notes carry a "verdict: ... BLOCK"
  // line would be read as a BLOCK (liberal recognition), so it is refused here instead of posted.
  const r = parseRecord(full)
  const ok = r && (verdict === 'PASS' ? r.kind === 'pass' && r.head === String(head || '').toLowerCase()
    : r.kind === 'block' && !r.why && r.seat === seat && r.heads && r.heads.length === 1 && r.heads[0] === String(head || '').toLowerCase())
  if (!ok) throw new Error(`malformed record: it would read back as ${r ? `${r.kind}${r.why ? ` (${r.why})` : ''}` : 'no record'}${verdict === 'PASS' ? ' — keep "verdict:" lines out of PASS notes' : ''}`)
  return full
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

export function cli({ argv, gh = ghCli, env = process.env }) {
  const f = flags(argv)
  const repo = f.repo || env.LOOP_REPO || DEFAULT_REPO
  const cmd = f._[0]
  if ('head' in f && (f.head === true || f.head === '')) return { code: EXIT_BAD_FLAG, out: 'usage: --head given with no value — pass the 40-hex head sha (it never defaults to the current head)' }
  if (String(repo).split('/')[0].toLowerCase() !== PINNED_OWNER) return { code: cmd === 'gate' ? EXIT_REVIEW : EXIT_USAGE, out: `review: refused — repo "${String(repo).slice(0, 80)}" is not owned by ${PINNED_OWNER} (the author allowlist is pinned, never derived from --repo/LOOP_REPO)` }
  if (!f.pr || f.pr === true || !['gate', 'record', 'klass'].includes(cmd)) return { code: EXIT_USAGE, out: 'usage: review-gate.mjs gate|record|klass --pr N [...] (see the header)' }
  if (cmd === 'gate') { const g = reviewGate({ gh, repo, pr: f.pr, head: f.head }); return { code: g.block ? EXIT_REVIEW : 0, out: g.line } }
  if (cmd === 'klass') {
    try { const { cls } = fetchClass({ gh, repo, pr: f.pr }); return { code: 0, out: cls.security ? `security ${cls.why}` : 'standard' } } catch (err) {
      return { code: EXIT_USAGE, out: `klass unreadable: ${String(err && err.message).split('\n')[0]} — treat as security` }
    }
  }
  let body, current
  try {
    body = recordBody({ seat: f.seat, lens: f.lens, head: f.head, verdict: f.verdict, blocking: f.blocking, notes: f['notes-file'] ? fs.readFileSync(f['notes-file'], 'utf8') : '' })
    current = String(JSON.parse(gh(['pr', 'view', String(f.pr), '-R', repo, '--json', 'headRefOid'])).headRefOid || '').toLowerCase()
  } catch (err) { return { code: EXIT_USAGE, out: `record refused: ${String(err && err.message).split('\n')[0]}` } }
  if (current !== String(f.head).toLowerCase()) return { code: EXIT_USAGE, out: `record refused: ${sha9(f.head)} is not the PR's current head ${sha9(current)} — review the current head` }
  const tmp = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'review-gate-')), 'body.md')
  fs.writeFileSync(tmp, body)
  try { gh(['pr', 'comment', String(f.pr), '-R', repo, '--body-file', tmp]) } catch (err) { return { code: 1, out: `record not posted: ${String(err && err.message).split('\n')[0]}` } }
  return { code: 0, out: `review record posted: ${f.seat} ${f.verdict} blocking=${f.blocking} head=${sha9(f.head)} on #${f.pr}` }
}

// ═══ THE PROMPT CONTRACT — build.mjs validation (item step 5) ═══════════════════════════════════════════════════
// A static reader of the generated harness: every agent dispatch → {seat, text}, where text is the prompt literal
// plus the source of every top-level const it interpolates (transitively). Then three rules:
//   (1) a prompt that contains "gh pr merge" also contains "review-gate.mjs gate" and "--match-head-commit";
//   (2) ONLY reviewer-seat prompts (review-opus, review-opus2) contain "review-gate.mjs record";
//   (3) every reviewer-seat prompt does contain it (a reviewer that cannot post its record can never pass a PR).
// LIMIT: item specs are data interpolated at runtime (${item.spec}); they are not expanded here.

const IDENT = /[A-Za-z_$][\w$]*/y
const CONT = new Set(['?', ':', '.', '+', ')', ']', '}', '`', '|', '&', ',', '-'])

function skipString(src, i) {
  const q = src[i]
  for (let j = i + 1; j < src.length; j++) {
    if (src[j] === '\\') { j++; continue }
    if (src[j] === q || (src[j] === '\n' && q !== '`')) return j + 1
  }
  return src.length
}

/** src[i] === '`'. Returns the index after the closing backtick; adds every identifier used in its ${} code to ids. */
function walkTemplate(src, i, ids) {
  for (let j = i + 1; j < src.length; j++) {
    const ch = src[j]
    if (ch === '\\') { j++; continue }
    if (ch === '`') return j + 1
    if (ch === '$' && src[j + 1] === '{') j = walkCode(src, j + 2, ids, true) - 1
  }
  throw new Error(`unterminated template literal at offset ${i}`)
}

/** Walks code from i; with inBrace, stops after the matching '}'. Collects identifiers outside strings and comments. */
function walkCode(src, i, ids, inBrace, stopAtStatementEnd = false, stopAtComma = false) {
  let depth = 0
  let j = i
  while (j < src.length) {
    const ch = src[j]
    if (ch === '`') { j = walkTemplate(src, j, ids); continue }
    if (ch === "'" || ch === '"') { j = skipString(src, j); continue }
    if (ch === '/' && src[j + 1] === '/') { const e = src.indexOf('\n', j); j = e < 0 ? src.length : e; continue }
    if (ch === '/' && src[j + 1] === '*') { const e = src.indexOf('*/', j + 2); j = e < 0 ? src.length : e + 2; continue }
    if (ch === ',' && stopAtComma && depth === 0) return j
    if (ch === '{' || ch === '(' || ch === '[') depth++
    else if (ch === '}' || ch === ')' || ch === ']') {
      if (depth === 0 && inBrace && ch === '}') return j + 1
      if (depth === 0 && stopAtComma) return j
      depth--
    } else if (ch === '\n' && stopAtStatementEnd && depth <= 0) {
      const next = src[j + 1]
      if (next !== undefined && !/\s/.test(next) && !CONT.has(next)) return j
    } else if (/[A-Za-z_$]/.test(ch) && !/[\w$]/.test(src[j - 1] || '')) {
      IDENT.lastIndex = j
      const m = IDENT.exec(src)
      ids.add(m[0])
      j += m[0].length
      continue
    }
    j++
  }
  return src.length
}

/** Top-level `const|let NAME = ...` → {name, text, ids}. A body runs to the end of its statement. */
export function topLevelDecls(src) {
  const out = new Map()
  for (const m of src.matchAll(/^(?:export\s+)?(?:const|let)\s+([A-Za-z_$][\w$]*)\s*=/gm)) {
    const start = m.index + m[0].length
    if (/^\s*await\b/.test(src.slice(start, start + 40))) continue // an agent dispatch's RESULT, not prompt text
    const ids = new Set()
    let end
    try { end = walkCode(src, start, ids, false, true) } catch { end = src.indexOf('\n', start) }
    out.set(m[1], { name: m[1], text: src.slice(m.index, end), ids })
  }
  return out
}

const label0 = (src, after) => { const opt = src.slice(after, after + 600).match(/label:\s*(['`])([^'`]*)/); return opt ? opt[2] : '' }

/** Every agent dispatch → {seat, label, text, line}. text = the prompt literal + every const it reaches. */
export function promptUnits(src) {
  const decls = topLevelDecls(src)
  const units = []
  for (const m of src.matchAll(/(?<![\w$.])agentR?\(\s*/g)) {
    if (/function\s*$/.test(src.slice(Math.max(0, m.index - 20), m.index))) continue
    const at = m.index + m[0].length
    const line = src.slice(0, m.index).split('\n').length
    if (/^prompt\b/.test(src.slice(at, at + 8))) continue // the agentR/agentRRetry wrappers forwarding their own argument
    // The first argument: a literal, an identifier, a call or a ternary — every identifier it uses is followed.
    const ids = new Set()
    let after
    try { after = walkCode(src, at, ids, false, false, true) } catch (err) { units.push({ seat: 'unresolved', label: String(err.message), text: '', line }); continue }
    const parts0 = [src.slice(at, after)]
    if (!label0(src, after) && !parts0[0].includes('`') && !/^[A-Za-z_$]/.test(parts0[0])) continue // prose, not a call
    const innerSeen = new Set()
    for (const name of ids) { // a Set iterates what is added while iterating: inner prompts reach inner prompts
      if (decls.has(name) || innerSeen.has(name)) continue
      innerSeen.add(name)
      // an inner prompt: `const NAME = \`...\``, `const NAME = (args) => \`...\`` or `function NAME(args) { ... return \`...\``
      const esc = name.replace(/\$/g, '\\$')
      const d = new RegExp(`^[ \\t]+(?:(?:const|let)\\s+${esc}\\s*=\\s*(?:\\([^)]*\\)\\s*=>\\s*)?|function\\s+${esc}\\s*\\([^)]*\\)\\s*\\{[\\s\\S]*?return\\s+)\``, 'm').exec(src)
      if (!d) continue
      const tick = d.index + d[0].length - 1
      parts0.push(src.slice(tick, walkTemplate(src, tick, ids)))
    }
    const literal = parts0.join('\n')
    const label = label0(src, after)
    const seat = label.split(/:|\$\{/)[0] || 'unknown'
    const seen = new Set()
    const parts = [literal]
    const queue = [...ids]
    while (queue.length) {
      const name = queue.shift()
      if (seen.has(name) || !decls.has(name)) continue
      seen.add(name)
      const d = decls.get(name)
      parts.push(d.text)
      queue.push(...d.ids)
    }
    units.push({ seat, label, text: parts.join('\n'), line })
  }
  return units
}

export const MERGE_NEEDLE = 'gh pr merge'
export const GATE_NEEDLE = 'review-gate.mjs gate'
export const RECORD_NEEDLE = 'review-gate.mjs record'
export const MATCH_HEAD_NEEDLE = '--match-head-commit'

/** units: [{seat, label, text, line}] → [problem strings]. Empty means the prompt contract holds. */
export function promptGateProblems(units) {
  const problems = []
  for (const u of units || []) {
    const where = `${u.label || u.seat} prompt (line ${u.line ?? '?'})`
    if (u.seat === 'unresolved') { problems.push(`${where}: the dispatched prompt could not be resolved to a literal — the review-gate contract cannot be checked`); continue }
    const t = String(u.text || '')
    if (t.includes(MERGE_NEEDLE) && !t.includes(GATE_NEEDLE)) problems.push(`${where} contains "${MERGE_NEEDLE}" but not "${GATE_NEEDLE}" — every merge path runs the review gate immediately before it merges`)
    if (t.includes(MERGE_NEEDLE) && !t.includes(MATCH_HEAD_NEEDLE)) problems.push(`${where} contains "${MERGE_NEEDLE}" but not "${MATCH_HEAD_NEEDLE}" — the merge must be bound to the sha the review gate read`)
    const reviewer = REVIEW_SEATS.includes(u.seat)
    if (t.includes(RECORD_NEEDLE) && !reviewer) problems.push(`${where} (seat ${u.seat}) contains "${RECORD_NEEDLE}" — ONLY reviewer seats (${REVIEW_SEATS.join(', ')}) are handed the record subcommand; a fixer, lander, sweeper or drain that can post a PASS makes the gate a self-attestation`)
    if (reviewer && !t.includes(RECORD_NEEDLE)) problems.push(`${where} (seat ${u.seat}) does not contain "${RECORD_NEEDLE}" — a reviewer that cannot post its record can never pass a PR`)
  }
  return problems
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const r = cli({ argv: process.argv.slice(2) })
  console.log(r.out)
  process.exitCode = r.code
}
