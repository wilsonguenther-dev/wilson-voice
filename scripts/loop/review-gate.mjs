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
 * Free-text notes follow the first blank line and are never read as fields.
 * Not a record, ever: an author outside REVIEW_AUTHORS (the account the seats post as); a bot author.
 * A BLOCK FAILS CLOSED (review-opus-h1 B1/B2 on PR #222, the same rule as Learn's H-GATE-1). From an allowed
 * author, ANY comment whose header carries a `verdict: BLOCK...` line is a blocking record:
 *   - under ANY seat name (review-opus-h1, a fixer, no seat line at all): seat names only matter for PASSes;
 *   - EDITED (updated_at != created_at): still blocking, until a fresh unedited record from the same seat on the
 *     same head supersedes it;
 *   - malformed: a missing or unparsable `blocking=` (or blocking=0) is blocking=1; a duplicate field is ignored in
 *     favour of blocking; a 7..39-hex head binds to the head it prefixes; a missing or unreadable head binds to
 *     EVERY head (it blocks until the same seat posts a newer record on the current head, or a human deletes it).
 * A PASS counts only when it is well formed, unedited (an edited PASS is VOID — re-post, never rebind), from a
 * seat in REVIEW_SEATS (a fixer, drain, sweeper, land or any unlisted seat never passes a PR), with blocking=0
 * and a full 40-hex head. A PASS that does not count also never supersedes an earlier BLOCK.
 *
 * THE DECISION — reviewDecision(), pure, against the PR's CURRENT head only (a push makes a new head, so ANY push —
 * a rebase, a conflict resolution, a fix — invalidates every record and needs a fresh PASS):
 *   SECURITY class (securityClassOf: an item id in SECURITY_IDS, a security label, a file under SECURITY_PATHS, or
 *   a file list longer than one page): a PASS from review-opus AND a PASS from review-opus2 on the head — two
 *   DIFFERENT seats; two PASSes from one seat are one review.
 *   STANDARD: one PASS from either seat on the head.
 *   NO record on the head refuses. There is no unreviewed exception in Yap (Learn's "build-mode LAND" pass-through
 *   is deliberately NOT ported).
 *   Any seat whose newest record ON THE HEAD is BLOCK refuses. Records on older heads are history: never PASS,
 *   never BLOCK (an unbound BLOCK — no readable head — is the one exception: it applies to every head).
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
 * SQLite history and its migrations, the polish_protocol / sidecar IPC, any path matching the trust keywords, and
 * the loop harness itself (scripts/loop/, this file included) and CI.
 */
export const SECURITY_PATHS = [
  /^desktop\/src-tauri\/capabilities\//, /^desktop\/src-tauri\/tauri(?:\.[\w-]+)?\.conf\.json$/, /^desktop\/src-tauri\/src\/db/,
  /migration/i, /polish_protocol|sidecar/i,
  /updater|sign|notariz|keychain|permission|accessibility|microphone|network|http|upload|telemetry/i,
  /^scripts\/loop\//, /^scripts\/cicd-loop-all\.mjs$/, /^\.github\/workflows\//,
]
/** template.mjs SECURITY_IDS ('SEC-|UPD-|PRIV-|DB-|PERM-'): an item id at the start of the PR title. */
export const SECURITY_ID_RE = /^\s*(?:SEC-|UPD-|PRIV-|DB-|PERM-)/i
const SHA_RE = /^[0-9a-f]{40}$/
const SHA_PREFIX_RE = /^[0-9a-f]{7,40}$/
const BLOCK_RE = /^BLOCK/i
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

/**
 * One comment body → {seat, lens, head, verdict, blocking, defect?} | {malformed: why} | null (not a record at all).
 * Any header verdict line starting BLOCK makes it a BLOCK record that is never `malformed` (fails closed): its
 * defects are listed in `defect`, blocking is at least 1, and head is a full sha, a 7..39-hex prefix, or null
 * (unbound — applies to every head).
 */
export function parseRecord(body) {
  const lines = String(body || '').split('\n').map((l) => l.trim())
  const first = lines.find(Boolean)
  if (first !== RECORD_MARKER) return null
  // Fields are read from the HEADER only: the marker line through the first blank line after it. Free-text notes
  // follow that blank line (recordBody writes them there) and may quote "verdict:" or "lens:" lines.
  const start = lines.indexOf(RECORD_MARKER) + 1
  const blank = lines.indexOf('', start)
  const header = lines.slice(start, blank === -1 ? lines.length : blank)
  const field = (k) => header.filter((l) => l.toLowerCase().startsWith(`${k}:`)).map((l) => l.slice(k.length + 1).trim())
  const verdicts = field('verdict')
  if (verdicts.some((v) => BLOCK_RE.test(v))) return blockRecord(field, verdicts)
  const one = {}
  for (const k of ['seat', 'lens', 'head', 'verdict']) {
    const v = field(k)
    if (v.length !== 1) return { malformed: `${v.length ? 'more than one' : 'no'} ${k}: line` }
    one[k] = v[0]
  }
  const head = one.head.toLowerCase()
  if (!SHA_RE.test(head)) return { malformed: `head "${one.head.slice(0, 50)}" is not a full 40-hex sha` }
  const m = one.verdict.match(/^(PASS|BLOCK)\s+blocking=(\d+)$/)
  if (!m) return { malformed: `verdict "${one.verdict.slice(0, 50)}" is not "PASS blocking=0" / "BLOCK blocking=<n>"` }
  const blocking = Number(m[2])
  if (m[1] === 'PASS' && blocking !== 0) return { malformed: `PASS with blocking=${blocking}` }
  return { seat: one.seat, lens: one.lens, head, verdict: m[1], blocking }
}

/** A header with a BLOCK verdict line → a BLOCK record, whatever else is wrong with it (B2: malformed = blocking). */
function blockRecord(field, verdicts) {
  const defects = []
  const seats = field('seat')
  const seat = seats.length === 1 && seats[0] ? seats[0] : UNNAMED_SEAT
  if (seat === UNNAMED_SEAT) defects.push(`${seats.length} seat: lines`)
  const heads = field('head').map((h) => h.toLowerCase())
  let head = null
  if (heads.length === 1 && SHA_PREFIX_RE.test(heads[0])) {
    head = heads[0]
    if (head.length < 40) defects.push(`short head ${head}`)
  } else defects.push(`${heads.length === 1 ? `head "${heads[0].slice(0, 50)}"` : `${heads.length} head: lines`} unreadable — binds to every head`)
  if (verdicts.length !== 1) defects.push(`${verdicts.length} verdict: lines`)
  const m = verdicts.length === 1 ? verdicts[0].match(/^BLOCK\s+blocking=(\d+)$/) : null
  let blocking = m ? Number(m[1]) : 1
  if (!m) defects.push('blocking= missing or unparsable — treated as blocking=1')
  else if (blocking < 1) { blocking = 1; defects.push('BLOCK with blocking=0 — treated as blocking=1') }
  const lens = field('lens')
  return { seat, lens: lens.length === 1 ? lens[0] : '', head, verdict: 'BLOCK', blocking, ...(defects.length ? { defect: defects.join('; ') } : {}) }
}

/** Issue comments [{user, body, created_at, updated_at, id}] → {records: [...], ignored: [why]} (oldest first). */
export function recordsFrom(comments, { authors = REVIEW_AUTHORS } = {}) {
  const records = []
  const ignored = []
  const allowed = new Set((authors || []).map((a) => String(a || '').trim().toLowerCase()).filter(Boolean))
  for (const c of comments || []) {
    const r = parseRecord(c && c.body)
    if (r === null) continue
    if (isBot(c.user)) { ignored.push(`comment ${c.id}: bot author`); continue }
    if (!allowed.has(String((c.user && c.user.login) || '').toLowerCase())) { ignored.push(`comment ${c.id}: author ${(c.user && c.user.login) || '(none)'} is not a review seat account`); continue }
    const edited = Boolean(c.updated_at && c.created_at && c.updated_at !== c.created_at)
    // B1 + B2: a BLOCK from an allowed author is blocking under any seat name, edited or malformed (fails closed).
    if (r.verdict === 'BLOCK') { records.push({ ...r, edited, at: String(c.created_at || ''), id: c.id }); continue }
    if (r.malformed) { ignored.push(`comment ${c.id}: malformed (${r.malformed})`); continue }
    if (edited) { ignored.push(`comment ${c.id}: edited PASS is void (re-post, never rebind)`); continue }
    if (FIXER_SEAT_RE.test(r.seat)) { ignored.push(`comment ${c.id}: fixer seat ${r.seat} never passes a PR`); continue }
    if (!REVIEW_SEATS.includes(r.seat)) { ignored.push(`comment ${c.id}: seat ${r.seat} is not a reviewer seat — its PASS does not count`); continue }
    records.push({ ...r, edited: false, at: String(c.created_at || ''), id: c.id })
  }
  records.sort((a, b) => a.at.localeCompare(b.at))
  return { records, ignored }
}

/**
 * THE DECISION. Pure. head: the PR's current head. security: securityClassOf(...).security. records: recordsFrom().records.
 * → {state: 'pass'|'missing'|'blocked', block, passedBy, missing: [why], blockedBy: [seat], line}
 * No record on the head refuses: Yap has no unreviewed exception.
 */
export function reviewDecision({ head, security = false, records = [] }) {
  const h = String(head || '').toLowerCase()
  const klass = security ? 'security' : 'standard'
  if (!SHA_RE.test(h)) return { state: 'missing', block: true, passedBy: [], missing: ['no full head sha to bind a review to'], blockedBy: [], line: `review: refused — no full head sha (${klass})` }
  // A record applies to h when its head is h, a short prefix of h (a malformed BLOCK), or null (an unbound BLOCK).
  const applies = (r) => r.head === null || r.head === h || (r.head.length < 40 && h.startsWith(r.head))
  const newest = new Map()
  for (const r of records) if (applies(r)) newest.set(r.seat, r) // records are oldest first: the last one per seat wins
  const why = (r) => [r.edited ? 'edited — fails closed' : '', r.defect || ''].filter(Boolean).join('; ')
  const blockedBy = [...newest.values()].filter((r) => r.verdict === 'BLOCK').map((r) => (why(r) ? `${r.seat} (${why(r)})` : r.seat))
  const passedBy = [...newest.values()].filter((r) => r.verdict === 'PASS' && REVIEW_SEATS.includes(r.seat) && !r.edited).map((r) => r.seat)
  const older = [...new Set(records.filter((r) => !applies(r)).map((r) => sha9(r.head)))]
  const hist = older.length ? `; records only on older head(s) ${older.join(', ')} — a push since the review needs a re-review` : ''
  if (blockedBy.length) return { state: 'blocked', block: true, passedBy, missing: [], blockedBy, line: `review: BLOCK on ${sha9(h)} by ${blockedBy.join(', ')} (${klass}) — refused` }
  const need = security ? REVIEW_SEATS.filter((s) => !passedBy.includes(s)) : passedBy.length ? [] : ['one Opus PASS']
  if (!need.length) return { state: 'pass', block: false, passedBy, missing: [], blockedBy: [], line: `review: PASS on ${sha9(h)} by ${passedBy.join(' + ')} (${klass})` }
  const missing = need.map((s) => (s === 'one Opus PASS' ? `no review-opus or review-opus2 PASS bound to ${sha9(h)}` : `no ${s} PASS bound to ${sha9(h)}`))
  return { state: 'missing', block: true, passedBy, missing, blockedBy: [], line: `review: missing on ${sha9(h)} (${klass}): ${missing.join('; ')}${passedBy.length ? ` (passed: ${passedBy.join(', ')})` : ''}${hist} — refused` }
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

/** The PR's class: its title, labels and files. */
export function fetchClass({ gh, repo, pr }) {
  const v = JSON.parse(gh(['pr', 'view', String(pr), '-R', repo, '--json', 'labels,headRefOid,title']))
  const files = String(gh(['api', `repos/${repo}/pulls/${pr}/files?per_page=100`, '--paginate', '--jq', '.[].filename'])).split('\n').map((s) => s.trim()).filter(Boolean)
  return { headRefOid: String(v.headRefOid || ''), cls: securityClassOf({ files, labels: v.labels || [], title: v.title || '' }) }
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
  const { records } = recordsFrom(comments, { authors: REVIEW_AUTHORS })
  const d = reviewDecision({ head: h, security: c.cls.security, records })
  return { ...d, security: c.cls.security, line: c.cls.security ? `${d.line} [security: ${c.cls.why}]` : d.line }
}

/** The canonical record body. Throws on a malformed field (the CLI turns that into exit 64). */
export function recordBody({ seat, lens, head, verdict, blocking, notes = '' }) {
  const n = String(blocking ?? '')
  if (!/^\d+$/.test(n)) throw new Error(`--blocking must be a non-negative integer, got "${n.slice(0, 20)}"`)
  if (verdict === 'BLOCK' && Number(n) === 0) throw new Error('BLOCK with blocking=0 — a BLOCK must name at least one blocking finding')
  const body = `${RECORD_MARKER}\nseat: ${seat}\nlens: ${String(lens || '').replace(/\s+/g, ' ').trim()}\nhead: ${String(head || '').toLowerCase()}\nverdict: ${verdict} blocking=${n}\n`
  const r = parseRecord(body)
  if (!r || r.malformed || r.defect) throw new Error(`malformed record: ${r ? r.malformed || r.defect : 'no marker'}`)
  if (!REVIEW_SEATS.includes(r.seat)) throw new Error(`seat must be one of ${REVIEW_SEATS.join(', ')}`)
  if (!r.lens) throw new Error('--lens is required')
  return notes ? `${body}\n${String(notes).replace(/^loop-review \(Opus\)$/gm, '(quoted record marker)')}\n` : body
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
