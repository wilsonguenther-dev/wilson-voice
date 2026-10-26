// yap-harness-defects-2026-10-19 — seven harness defects the Yap build/review passes hit on 2026-10-19
// (Projects/Loop-Logs/yap-launch-plan-2026-10-16.md 00:40Z-13:20Z, STATUS-yap.md rows PERM-A/PERM-D,
// 2026-09-12-yap-part01.md lines 218, 236, 240-244). Filed by seat yap-harness-items. Sorted after 07c
// (licensing) so this lane runs after the P0, walk and licensing lanes.
//
// CLASS: every item that edits scripts/loop/ or scripts/cicd-loop-all.mjs or .github/workflows/ is
// SECURITY (template SECURITY_PATHS): two Opus seats must PASS its head. SHARED PREAMBLE + STANDARD GATE:
// 00-y0-harness-and-gates.mjs. NO GUESSING: the root-cause probe goes into the PR body under
// "Root cause, proven" AND into docs/loop/evidence/<item id>.md, first line exactly
// "ROOT-CAUSE-PROBE <item id>".
//
// HARNESS TESTS: every new harness test lives in scripts/loop/harness-defects.test.mjs (node --test,
// HERMETIC: no gh, no git remote, no cargo, no network) and uses EXACTLY the test names the
// acceptances grep for. template.mjs is a Workflow script, not an importable module: test its pure
// helpers by exporting them from a small sibling module that build.mjs stamps into the template the
// way it already stamps review-gate.mjs constants, or by evaluating the generated part with stubbed
// agent()/log() exactly like build.mjs's dry run (5q). Either way the test must exercise the code the
// generated parts actually run, not a copy.
// Dependency holds below assemble the marker at run time ("DEPENDENCY-NOT-ON-$(printf MAIN)").

ITEMS.push({
  id: 'yap-hx-done-check-by-main', prompt: 'yap-hx-done-check-by-main', branch: 'loop/yap-hx-done-check-decides-from-main', gated: null,
  title: 'SECURITY: an item whose work is already on main is retired from MAIN\'S content or its merged PR, never by whether origin/<branch> still exists; a merged item\'s deleted branch is never recreated',
  preflight: `
    grep -q 'merged item with a deleted branch is already-done' scripts/loop/harness-defects.test.mjs && grep -q 'doneByMain' scripts/loop/template.mjs
  `,
  spec: `
    Panel: not required — FIX, class SECURITY (harness; edits scripts/loop/).
    Evidence (2026-10-19, build pass lanes:1): PERM-A (merged #157) and PERM-D (merged #164) were
    re-litigated. Their PREFLIGHTS went red on main for stale reasons — PERM-A's names a test target
    that was renamed (05-y1-audio-permission.mjs:50 "--test mic_auth"; the file is
    tests/mic_auth_status.rs), PERM-D's greps symbols that never shipped (05-y1-audio-permission.mjs:420
    'silent_take|all_zero_samples'; shipped names are is_silent_capture / TakeOutcome::SilentCapture).
    The builder then correctly found the work on main and opened no PR (part01 log :240, :243). But
    template.mjs runBuild STEP 2 (:1140 branchTo, :1141 runCommands('acceptance', ...)) runs the
    acceptance on "git checkout <item.branch> && git reset --hard origin/<item.branch>"; the merged
    PR's branch was deleted on merge, so the checkout failed, the acceptance "failed", and the ONE FIX
    ROUND (:1157-1190) "fixed" it by recreating origin/loop/perm-a-real-tcc-authorization-status and
    origin/loop/perm-d-first-run-preflight-before-any-take at main (part01 log :241, :244; STATUS rows
    "REPLAY ... branch recreated at main"). The saysAlreadyDone belt-and-braces (:1129-1133) only logs.
    Branch existence decided the outcome; main's content and the merged PR did not.
    DO 1 — export a pure doneByMain({ mergedPr, builderPr, builderSaysAlreadyDone, mainAcceptance })
    and use it in runBuild between STEP 1 and STEP 2: when the builder returned no PR (or said
    already-done), or "gh pr list --head <branch> --state merged" names a PR, run the item's
    acceptance on DETACHED origin/main (same runner seat, where = "git checkout --detach origin/main");
    allZero there -> status 'already-done' with evidence "merged PR #N, acceptance on main <sha>",
    no fix round, no branch, no push. Acceptance red on main with a merged PR -> status
    'regressed-on-main' (its own status, a human reads it), still no branch recreation.
    DO 2 — the fix-round prompt (and the PR-resume block at :1041-1060) forbid pushing a branch whose
    PR is merged: check "gh pr list --head <branch> --state merged" first and return
    {fixed:false, mergedPr:N} instead.
    DO 3 — build.mjs validate rule: refuse any item preflight or acceptance that names an origin/loop/
    ref (branch existence is never a done signal). 55-y11's 'origin/feat/yv1*' absence checks are
    retirement checks on human branches and stay.
    DO 4 — fix the two stale preflights: PERM-A "--test mic_auth" -> "--test mic_auth_status"; PERM-D
    grep -> 'is_silent_capture' in desktop/src-tauri/src/lib.rs. Do not touch their specs.
    Tests (scripts/loop/harness-defects.test.mjs): "merged item with a deleted branch is already-done"
    (no origin branch, mergedPr 157, acceptance on main allZero -> 'already-done', and the captured
    prompts contain no "git push" for that branch); "merged item red on main is regressed-on-main";
    "loop:validate refuses origin/loop/ in an item preflight or acceptance" (inject into a temp copy).
  `,
  acceptance: `
    mkdir -p desktop/target
    node --test --test-reporter=tap 'scripts/loop/*.test.mjs' > desktop/target/qa-hx-done.txt 2>&1
    grep -q '^# fail 0$' desktop/target/qa-hx-done.txt
    grep -Eq '^ *ok [0-9]+ - merged item with a deleted branch is already-done$' desktop/target/qa-hx-done.txt
    grep -Eq '^ *ok [0-9]+ - merged item red on main is regressed-on-main$' desktop/target/qa-hx-done.txt
    grep -Eq '^ *ok [0-9]+ - loop:validate refuses origin/loop/ in an item preflight or acceptance$' desktop/target/qa-hx-done.txt
    ! git grep -nE 'origin/loop/' -- scripts/loop/items ':(exclude)scripts/loop/items/07d-yap-harness-defects-2026-10-19.mjs'
    grep -q -- '--test mic_auth_status' scripts/loop/items/05-y1-audio-permission.mjs
    ! grep -qE 'custom-protocol --test mic_auth$' scripts/loop/items/05-y1-audio-permission.mjs
    ! grep -q 'all_zero_samples' scripts/loop/items/05-y1-audio-permission.mjs
    test -s docs/loop/evidence/yap-hx-done-check-by-main.md
    head -1 docs/loop/evidence/yap-hx-done-check-by-main.md | grep -qx 'ROOT-CAUSE-PROBE yap-hx-done-check-by-main'
    (cd desktop && npm run loop:build && npm run loop:validate)
  `,
})

ITEMS.push({
  id: 'yap-hx-lanes1-skip-laneB-warm', prompt: 'yap-hx-lanes1-skip-laneB-warm', branch: 'loop/yap-hx-lanes1-recon-skips-lane-b', gated: null,
  title: 'SECURITY: a build pass run with lanes:1 never creates, installs or warms lane B — recon sets up lane A only and leaves any existing lane B untouched',
  preflight: `
    grep -q 'lanes 1 recon never creates or warms lane B' scripts/loop/harness-defects.test.mjs
  `,
  spec: `
    Panel: not required — FIX, class SECURITY (harness).
    Root cause: template.mjs BUILD_LANES (:825-826) is honoured only by the item split (:1987-1990)
    and a log line (:1994). BUILD_RECON (:1784-1830) is a constant that ALWAYS creates both worktrees
    (:1794-1795 "git worktree add <WORKDIR_B> -b yap/recon-b"), npm-ci's both lanes, and warms both
    cargo targets ("the same for lane B with <WORKDIR_B> and <CARGO_TARGET_B>"). Under lanes:1 lane B
    is idle by design, yet recon paid a cold cargo build into target-b (~10-12 GB) every pass
    (launch-plan 2026-10-19T05:57Z: "lane B idle under lanes:1, yet recon re-warmed it"; 04:28Z and
    05:57Z the operator deleted target-b by hand to recover disk).
    DO — make the recon prompt a function of BUILD_LANES. lanes:1: create/reuse/install/warm lane A
    only; do not create lane B, do not run npm ci or cargo in it, do not delete it if it exists (an
    earlier lanes:2 part may own it), and say "lane B skipped (lanes:1)" in the recon return. The
    recon/STATUS row says the same. lanes:2 behaviour is byte-for-byte unchanged. Check every other
    prompt that names WORKDIR_B / CARGO_TARGET_B for the build pass (WORKTREE_CONTRACT 'other' lane,
    drain) and make lanes:1 leave lane B alone there too.
    Tests: "lanes 1 recon never creates or warms lane B" (generate/evaluate the build part with
    args {mode:'build', lanes:1}, capture the recon prompt: it contains no "worktree add" for
    WORKDIR_B and no CARGO_TARGET_B) and "lanes 2 recon still warms both lanes".
  `,
  acceptance: `
    mkdir -p desktop/target
    node --test --test-reporter=tap 'scripts/loop/*.test.mjs' > desktop/target/qa-hx-lanes1.txt 2>&1
    grep -q '^# fail 0$' desktop/target/qa-hx-lanes1.txt
    grep -Eq '^ *ok [0-9]+ - lanes 1 recon never creates or warms lane B$' desktop/target/qa-hx-lanes1.txt
    grep -Eq '^ *ok [0-9]+ - lanes 2 recon still warms both lanes$' desktop/target/qa-hx-lanes1.txt
    test -s docs/loop/evidence/yap-hx-lanes1-skip-laneB-warm.md
    head -1 docs/loop/evidence/yap-hx-lanes1-skip-laneB-warm.md | grep -qx 'ROOT-CAUSE-PROBE yap-hx-lanes1-skip-laneB-warm'
    (cd desktop && npm run loop:build && npm run loop:validate)
  `,
})

ITEMS.push({
  id: 'yap-hx-reviewers-reuse-target', prompt: 'yap-hx-reviewers-reuse-target', branch: 'loop/yap-hx-reviewer-target-dir-and-no-incremental', gated: null,
  title: 'SECURITY: reviewer and fixer seats build only into their chain\'s target-r1/target-r2 (never a cargo target in the scratchpad), and every warm/gate/test command runs with CARGO_INCREMENTAL=0',
  preflight: `
    grep -q 'reviewer and fixer prompts pin CARGO_TARGET_DIR to their chain target' scripts/loop/harness-defects.test.mjs && grep -q 'export CARGO_INCREMENTAL=0' scripts/loop/template.mjs
  `,
  spec: `
    Panel: not required — FIX, class SECURITY (harness).
    Root cause 1 (target dir): reviewOnePr (template.mjs :1334) computes target = REVIEW_TARGETS[chain]
    (:1337) but neither primaryPrompt (:1356) nor lensPrompt (:1405) interpolates it: both say "read-only
    greps only" in the chain dir, so a reviewer that wants to build or test the PR head makes its own
    tree and runs cargo with the DEFAULT target — inside that tree. 2026-10-19T05:02Z review-opus2 built
    a 12 GB target in scratchpad/pr229o2; 01:45Z and 03:34Z 4-8 GB targets (target-pr227, target-opus2)
    were found the same way. Only the land/fix prompts and REVIEW_RECON (:1879-1901) name target-rN.
    Root cause 2 (incremental): no harness command sets CARGO_INCREMENTAL. GATE_CMDS (:462-473), the
    WORKTREE_CONTRACT export (:313), runCommands WHERE (:919) and both recons' warm steps run debug
    cargo test/clippy with incremental ON, so every one-shot gate regrew 5-10 GB of
    target-*/debug/incremental per half hour (launch-plan 06:23Z, 06:47Z, 11:34Z). The operator's
    stopgap is ~/code/wilson-voice-loop/.cargo/config.toml [build] incremental=false (11:34Z) — machine
    state outside the repo that a fresh host does not have.
    DO 1 — both reviewer prompts and every fix-round prompt get a REVIEWER BUILD CONTRACT block: "if you
    build or test the PR head, do it in a detached worktree of LOCAL_REPO with
    export CARGO_TARGET_DIR=<this chain's target-rN> (the build-pass fixer: its lane's target); never
    run cargo with a target dir under the scratchpad, /tmp or the tree you made; remove your worktree
    (git worktree remove) before returning". The fixer's WORKTREE_CONTRACT already pins its target —
    keep it.
    DO 2 — "export CARGO_INCREMENTAL=0" next to EVERY "export CARGO_TARGET_DIR=" in template.mjs
    (GATE_CMDS, WORKTREE_CONTRACT, runCommands, BUILD_RECON, REVIEW_RECON, main-verify, the dev-launch
    lines). Keep the .cargo/config.toml stopgap; note in a comment that the env var makes it redundant.
    Tests: "reviewer and fixer prompts pin CARGO_TARGET_DIR to their chain target" (evaluate the review
    part with stubbed agent(), capture every reviewer/fix prompt per chain: each contains
    "export CARGO_TARGET_DIR=<REVIEW_TARGETS[chain]>" and the word scratchpad in a never-build line)
    and "every prompt that exports CARGO_TARGET_DIR also exports CARGO_INCREMENTAL=0" (build and review
    parts, every captured prompt).
  `,
  acceptance: `
    mkdir -p desktop/target
    node --test --test-reporter=tap 'scripts/loop/*.test.mjs' > desktop/target/qa-hx-target.txt 2>&1
    grep -q '^# fail 0$' desktop/target/qa-hx-target.txt
    grep -Eq '^ *ok [0-9]+ - reviewer and fixer prompts pin CARGO_TARGET_DIR to their chain target$' desktop/target/qa-hx-target.txt
    grep -Eq '^ *ok [0-9]+ - every prompt that exports CARGO_TARGET_DIR also exports CARGO_INCREMENTAL=0$' desktop/target/qa-hx-target.txt
    grep -q 'export CARGO_INCREMENTAL=0' scripts/loop/template.mjs
    test -s docs/loop/evidence/yap-hx-reviewers-reuse-target.md
    head -1 docs/loop/evidence/yap-hx-reviewers-reuse-target.md | grep -qx 'ROOT-CAUSE-PROBE yap-hx-reviewers-reuse-target'
    (cd desktop && npm run loop:build && npm run loop:validate)
  `,
})

ITEMS.push({
  id: 'yap-hx-dependency-marker-scan', prompt: 'yap-hx-dependency-marker-scan', branch: 'loop/yap-hx-dependency-marker-last-stdout-line', gated: null,
  title: 'SECURITY: the dependency hold reads DEPENDENCY-NOT-ON-MAIN only from the LAST stdout line of an executed preflight command (a per-command field in the runner schema), never from the mixed output tail or command text',
  preflight: `
    ( M=0; grep -q 'dependencyMarkerPrinted(pre)' scripts/loop/template.mjs || M=1; if [ "$M" = 1 ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): loop-dependency-marker-stdout-only (07b) has not landed — run the review pass to land it, then the build pass again"; exit 0; fi; grep -q 'lastLine' scripts/loop/template.mjs && grep -q 'dependency marker counts only as the last stdout line' scripts/loop/harness-defects.test.mjs && grep -q 'silent exit-0 preflight is already-done' scripts/loop/harness-defects.test.mjs )
  `,
  spec: `
    Panel: not required — FIX, class SECURITY (harness). REMAINING GAP ONLY — read what is already filed.
    Already covered by #230 (merged ca50beb): (a) build.mjs:301-312 refuses an item preflight that
    contains the LITERAL marker (item files assemble it at run time); (b) item
    loop-dependency-marker-stdout-only (07b, not yet built) replaces template.mjs:986
    "/DEPENDENCY-NOT-ON-MAIN/.test(asText(pre))" with dependencyMarkerPrinted(pre) matching
    /^DEPENDENCY-NOT-ON-MAIN:/m in "the executed commands' captured stdout". This item depends on (b)
    and is held until it lands.
    The gap: RUN_SCHEMA (template.mjs :857-878) has NO per-command stdout. exits[] is {command, exit};
    output is ONE seat-written string, "the raw tail of each command's output, pasted, at most ~80
    lines total" (runCommands :935-937). So "captured stdout" in (b) can only mean that mixed tail, in
    which a marker line can come from a command that merely PRINTS file content (a grep -n or cat of an
    item file / generated part that documents the marker), from the seat's own echo of a header
    comment, or from a non-final line of a command whose real check then ran and failed. The /m flag
    accepts any line. The 2026-10-19 mis-mark of yap-p0-perm-repair (launch-plan 03:27Z; reviewer
    note "unproven D7 runner-cwd premise", part01 :206) is exactly that class.
    DO — RUN_SCHEMA exits[] items gain lastLine, a REQUIRED string field: the final non-empty line
    that command wrote to stdout, verbatim, or the EMPTY STRING "" when it wrote no non-empty line.
    "" is a VALID value and means SILENT, never missing: a passing grep -q, test, or && chain prints
    nothing and exits 0, and runCommands (template.mjs) already rules "a command that prints nothing
    and exits 0 PASSED" (the runner prompt says how: run each command as
    "<cmd> > <tmp> 2>&1; echo exit=$?; tail -n 1 <tmp>" — bare exit, no pipe on the command itself).
    dependencyMarkerPrinted(pre) becomes: true only when SOME exits[i] has exit 0 AND its lastLine
    starts with "DEPENDENCY-NOT-ON-MAIN:"; output, command, notes and every other field are never
    consulted. FAIL CLOSED ON MISSING EVIDENCE (fix round 1, independent reviewer BLOCKING 1): every
    hold preflight echoes the marker and then exits 0, so runPassed(pre) is TRUE for a held item and
    the normal verdict for it is already-done (template.mjs:989-992). Therefore, whenever
    runPassed(pre) is true and ANY exits[i] with exit 0 has a lastLine field that is ABSENT, is not a
    string, or contains a newline (ambiguous), the item is INCONCLUSIVE: return status
    'held: preflight inconclusive (no lastLine)', log "runner returned no usable lastLine — held,
    never already-done", dispatch no builder, and let the next pass re-run the preflight. A missing
    or mangled lastLine can NEVER yield already-done, whether or not the marker text appears
    anywhere else in the payload. An EMPTY-STRING lastLine is NOT inconclusive (fix round 2, both
    reviewers BLOCKING: holding on "" made already-done unreachable for every preflight whose passing
    command is silent — 10 of the 12 grep-only preflights that pass on main e60ea75, 118 of 154
    preflights at e2eccff, and this item's own preflight once it lands — so they would be held on
    every pass and never retired). already-done requires runPassed(pre) AND every exit-0 command
    carrying a string lastLine with no newline ("" included) AND none of them starting with the
    marker. build.mjs: refuse an item preflight
    whose marker echo is not immediately followed by "exit 0" inside the same subshell (the shape
    every hold uses), so the marker is always that command's last line.
    Tests (harness-defects.test.mjs): "dependency marker counts only as the last stdout line"
    (lastLine marker + exit 0 -> true); "marker on a non-final stdout line is ignored" (marker only in
    output tail, lastLine something else -> false); "marker in command text or notes is ignored";
    "loop:validate refuses a marker echo not followed by exit 0";
    "absent lastLine field with the marker in output is held, never already-done" (all exit 0,
    marker in output, the lastLine KEY absent -> status starts with 'held:', not 'already-done');
    "absent lastLine field without the marker is held, never already-done" (all exit 0, no marker
    anywhere, one exits[i] with NO lastLine key -> 'held:'); "non-string lastLine is held" (lastLine
    null or 0 -> 'held:'); "multi-line lastLine is ambiguous and held";
    "silent exit-0 preflight is already-done" (every exits[i] exit 0 with lastLine "", no marker
    anywhere -> status 'already-done', NOT 'held:' — the liveness case; it must fail on a build that
    treats "" as inconclusive).
  `,
  acceptance: `
    mkdir -p desktop/target
    node --test --test-reporter=tap 'scripts/loop/*.test.mjs' > desktop/target/qa-hx-marker.txt 2>&1
    grep -q '^# fail 0$' desktop/target/qa-hx-marker.txt
    grep -Eq '^ *ok [0-9]+ - dependency marker counts only as the last stdout line$' desktop/target/qa-hx-marker.txt
    grep -Eq '^ *ok [0-9]+ - marker on a non-final stdout line is ignored$' desktop/target/qa-hx-marker.txt
    grep -Eq '^ *ok [0-9]+ - marker in command text or notes is ignored$' desktop/target/qa-hx-marker.txt
    grep -Eq '^ *ok [0-9]+ - loop:validate refuses a marker echo not followed by exit 0$' desktop/target/qa-hx-marker.txt
    grep -Eq '^ *ok [0-9]+ - absent lastLine field with the marker in output is held, never already-done$' desktop/target/qa-hx-marker.txt
    grep -Eq '^ *ok [0-9]+ - absent lastLine field without the marker is held, never already-done$' desktop/target/qa-hx-marker.txt
    grep -Eq '^ *ok [0-9]+ - non-string lastLine is held$' desktop/target/qa-hx-marker.txt
    grep -Eq '^ *ok [0-9]+ - multi-line lastLine is ambiguous and held$' desktop/target/qa-hx-marker.txt
    grep -Eq '^ *ok [0-9]+ - silent exit-0 preflight is already-done$' desktop/target/qa-hx-marker.txt
    grep -q 'preflight inconclusive (no lastLine)' scripts/loop/template.mjs
    grep -q 'lastLine' scripts/loop/template.mjs
    ! grep -q 'DEPENDENCY-NOT-ON-MAIN/.test(asText' scripts/loop/template.mjs
    test -s docs/loop/evidence/yap-hx-dependency-marker-scan.md
    head -1 docs/loop/evidence/yap-hx-dependency-marker-scan.md | grep -qx 'ROOT-CAUSE-PROBE yap-hx-dependency-marker-scan'
    (cd desktop && npm run loop:build && npm run loop:validate)
  `,
})

ITEMS.push({
  id: 'yap-hx-test-build-repo-resolve', prompt: 'yap-hx-test-build-repo-resolve', branch: 'loop/yap-hx-test-build-resolves-repo', gated: null,
  title: 'scripts/test-build.sh works from any checkout: --repo <path>, else the $PWD checkout, else its own path, and a clear named error (exit 2) when none is a wilson-voice checkout',
  preflight: `
    ( M=0; test -f scripts/test-build.sh || M=1; if [ "$M" = 1 ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR #238 (Yap test channel, scripts/test-build.sh) has not landed — run the review pass to land it, then the build pass again"; exit 0; fi; grep -q -- '--repo' scripts/test-build.sh && grep -q -- '--print-repo' scripts/test-build.sh && grep -q 'not a wilson-voice checkout' scripts/test-build.sh )
  `,
  spec: `
    Panel: not required — FIX (test tooling; depends on #238).
    Root cause: #238's scripts/test-build.sh:20 is ROOT="$(cd "$(dirname "$0")/.." && pwd)" — the repo
    is wherever the SCRIPT file sits. 2026-10-19T12:46Z the operator ran a copy of the script taken
    from origin/feat/yap-test-channel (local main predated #238) from outside a checkout and it died
    with "not a git repository" (launch-plan 12:46Z attempt 1); it only worked after a dedicated
    worktree was made (tc-run).
    DO — resolution order: (1) --repo <path>; (2) the git toplevel of $PWD when it is a wilson-voice
    checkout (has desktop/src-tauri/tauri.conf.json and scripts/test-build.sh's sibling files);
    (3) the script's own ../ when that is one; else print
    "test-build: not a wilson-voice checkout: pass --repo <path> or run from inside one" to stderr and
    exit 2. Print the resolved repo and which rule chose it as the first output line. New flag
    --print-repo: resolve, print the absolute repo path alone on stdout, exit 0, build nothing.
    --help documents both. Extend scripts/test-build.test.sh with the three cases.
  `,
  acceptance: `
    ( R=$(git rev-parse --show-toplevel) && T=$(mktemp -d) && cp scripts/test-build.sh "$T/" && cd "$T" && test "$(bash ./test-build.sh --repo "$R" --print-repo)" = "$R" )
    ( R=$(git rev-parse --show-toplevel) && T=$(mktemp -d) && cp scripts/test-build.sh "$T/" && cd "$R" && test "$(bash "$T/test-build.sh" --print-repo)" = "$R" )
    ( T=$(mktemp -d) && cp scripts/test-build.sh "$T/" && cd "$T" && bash ./test-build.sh --print-repo > "$T/out.txt" 2>&1; rc=$?; test "$rc" -eq 2 && grep -q 'not a wilson-voice checkout' "$T/out.txt" )
    bash scripts/test-build.test.sh
    shellcheck scripts/test-build.sh
  `,
})

ITEMS.push({
  id: 'yap-hx-flaky-timing-under-load', prompt: 'yap-hx-flaky-timing-under-load', branch: 'loop/yap-hx-long-take-budget-load-aware', gated: null,
  title: 'long_take_budget::chunk_decode_wall_per_audio_second is load-aware: above one runnable task per core it reports NOT-MEASURED (never green); every gate runs it alone with --nocapture and counts budget=measured only from a positive BUDGET-MEASURED line, so a loaded host neither fails main-verify on a timing ratio nor passes it unmeasured',
  preflight: `
    grep -q 'fn load_gate' desktop/src-tauri/tests/long_take_budget.rs && grep -q 'BUDGET-MEASURED ratio=' desktop/src-tauri/tests/long_take_budget.rs && grep -q 'BUDGET-MEASURED' scripts/loop/template.mjs && grep -q -- '--test-threads=1 --nocapture' scripts/loop/template.mjs
  `,
  spec: `
    Panel: not required — FIX (test flake).
    Root cause: desktop/src-tauri/tests/long_take_budget.rs:381 chunk_decode_wall_per_audio_second
    asserts ratio <= CHUNK_DECODE_WALL_PER_AUDIO_SECOND_RATIO_CEILING (:97, 1.60) at :453 — a wall-clock
    ratio of chunked vs one-shot decode. Under host load the chunked leg is starved: 2026-10-19 recon
    baseline cargotest=101 at load avg 55 with lane B building (part01 :218), main-verify run 2 ratio
    3.68x at load avg 19 (part01 :236, launch-plan 11:43Z); the same test passes alone. main-verify
    needed three cargo test runs to go green.
    DO — pure fn load_gate(load1: f64, cores: usize) -> Option<String> in the test file: Some(reason)
    when load1 > cores as f64 (one runnable task per core), else None. Read load1 with
    libc::getloadavg (libc is already a dependency) and cores with
    std::thread::available_parallelism. NO environment override of the load (fix round 1,
    independent reviewer BLOCKING 2: an env var that turns the gate off fails open); the unit tests
    call the pure fn with literal values instead.
    FAIL CLOSED — A SKIP IS NEVER GREEN (fix round 1, BLOCKING 2: this host has 12 cores and loop
    loads of 19-55, so a skip-as-pass would leave the 1.60 budget unmeasured in every gate). The gate
    runs FIRST in the test, before the model-present check. On Some it re-reads the load up to 3
    times, 20 s apart (not holding HEAVY), and measures as soon as load1 <= cores. If the host is
    still loaded it prints exactly one line
    "LOAD-AWARE NOT-MEASURED: 1-min load <load1 .2> > <cores> cores — chunk_decode ratio not measured"
    and returns. That line is NOT a pass: the harness treats it as not-measured.
    POSITIVE EVIDENCE ONLY (fix round 2, both reviewers BLOCKING: libtest CAPTURES and DISCARDS the
    println!/eprintln! output of a PASSING test unless --nocapture or --show-output is given — probe
    with cargo 1.94.0: a passing test that prints the line, run as plain 'cargo test > f 2>&1', left
    0 matching lines and only '... ok' in f; with '-- --nocapture' the line was there. So grepping the
    ordinary gate run for a NEGATIVE marker reads "measured" every time, and the skip() path for
    absent ASR weights does too). On the one path that actually timed both decode arms, the test
    prints exactly one line, after computing ratio and BEFORE the ceiling assert:
    "BUDGET-MEASURED ratio=<ratio .3> ceiling=<ceiling .2> load1=<load1 .2> cores=<cores>",
    preceded by a bare println!() — under '--test-threads=1 --nocapture' libtest writes
    "test <name> ... " with NO newline, so the test's first printed line shares that line (probe,
    cargo 1.94.0: "test chunk_decode_wall_per_audio_second ... BUDGET-MEASURED ratio=1.234 ...").
    The LOAD-AWARE NOT-MEASURED path and the skip() path (ASR weights absent) never print it.
    HARNESS (scripts/loop/template.mjs): add an exported pure fn budgetMeasured(text, exit) -> true
    ONLY when exit === 0 AND text has a line matching
    /^(test [^ ]+ [.][.][.] )?BUDGET-MEASURED ratio=[0-9]+[.][0-9]+ /m (bare, or after libtest's
    test-name prefix);
    every other input -> false: no such line, a LOAD-AWARE NOT-MEASURED line, a
    "long_take_budget SKIPPED" line, empty text, or a nonzero exit. The ABSENCE of a negative marker
    NEVER means measured. Every gate block that runs "cargo test --features custom-protocol" (the
    build gate near :474, the review/fix gates, and MAIN VERIFY near :2212) keeps that run as it is
    and THEN runs the budget test ON ITS OWN, serially and UNCAPTURED, redirected to a file, exit
    read bare:
    "cargo test --features custom-protocol --test long_take_budget chunk_decode_wall_per_audio_second -- --test-threads=1 --nocapture > <file> 2>&1 ; echo budgettest=$?"
    It runs at most 3 times in all, again only while budgetMeasured(file, budgettest) is false AND
    the file has no BUDGET-MEASURED line (a measured ratio over the ceiling exits nonzero: that is a
    real red, never retried into green). Report "budget=measured" or "budget=not-measured" beside
    cargotest=, decided by budgetMeasured on the dedicated run's file and exit alone.
    budget=not-measured is NOT green: a land seat does not merge on it (status review-pending,
    reason "budget not measured"), and main-verify lists it as a not-measured row, never inside
    "all exit 0". Below the gate the 1.60 ceiling is unchanged — do not widen it. Rust unit tests
    load_gate_skips_above_core_count / load_gate_runs_at_or_below_core_count; node tests (in
    scripts/loop/harness-defects.test.mjs) "a LOAD-AWARE NOT-MEASURED cargo test is never green";
    "captured cargo test output with no BUDGET-MEASURED line is not measured" (input
    the two lines "test chunk_decode_wall_per_audio_second ... ok" and "test result: ok. 1 passed", exit 0 -> false —
    exactly what a captured passing run looks like); "a skipped budget run is not measured"
    ("long_take_budget SKIPPED — ASR weights absent ...", exit 0 -> false); "only a BUDGET-MEASURED
    line with exit 0 is measured" ("BUDGET-MEASURED ratio=1.234 ceiling=1.60 load1=3.10 cores=12"
    with exit 0 -> true, with exit 101 -> false); "BUDGET-MEASURED after the libtest test-name
    prefix is measured" ("test chunk_decode_wall_per_audio_second ... BUDGET-MEASURED ratio=1.234
    ceiling=1.60 load1=3.10 cores=12", exit 0 -> true).
    Note it in docs/BUDGETS.md next to the ceiling.
  `,
  acceptance: `
    mkdir -p desktop/target
    (cd desktop/src-tauri && cargo test --features custom-protocol --test long_take_budget load_gate) > desktop/target/qa-hx-loadgate.txt 2>&1
    grep -q 'load_gate_skips_above_core_count ... ok' desktop/target/qa-hx-loadgate.txt
    grep -q 'load_gate_runs_at_or_below_core_count ... ok' desktop/target/qa-hx-loadgate.txt
    ! grep -q 'YAP_BUDGET_LOAD_OVERRIDE' desktop/src-tauri/tests/long_take_budget.rs
    grep -q 'LOAD-AWARE NOT-MEASURED: 1-min load' desktop/src-tauri/tests/long_take_budget.rs
    ! grep -q 'LOAD-AWARE skip' desktop/src-tauri/tests/long_take_budget.rs
    node --test --test-reporter=tap 'scripts/loop/*.test.mjs' > desktop/target/qa-hx-budget.txt 2>&1
    grep -q '^# fail 0$' desktop/target/qa-hx-budget.txt
    grep -Eq '^ *ok [0-9]+ - a LOAD-AWARE NOT-MEASURED cargo test is never green$' desktop/target/qa-hx-budget.txt
    grep -Eq '^ *ok [0-9]+ - captured cargo test output with no BUDGET-MEASURED line is not measured$' desktop/target/qa-hx-budget.txt
    grep -Eq '^ *ok [0-9]+ - a skipped budget run is not measured$' desktop/target/qa-hx-budget.txt
    grep -Eq '^ *ok [0-9]+ - only a BUDGET-MEASURED line with exit 0 is measured$' desktop/target/qa-hx-budget.txt
    grep -Eq '^ *ok [0-9]+ - BUDGET-MEASURED after the libtest test-name prefix is measured$' desktop/target/qa-hx-budget.txt
    grep -q 'budget=not-measured' scripts/loop/template.mjs
    grep -q 'BUDGET-MEASURED ratio=' desktop/src-tauri/tests/long_take_budget.rs
    grep -q -- 'chunk_decode_wall_per_audio_second -- --test-threads=1 --nocapture' scripts/loop/template.mjs
    (cd desktop/src-tauri && cargo test --features custom-protocol --test long_take_budget chunk_decode_wall_per_audio_second -- --test-threads=1 --nocapture) > desktop/target/qa-hx-budget-run.txt 2>&1
    grep -Eq '^(test [^ ]+ [.][.][.] )?(BUDGET-MEASURED ratio=[0-9]+[.][0-9]+ |LOAD-AWARE NOT-MEASURED: 1-min load|long_take_budget SKIPPED)' desktop/target/qa-hx-budget-run.txt
    grep -q 'LOAD-AWARE' docs/BUDGETS.md
    grep -q 'RATIO_CEILING: f64 = 1.60;' desktop/src-tauri/tests/long_take_budget.rs
  `,
})

ITEMS.push({
  id: 'yap-hx-fmt-clippy-debt', prompt: 'yap-hx-fmt-clippy-debt', branch: 'loop/yap-hx-fmt-sweep-and-fmt-gate', gated: null,
  title: 'SECURITY: one rustfmt sweep makes cargo fmt --check clean on main and rustfmt becomes a BLOCKING CI step and harness gate; clippy -D warnings stays green on app + sidecars (Y0-A #234 owns the clippy gate)',
  preflight: `
    ( M=0; grep -q 'clippy --all-targets --features custom-protocol -- -D warnings' .github/workflows/ci.yml || M=1; if [ "$M" = 1 ]; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): Y0-A (PR #234, blocking clippy + the yap-diarize main.rs:836 fix) has not landed — run the review pass to land it, then the build pass again"; exit 0; fi; (cd desktop && cargo fmt --all -- --check) && ! grep -q 'continue-on-error' .github/workflows/ci.yml )
  `,
  spec: `
    Panel: not required — FIX, class SECURITY (.github/workflows/ and scripts/loop/template.mjs).
    Root cause, measured on main 7e3276c: "cd desktop && cargo fmt --all -- --check" exits 1 with 43
    "Diff in" hunks (drift from #218/#227/#229/#231 since the b4b8d06 sweep); ci.yml:92-94 runs it as
    "rustfmt (informational)" with a no-fail suffix and ci.yml:95-97 does the same for clippy;
    template.mjs calls fmt informational at :431, :1105, :1374, :1824, :1975, :2224, :2449.
    "cargo clippy -- -D warnings" fails on desktop/yap-diarize/src/main.rs:836
    "assert!(CLUSTER_COUNT_UNKNOWN < 0);" (assertions_on_constants) — PR #234 Y0-A already fixes that
    line (const { assert!(...) }) and already makes clippy -D warnings blocking for the app AND the
    sidecars, keeping rustfmt advisory via continue-on-error "until the next tree-wide sweep".
    DO NOT DUPLICATE Y0-A: this item is held until #234 lands and then does ONLY the remainder:
    DO 1 — the sweep: one mechanical commit "style(yap): cargo fmt --all sweep" (run from desktop/),
    nothing else in it; append its full sha to .git-blame-ignore-revs with a dated comment. If Y0-B
    (#236, which formats 7 drifted files) landed first, sweep what is left.
    DO 2 — ci.yml: the rustfmt step loses continue-on-error and becomes "rustfmt" (a gate). No
    continue-on-error and no no-fail suffix anywhere in ci.yml.
    DO 3 — template.mjs: fmt is a gate. Remove every "fmt informational" carve-out (lines above),
    the recon baseline expects fmt=0, and the reviewer contract stops listing rustfmt drift as
    advisory-because-main-fails.
  `,
  acceptance: `
    (cd desktop && cargo fmt --all -- --check)
    ! grep -q 'continue-on-error' .github/workflows/ci.yml
    ! grep -qF '|| true' .github/workflows/ci.yml
    grep -qE 'run: cargo fmt --all -- --check$' .github/workflows/ci.yml
    (cd desktop/src-tauri && cargo clippy --all-targets --features custom-protocol -- -D warnings)
    (cd desktop && cargo clippy -p yap-polish -p yap-diarize --all-targets --release -- -D warnings)
    test "$(grep -cE '^[0-9a-f]{40}$' .git-blame-ignore-revs)" -ge 2
    ! grep -niE 'fmt.{0,40}informational|informational.{0,12}fmt' scripts/loop/template.mjs
    test -s docs/loop/evidence/yap-hx-fmt-clippy-debt.md
    head -1 docs/loop/evidence/yap-hx-fmt-clippy-debt.md | grep -qx 'ROOT-CAUSE-PROBE yap-hx-fmt-clippy-debt'
    (cd desktop && npm run loop:build && npm run loop:validate)
  `,
})
