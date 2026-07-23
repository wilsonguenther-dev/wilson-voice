export const meta = {
  name: 'yap-cicd-loop',
  description: 'Autonomous CI/CD loop for the Yap app: branch → build → local-verify → PR → CI → adversarial review → fix → squash-merge on green+PASS. Leaves failing items as open PRs. High effort.',
  phases: [
    { title: 'Build', detail: 'implement one item + local verify + open PR' },
    { title: 'CI+Review', detail: 'watch CI + adversarial QA vs acceptance' },
    { title: 'Fix', detail: 'address CI/review issues, re-push' },
    { title: 'Merge', detail: 'squash-merge on green + PASS' },
  ],
}

const REPO = '/Users/wilsonguenther/Desktop/wilson-voice';
const CHANGELOG = 'docs/CHANGELOG-YAP.md';
const MAX_FIX_ROUNDS = 2;
const CI_TIMEOUT = '32m';   // macOS Tauri build + cargo test is slow

// Backlog — each item is tightly scoped with TESTABLE acceptance. Big items point
// at the in-repo authority (the prototype / roadmap) so the agent reads truth, not a guess.
const BATCHES = {
  yap: [
    { id: 'YV1', title: 'Port the polished pill into the live app (world-fill camera + props)',
      ref: 'docs/prototypes/yappy-pill.html',
      spec: 'Port the design in docs/prototypes/yappy-pill.html into desktop/src/pill/YappyPill.tsx (keep it driven by the REAL app events: get_status/status/recording/audio_level/transcript, MouthDriver from ./mouth, reactiveLine from ./tone). Bring over: (1) the PULL-BACK CAMERA — a drawImage source-rect zoom so at rest the capsule is zoomed on the face and on activity it pans back so the pixel WORLD (sky+grass) fills the ENTIRE capsule edge-to-edge (no black rectangle-in-a-box); (2) a SKY-BLUE capsule fill (not obsidian); (3) length-tier PROPS/personas keyed off the transcript wordCount: paragraph → notepad + pencil in the wing; a few paragraphs → a desk + a pixel typewriter + glasses (receptionist); essay → glasses + full filing; (4) tone-aware working chatter + final line (rude/friendly/rose) from a data table. Keep the rest capsule small (tiny face). Do NOT touch ClassicPill, the analytics, or unrelated files. Match surrounding TS style; keep tsc strict-clean.',
      accept: 'desktop/src/pill/YappyPill.tsx contains a pull-back camera via drawImage with a source-rect (grep -n "drawImage(os," must show 8+ numeric args), a sky-blue capsule (grep -in "c6ecff\\|8fd6ff\\|sky" YappyPill.tsx non-empty), tier props (grep -Ein "typewriter|desk|pencil|notepad|receptionist" YappyPill.tsx non-empty), and tone-aware lines (grep -n "rude" YappyPill.tsx non-empty). `cd desktop && npx tsc --noEmit` exits 0 and `cd desktop/src-tauri && cargo test` passes. CI green.' },

    { id: 'YV2', title: 'Rename user-facing "Wilson Voice" → "Yap" (KEEP bundle id + data dir → TCC persists)',
      ref: 'docs/OPEN-SOURCE-ROADMAP.md',
      spec: 'Rename all USER-FACING occurrences of "Wilson Voice" to "Yap": productName in desktop/src-tauri/tauri.conf.json, CFBundleDisplayName/CFBundleName in desktop/src-tauri/Info.plist, the tray menu / window titles, and UI copy strings in desktop/src/**. CRITICAL — do NOT change the bundle identifier "com.wilsonguenther.wilson-voice" (renaming it resets macOS TCC and loses the user\'s Mic/Accessibility grants) and do NOT change the data directory name "WilsonVoice" in data_dir() (renaming it orphans the user\'s SQLite history). Only user-visible text changes. Leave code comments/paths referencing WilsonVoice/wilson-voice as-is where they are identifiers.',
      accept: 'productName is "Yap" in tauri.conf.json; grep -n "com.wilsonguenther.wilson-voice" desktop/src-tauri/tauri.conf.json STILL matches (bundle id unchanged); grep -n "WilsonVoice" desktop/src-tauri/src/lib.rs still shows the data_dir path (unchanged); no USER-FACING "Wilson Voice" string remains in desktop/src/ (grep -rn "Wilson Voice" desktop/src returns nothing). `cd desktop && npx tsc --noEmit` exits 0, `cd desktop/src-tauri && cargo build` succeeds. CI green.' },

    { id: 'YV3', title: 'Smart dictation v1 — context→mode mapping + list formatting (Wispr parity)',
      ref: 'docs/OPEN-SOURCE-ROADMAP.md',
      spec: 'Add smart, context-aware dictation groundwork in Rust (the app already resolves the frontmost app via focus::frontmost_app_name → source_app). (1) Add a pure function that maps an app name / bundle to a dictation MODE: email (Gmail/Mail/Outlook/Superhuman), document (Google Docs/Word/Pages), notes (Notes/Bear/Obsidian), code (Terminal/iTerm/VS Code/Xcode/Warp), chat (Slack/Discord/Messages), else plain. (2) Add a pure text-formatting function that detects LIST intent (dictation with enumerator cues like "first, second, next" or clearly itemized fragments) and formats it as a bullet/numbered list, otherwise preserves paragraphs. Wire nothing risky into the live paste path yet if it needs UI — just the pure functions + a place to call them. Add #[test]s proving: the app→mode mapping for ~6 apps, and that a list-like input formats to a list while prose stays prose. Keep everything else untouched.',
      accept: 'A Rust module/functions exist for app→mode and list-formatting with unit tests (grep -rn "fn .*mode\\|fn format" desktop/src-tauri/src non-empty; new #[test] fns present). `cd desktop/src-tauri && cargo test` passes including the new tests. `cd desktop && npx tsc --noEmit` exits 0. CI green.' },
  ],
};

let A = {};
try { A = typeof args === 'string' ? JSON.parse(args) : (args || {}); } catch (e) { A = {}; }
const runLabel = A.runLabel || 'yap';
const batch = Array.isArray(A.batch) && A.batch.length ? A.batch : (BATCHES[runLabel] || BATCHES.yap);

const GUARD = `HARD RULES: implement ONLY this one item — no scope creep, no unrelated refactors, no stubs/mock data, targeted minimal diffs matching surrounding style. Repo: ${REPO}. Trunk is main. Use the authenticated gh CLI (account wilsonguenther-dev). Never edit main directly. If you cannot make the LOCAL gate pass, STOP and report opened=false — do NOT open a broken PR.`;

function buildPrompt(item) {
  const branch = 'fix/' + item.id.toLowerCase();
  return `You are a ruthless senior Rust + TypeScript engineer implementing ONE item in the Yap (wilson-voice) macOS Tauri app via a professional CI/CD PR flow.

ITEM ${item.id} — ${item.title}
SPEC: ${item.spec}
ACCEPTANCE (ALL must hold, these are testable): ${item.accept}
READ FIRST: ${REPO}/${item.ref}. Also read the files you'll change + their neighbours to match real structure/style.
${GUARD}

STEPS:
1. cd ${REPO} && git checkout main && git pull --ff-only origin main && git checkout -b ${branch}
2. Implement per spec. Targeted minimal diffs.
3. LOCAL VERIFY (hard gate): cd ${REPO}/desktop && npm ci && npx tsc --noEmit  (must exit 0); then cd ${REPO}/desktop/src-tauri && cargo test  (must pass). Also run the acceptance grep checks yourself and confirm they hold. If anything fails, fix until green or STOP with opened=false + why.
4. Append a one-line entry to ${REPO}/${CHANGELOG} (create it if missing).
5. cd ${REPO} && git add -A && git commit -m "feat(yap): ${item.id} <short summary>" && git push -u origin ${branch}
6. gh pr create --base main --head ${branch} --title "feat(yap): ${item.id} ${item.title}" --body "<what changed + how each acceptance criterion is met>". Capture the PR number.
Return JSON: {implemented, filesChanged[], localVerifyPassed, opened, branch:"${branch}", prNumber, prUrl, sha, notes}.`;
}

function reviewPrompt(item, build) {
  return `Independent adversarial reviewer + CI watcher for a Yap PR. Try HARD to FALSIFY that it meets acceptance. Do NOT spawn sub-agents.
ITEM ${item.id}. ACCEPTANCE: ${item.accept}. PR #${build && build.prNumber} (branch ${build && build.branch}) in ${REPO}.
1. cd ${REPO}. Wait for CI: gh pr checks ${build && build.prNumber} --watch --interval 25 (timeout ~${CI_TIMEOUT}). Record pass/fail per check.
2. gh pr diff ${build && build.prNumber}; open the changed files. RUN each acceptance grep/command yourself and confirm the exact expected output.
3. Falsify: is every criterion genuinely met (reason about a concrete failing input)? Any stubs/mock data? Scope creep (files unrelated to ${item.id})? For YV2 specifically: confirm the bundle id and data_dir were NOT changed.
Return JSON: {ciGreen, failingChecks[], verdict:"PASS"|"FAIL", scopeCreep, issues:[{severity,what,where,fix}]}. PASS only if acceptance is genuinely met AND CI is green. Never claim green if CI failed or never ran.`;
}

function fixPrompt(item, build, review) {
  const issues = JSON.stringify((review && review.issues) || [], null, 1);
  const ci = review && !review.ciGreen ? `CI is RED (${JSON.stringify(review.failingChecks || [])}) — fix the build/tests too.` : 'CI is green; address the review issues.';
  return `Fix PR #${build && build.prNumber} (branch ${build && build.branch}) for ${item.id} in ${REPO}. ${ci}
Fix ONLY these: ${issues}
${GUARD}
cd ${REPO} && git checkout ${build && build.branch} && git pull --ff-only. Implement → LOCAL VERIFY (cd desktop && npx tsc --noEmit; cd desktop/src-tauri && cargo test; re-run the acceptance greps) → append changelog → commit "fix(yap): ${item.id} address CI/review" → git push. Return the build JSON shape (same prNumber/branch).`;
}

function mergePrompt(item, build) {
  return `Merge the approved + green PR #${build && build.prNumber} (branch ${build && build.branch}) in ${REPO}. Final guard: run gh pr checks ${build && build.prNumber} and confirm ALL required checks pass. Then cd ${REPO} && gh pr merge ${build && build.prNumber} --squash --delete-branch && git checkout main && git pull --ff-only origin main. Return {merged, mergeSha, notes}.`;
}

const BUILD_SCHEMA = { type: 'object', additionalProperties: false, required: ['implemented', 'localVerifyPassed', 'opened', 'notes'], properties: { implemented: { type: 'boolean' }, filesChanged: { type: 'array', items: { type: 'string' } }, localVerifyPassed: { type: 'boolean' }, opened: { type: 'boolean' }, branch: { type: 'string' }, prNumber: { type: 'number' }, prUrl: { type: 'string' }, sha: { type: 'string' }, notes: { type: 'string' } } };
const REVIEW_SCHEMA = { type: 'object', additionalProperties: false, required: ['ciGreen', 'verdict', 'scopeCreep', 'issues'], properties: { ciGreen: { type: 'boolean' }, failingChecks: { type: 'array', items: { type: 'string' } }, verdict: { type: 'string', enum: ['PASS', 'FAIL'] }, scopeCreep: { type: 'boolean' }, issues: { type: 'array', items: { type: 'object', additionalProperties: false, required: ['severity', 'what', 'fix'], properties: { severity: { type: 'string', enum: ['critical', 'high', 'medium', 'low'] }, what: { type: 'string' }, where: { type: 'string' }, fix: { type: 'string' } } } } } };
const MERGE_SCHEMA = { type: 'object', additionalProperties: false, required: ['merged', 'notes'], properties: { merged: { type: 'boolean' }, mergeSha: { type: 'string' }, notes: { type: 'string' } } };

log(`Yap CI/CD loop [${runLabel}] — ${batch.length} items in ${REPO}`);
const results = [];
for (let i = 0; i < batch.length; i++) {
  const item = batch[i];
  phase('Build');
  log(`[${item.id}] BUILD (${i + 1}/${batch.length}): ${item.title}`);
  const build = await agent(buildPrompt(item), { label: `build:${item.id}`, phase: 'Build', schema: BUILD_SCHEMA, effort: 'high' });
  if (!build || !build.opened || !build.prNumber) { results.push({ id: item.id, status: 'no-pr', notes: build && build.notes }); log(`[${item.id}] no PR: ${build && build.notes}`); continue; }
  phase('CI+Review');
  let review = await agent(reviewPrompt(item, build), { label: `review:${item.id}`, phase: 'CI+Review', schema: REVIEW_SCHEMA, effort: 'high' });
  let round = 0;
  while (review && (!review.ciGreen || review.verdict === 'FAIL') && round < MAX_FIX_ROUNDS) {
    round++;
    phase('Fix');
    log(`[${item.id}] FIX round ${round}`);
    await agent(fixPrompt(item, build, review), { label: `fix:${item.id}:r${round}`, phase: 'Fix', schema: BUILD_SCHEMA, effort: 'high' });
    phase('CI+Review');
    review = await agent(reviewPrompt(item, build), { label: `review:${item.id}:r${round}`, phase: 'CI+Review', schema: REVIEW_SCHEMA, effort: 'high' });
  }
  const green = review && review.ciGreen && review.verdict === 'PASS' && !review.scopeCreep;
  let merged = null;
  if (green) { phase('Merge'); merged = await agent(mergePrompt(item, build), { label: `merge:${item.id}`, phase: 'Merge', schema: MERGE_SCHEMA }); }
  else log(`[${item.id}] NOT merged — left open for a human`);
  results.push({ id: item.id, prNumber: build.prNumber, prUrl: build.prUrl, ciGreen: review && review.ciGreen, verdict: review && review.verdict, fixRounds: round, merged: merged && merged.merged, status: merged && merged.merged ? 'merged' : 'open' });
  log(`[${item.id}] → ${merged && merged.merged ? 'MERGED' : 'LEFT OPEN'}`);
}
const mergedN = results.filter(r => r.status === 'merged').length;
log(`Yap CI/CD loop done: ${mergedN}/${batch.length} merged`);
return { loop: runLabel, merged: mergedN, total: batch.length, results };
