#!/usr/bin/env node
// Y7-C — the engine behind scripts/assert-tests-are-non-vacuous.sh.
//
// docs/loop/MUTATIONS.md is a table with ONE ROW PER TEST FILE in
// desktop/src-tauri/tests/. Each row names a SINGLE-LINE mutation of SOURCE
// (never of the test) and the ONE test function that must go red under it:
//
//   | # | test file | named test | source file | anchor | from | to | executed |
//
//   source file  path relative to desktop/src-tauri (src/…, Info.plist, …)
//   anchor       a substring that must match EXACTLY ONE line of that file
//   from → to    replaced once, inside that line only ("(empty)" = delete)
//   executed     written by `--execute`, never by hand: the observed
//                transition, plus the sha256 of the test file it was observed
//                against. A row whose test file changed since is STALE.
//
// Subcommands (the .sh is the only intended caller):
//   rows [file…]        one \x1f-separated line per row for the shell loop
//   apply <n> <backup>  copy the source to <backup>, apply row n's mutation
//   record <tsv>        write observed transitions back into the ledger
//   verify              shape check, no cargo: every test file has a row, every
//                       mutation applies to the CURRENT tree, every row carries
//                       an `executed: red` record bound to its test's bytes
//   scopeless           scan scripts/loop/items/*.mjs for grep invocations with
//                       no path scope (a scopeless grep in a gate is a false green)
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const SRC_TAURI = path.join(REPO, 'desktop', 'src-tauri');
const TESTS = path.join(SRC_TAURI, 'tests');
const LEDGER = path.join(REPO, 'docs', 'loop', 'MUTATIONS.md');
const ITEMS = path.join(REPO, 'scripts', 'loop', 'items');
const START = '<!-- ledger:start -->';
const END = '<!-- ledger:end -->';
const EMPTY = '(empty)';
const SEP = '\x1f';

const die = (msg, code = 2) => { process.stderr.write(`non-vacuous: ${msg}\n`); process.exit(code); };
const sha12 = (file) => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex').slice(0, 12);

function splitRow(line) {
  // Split on pipes that are not escaped; a code span may carry `\|`.
  const cells = [];
  let cur = '';
  for (let i = 0; i < line.length; i++) {
    if (line[i] === '\\' && line[i + 1] === '|') { cur += '|'; i++; continue; }
    if (line[i] === '|') { cells.push(cur); cur = ''; continue; }
    cur += line[i];
  }
  cells.push(cur);
  return cells.slice(1, -1).map((c) => c.trim());
}
const unCode = (c) => (c.startsWith('`') && c.endsWith('`') && c.length >= 2 ? c.slice(1, -1) : c);
export const cell = (s) => '`' + String(s).replace(/\|/g, '\\|') + '`';

function readLedger() {
  if (!fs.existsSync(LEDGER)) die(`${path.relative(REPO, LEDGER)} is missing`);
  const text = fs.readFileSync(LEDGER, 'utf8');
  const lines = text.split('\n');
  const a = lines.indexOf(START);
  const b = lines.indexOf(END);
  if (a < 0 || b < a) die(`ledger markers ${START} … ${END} are missing`);
  const rows = [];
  for (let i = a + 1; i < b; i++) {
    const l = lines[i];
    if (!l.startsWith('| ')) continue;
    const c = splitRow(l);
    if (c.length !== 8 || c[0] === '#' || /^-+$/.test(c[0])) continue;
    rows.push({
      line: i, n: Number(c[0]), testFile: unCode(c[1]), test: unCode(c[2]), source: unCode(c[3]),
      anchor: unCode(c[4]), from: unCode(c[5]), to: unCode(c[6]) === EMPTY ? '' : unCode(c[6]), executed: c[7],
    });
  }
  return { text, lines, rows };
}

function locate(row) {
  if (!row.source) return { err: 'no source file named' };
  const file = path.join(SRC_TAURI, row.source);
  if (!fs.existsSync(file) || !fs.statSync(file).isFile()) return { err: `source ${row.source} does not exist` };
  if (row.source.startsWith('tests/')) return { err: 'the mutation targets a TEST file; mutate the source' };
  const lines = fs.readFileSync(file, 'utf8').split('\n');
  const hits = [];
  lines.forEach((l, i) => { if (l.includes(row.anchor)) hits.push(i); });
  if (hits.length !== 1) return { err: `anchor matches ${hits.length} lines of ${row.source} (must be exactly 1)` };
  const i = hits[0];
  if (!lines[i].includes(row.from)) return { err: `line ${i + 1} of ${row.source} does not contain the "from" text` };
  const mutated = lines[i].replace(row.from, row.to);
  if (mutated === lines[i]) return { err: 'the mutation is a no-op' };
  if (mutated.includes('\n')) return { err: 'the mutation is not single-line' };
  return { file, lines, i, mutated };
}

function nonIgnoredTests(testFile) {
  const L = fs.readFileSync(path.join(TESTS, testFile), 'utf8').split('\n');
  const out = new Map();
  let pending = false, ignored = false;
  for (const l of L) {
    if (/^\s*#\[(tokio::)?test/.test(l)) { pending = true; ignored = false; }
    if (pending && /#\[ignore/.test(l)) ignored = true;
    const m = l.match(/\bfn\s+(\w+)\s*\(/);
    if (pending && m) { out.set(m[1], !ignored); pending = false; }
  }
  return out;
}

function verify() {
  const { rows } = readLedger();
  const files = fs.readdirSync(TESTS).filter((f) => f.endsWith('.rs')).sort();
  const byFile = new Map();
  const problems = [];
  for (const r of rows) {
    if (byFile.has(r.testFile)) problems.push(`row ${r.n}: ${r.testFile} has more than one row`);
    byFile.set(r.testFile, r);
  }
  for (const f of files) if (!byFile.has(f)) problems.push(`${f}: NEW TEST FILE WITH NO ROW — add its mutation and run --execute --only ${f}`);
  let red = 0, exempt = 0;
  const exemptions = [];
  for (const r of rows) {
    const where = `row ${r.n} (${r.testFile} :: ${r.test})`;
    const testPath = path.join(TESTS, r.testFile);
    if (!fs.existsSync(testPath)) { problems.push(`${where}: the test file is gone — delete the row`); continue; }
    const tests = nonIgnoredTests(r.testFile);
    if (r.executed.startsWith('exempt: ignored-only')) {
      if ([...tests.values()].some(Boolean)) problems.push(`${where}: exempt as ignored-only, but the file has a test that runs`);
      else { exempt++; exemptions.push(`${r.testFile}: ${r.executed}`); }
      continue;
    }
    if (r.executed.startsWith('exempt:')) {
      // Any other exemption must say WHY in a sentence, and is printed on every run.
      if (r.executed.length < 60) problems.push(`${where}: an exemption needs a stated reason`);
      else { exempt++; exemptions.push(`${r.testFile}: ${r.executed}`); }
      continue;
    }
    if (!tests.has(r.test)) { problems.push(`${where}: no #[test] fn named ${r.test}`); continue; }
    if (!tests.get(r.test)) problems.push(`${where}: ${r.test} is #[ignore]d, so no default run can go red`);
    const loc = locate(r);
    if (loc.err) { problems.push(`${where}: ${loc.err}`); continue; }
    if (!r.executed.startsWith('executed: red')) { problems.push(`${where}: never observed red (executed column: "${r.executed || 'empty'}") — a row whose mutation was never executed is a failed row`); continue; }
    const want = `test sha256 ${sha12(testPath)}`;
    if (!r.executed.includes(want)) { problems.push(`${where}: STALE — the test file changed since the red run (${want} not in the record); re-run --execute --only ${r.testFile}`); continue; }
    red++;
  }
  console.log(`ledger: ${files.length} test files · ${rows.length} rows · ${red} observed red on the current test bytes · ${exempt} exempt`);
  for (const e of exemptions) console.log(`  EXEMPT ${e}`);
  for (const p of problems) console.log(`  FAIL ${p}`);
  return problems.length === 0;
}

// ── scopeless grep census ────────────────────────────────────────────────────
// A grep is SCOPED when it reads a pipe (its scope is the producer) or names at
// least one path that is not the whole tree. `git grep` is scoped only with a
// `-- <pathspec>`. Prose ("the grep proving absence") is not a command and is
// skipped: only a grep in COMMAND POSITION is checked.
const CMD_START = /(^|[|;&(!`]|\$\(|&&|\|\||\bthen\b|\bdo\b|\bif\b)\s*$/;
const WHOLE_TREE = new Set(['.', './', '..', '/', '~', '*', '$HOME', '"$HOME"']);
const OPTS_WITH_ARG = new Set(['-e', '-f', '-m', '-A', '-B', '-C', '--max-count', '--regexp', '--file', '--context', '--include', '--exclude', '--exclude-dir']);

function shellWords(s) {
  // Returns words; a word that carried quotes is marked so "<false/>" is a
  // pattern, never mistaken for a redirect.
  const out = []; let cur = ''; let q = null; let quoted = false;
  const push = () => { out.push({ w: cur, quoted }); cur = ''; quoted = false; };
  for (let i = 0; i < s.length; i++) {
    const ch = s[i];
    if (q) { if (ch === q) q = null; else cur += ch; continue; }
    if (ch === "'" || ch === '"') { q = ch; quoted = true; continue; }
    if (ch === '`') { if (cur || quoted) push(); out.push({ w: ';', quoted: false }); continue; }
    if (/\s/.test(ch)) { if (cur || quoted) push(); continue; }
    if (';|&)'.includes(ch)) { if (cur || quoted) push(); out.push({ w: ch, quoted: false }); continue; }
    cur += ch;
  }
  if (cur || quoted) push();
  return out;
}

export function classifyGrep(line) {
  line = line.replace(/\\`/g, '`'); // a template literal's escaped backtick is a code span
  const findings = [];
  const re = /\b(git\s+grep|grep|egrep|fgrep|rg)\b/g;
  let m;
  while ((m = re.exec(line))) {
    const before = line.slice(0, m.index);
    if (!CMD_START.test(before)) continue;
    const piped = /\|\s*$/.test(before) && !/\|\|\s*$/.test(before);
    const words = shellWords(line.slice(m.index + m[0].length));
    const argv = [];
    for (const { w, quoted } of words) {
      if (!quoted && [';', '|', '&', ')'].includes(w)) break;
      if (!quoted && /^\d*[<>]/.test(w)) continue;
      argv.push(w.replace(/\\\\/g, '\\'));
    }
    const git = m[1].startsWith('git');
    if (piped && !git) continue;
    let pattern = null; const paths = []; let explicitE = false;
    for (let i = 0; i < argv.length; i++) {
      const a = argv[i];
      if (a === '--') { paths.push(...argv.slice(i + 1 + (pattern === null && !explicitE ? 1 : 0))); if (pattern === null && !explicitE) pattern = argv[i + 1]; break; }
      if (a.startsWith('-') && a.length > 1) { if (a === '-e' || a === '--regexp') explicitE = true; if (OPTS_WITH_ARG.has(a)) i++; continue; }
      if (pattern === null && !explicitE) { pattern = a; continue; }
      paths.push(a);
    }
    if (pattern === null && !explicitE) continue; // a bare mention of the word, not an invocation
    if (paths.length === 0) findings.push({ kind: git ? 'git grep with no pathspec (searches the whole repo)' : 'grep with no path (reads stdin: empty in a gate)', text: line.trim() });
    else if (paths.every((p) => WHOLE_TREE.has(p))) findings.push({ kind: `grep scoped to the whole tree (${paths.join(' ')})`, text: line.trim() });
  }
  return findings;
}

function scopeless() {
  if (!fs.existsSync(ITEMS)) { console.log('scopeless: no scripts/loop/items — nothing to scan'); return true; }
  const files = fs.readdirSync(ITEMS).filter((f) => f.endsWith('.mjs')).sort();
  let checked = 0; const bad = [];
  for (const f of files) {
    const raw = fs.readFileSync(path.join(ITEMS, f), 'utf8').split('\n');
    // ONE invocation may span lines: a trailing backslash, a quote or code span
    // still open at the end of the line, or a next line that starts with `-- `.
    const odd = (t, ch) => ((t.match(new RegExp('\\' + ch, 'g')) || []).length - (t.match(new RegExp('\\\\\\' + ch, 'g')) || []).length) % 2 === 1;
    const L = raw.map((l, i) => {
      let j = i, out = l;
      const at = out.search(/\b(grep|rg)\b/);
      if (at < 0) return out;
      for (let k = 0; k < 4 && j + 1 < raw.length; k++) {
        const tail = out.slice(Math.max(0, at - 6));
        const cont = /\\\s*$/.test(out) || odd(tail.replace(/\\`/g, '`'), '`') || odd(tail, '"') || /^\s*--\s/.test(raw[j + 1]);
        if (!cont) break;
        out = out.replace(/\\+\s*$/, ' ') + ' ' + raw[++j].trim();
      }
      return out;
    });
    L.forEach((l, i) => {
      if (!/\b(grep|rg)\b/.test(l)) return;
      const hits = classifyGrep(l);
      checked++;
      for (const h of hits) bad.push(`${f}:${i + 1}: ${h.kind}: ${h.text.slice(0, 160)}`);
    });
  }
  console.log(`scopeless: ${files.length} item files · ${checked} lines mention grep · ${bad.length} scopeless grep invocation(s)`);
  for (const b of bad) console.log(`  FAIL ${b}`);
  return bad.length === 0;
}

const [cmd, ...args] = process.argv.slice(2);
if (cmd === 'rows') {
  const { rows } = readLedger();
  const only = new Set(args);
  for (const r of rows) {
    if (only.size && !only.has(r.testFile) && !only.has(String(r.n))) continue;
    if (r.executed.startsWith('exempt:')) continue;
    console.log([r.n, r.testFile.replace(/\.rs$/, ''), r.test, r.source].join(SEP));
  }
} else if (cmd === 'apply') {
  const [n, backup] = args;
  const r = readLedger().rows.find((x) => String(x.n) === n) || die(`no row ${n}`);
  const loc = locate(r);
  if (loc.err) die(`row ${n}: ${loc.err}`);
  fs.copyFileSync(loc.file, backup);
  loc.lines[loc.i] = loc.mutated;
  fs.writeFileSync(loc.file, loc.lines.join('\n'));
  console.log(`mutated ${r.source}:${loc.i + 1}`);
} else if (cmd === 'record') {
  const results = new Map(fs.readFileSync(args[0], 'utf8').trim().split('\n').filter(Boolean).map((l) => { const [n, v] = l.split('\t'); return [n, v]; }));
  const { lines, rows } = readLedger();
  for (const r of rows) {
    if (!results.has(String(r.n))) continue;
    const c = lines[r.line].split(/(?<!\\)\|/);
    c[c.length - 2] = ` ${results.get(String(r.n)).replace(/\|/g, '\\|')} `;
    lines[r.line] = c.join('|');
  }
  fs.writeFileSync(LEDGER, lines.join('\n'));
} else if (cmd === 'sha') {
  console.log(sha12(path.join(TESTS, args[0])));
} else if (cmd === 'verify') {
  process.exit(verify() ? 0 : 1);
} else if (cmd === 'scopeless') {
  process.exit(scopeless() ? 0 : 1);
} else if (cmd === 'classify') {
  console.log(JSON.stringify(classifyGrep(args.join(' '))));
} else {
  die('usage: non-vacuous.mjs rows|apply|record|sha|verify|scopeless|classify', 64);
}
