// Y0 — the harness floor. Nothing else in this loop is trustworthy until the gates are real.
//
// ════════════════════════════════════════════════════════════════════════════
// SHARED PREAMBLE — every `preflight` and `acceptance` in EVERY item file in
// this directory assumes it has been sourced. It is not repeated per item.
// ════════════════════════════════════════════════════════════════════════════
//
//   REPO  = the fresh clone the loop made. There is NO package.json at the repo
//           root: the app is `desktop/`, which is BOTH the npm project and the
//           Cargo workspace (members: src-tauri, yap-polish, yap-diarize).
//
//   cd ${APP} && npm ci
//
//   # THE SIDECARS ARE A PRECONDITION OF EVERY `cargo build` OF THE APP.
//   # `bundle.externalBin` in tauri.conf.json names binaries/yap-polish and
//   # binaries/yap-diarize, so src-tauri will not build until both are staged.
//   # This is the first command of every gate, not an optional step.
//   stage_sidecars() {
//     cargo build -p yap-polish -p yap-diarize --release
//     TRIPLE="$(rustc -vV | awk '/^host:/ {print $2}')"
//     mkdir -p src-tauri/binaries
//     cp target/release/yap-polish  "src-tauri/binaries/yap-polish-<triple>"
//     cp target/release/yap-diarize "src-tauri/binaries/yap-diarize-<triple>"
//   }
//
//   THE STANDARD GATE — applies to every item, never repeated in `acceptance`.
//   Each exit code is read DIRECTLY. Never through a pipe: a pipe reports the
//   exit code of `tail`, which is how a red gate ships green
//   (the "verification that verifies nothing" rule).
//
//     cd ${APP} && npm ci
//     npm run build            ; BUILD=$?   # `tsc && vite build` — this IS the typecheck
//     npm test                 ; VITEST=$?  # vitest run
//     stage_sidecars
//     cd src-tauri
//     cargo test --features custom-protocol ; RTEST=$?
//     cargo clippy --all-targets --features custom-protocol ; CLIPPY=$?
//     test $BUILD -eq 0 && test $VITEST -eq 0 && test $RTEST -eq 0 \
//       && test $CLIPPY -eq 0
//
//   SUPERSEDED 2026-09-12 (post-panel, pre-launch): the rustfmt sweep LANDED
//   (`cargo fmt --all`, 152 hunks / 21 files, b4b8d06, listed in
//   .git-blame-ignore-revs), so `cargo fmt --all -- --check` exits 0 on main and
//   IS a conjunct of the gate now. Also: clippy runs WITHOUT `-- -D warnings` —
//   a mechanical `clippy --fix` sweep landed (0f413a7) and the 16 lints that
//   survive it need real refactors, which is a loop item and not a gate flag.
//   And the gate builds AND STAGES BOTH sidecars (yap-polish AND yap-diarize):
//   with only polish staged, the app's cargo commands exit 101 in a fresh
//   worktree — and pass on a warm one, which is the false green that made the
//   measured table necessary.
//   `docs/loop/HARNESS.md` + the template's `GATE_CMDS` are the SINGLE source of
//   the gate; this block is a restatement and loses to them on any conflict.
//
//   Until Y0-A lands, CLIPPY is `|| true` in .github/workflows/ci.yml and
//   therefore proves nothing THERE. Y0-A is the first item for that reason.
//
//   ── PANEL 2026-09-12, BINDING ON EVERY ITEM IN EVERY FILE ────────────
//   (a) A `grep -q '<some_test_name>' <a test file this item writes>` proves
//       only that a NAME exists. An empty test body satisfies it and the gate.
//       So: every acceptance block that greps for a test name must ALSO carry a
//       mutation that turns that test RED — mutate a file, re-run, require a
//       non-zero exit, restore, then `git diff --exit-code <file>`. Y0-C is the
//       worked example. Y7-C collects the rows; it does not excuse the item.
//   (b) NEVER anchor an acceptance or pre-flight grep to `desktop/src/App.tsx`.
//       Y5-G moves all seven views out of it, so a path-anchored grep rots and
//       the item re-proves as not-done on every resume. Use
//       `grep -rq '<marker>' desktop/src`, or name the view module.
//   (c) A bare `cargo test --features custom-protocol <filter>` exits 0 when it
//       matches NOTHING (MEASURED: exit 0). Pre-flights therefore name a target:
//       `cargo test --features custom-protocol --test <target>`.
//   (d) NOTHING launches the app without `YAP_DATA_DIR` pointed at a scratch
//       directory and `--smoke` (Y0-D). Two lanes and Wilson's installed copy
//       otherwise share one SQLite history, one settings store, one models dir
//       and one global PTT binding.
//   (e) An acceptance line that already exits 0 on unmodified `origin/main` is
//       not acceptance. Run the block against main before you believe it.
//
//   UI ITEMS additionally owe, in the PR body:
//     * a screenshot of the main window at 980x700 (the configured default) AND
//       at 720x520 (minWidth/minHeight — the size the window can actually be
//       dragged to), because a layout that only works at the default size is
//       the "looks broken" complaint;
//     * a screenshot of the FLOAT PILL window for every state the item touches;
//     * the answer to "does this state read correctly in the 30px side dock?"
//
//   THE APP IS NOT SANDBOXED AND MUST NEVER BECOME SANDBOXED
//   (feedback_never_sandbox_utility_apps). `desktop/src-tauri/Entitlements.plist`
//   pins `com.apple.security.app-sandbox` = <false/>. Any item that touches
//   entitlements must assert the VALUE, not the key.
//
//   OBSERVABILITY: local-only crash capture (`src-tauri/src/crash.rs`). NEVER
//   Sentry, NEVER PostHog, NEVER Segment (feedback_queryguard,
//   reference_wispr_parity_research §5 — "no telemetry" is a shipped claim and a
//   marketing line). `git grep -i "sentry\|posthog\|segment.io" desktop` must
//   stay at 0 matches.
//
//   AUDITED STATE: origin/main @ 4e8c9adf (YV126, 2026-08-16). NOTE for the
//   builder: a local working tree may sit on `main` @ 734aa8c (YV83), which is
//   23 commits BEHIND. Every file:line in these specs is against 4e8c9adf.
//   Fetch and branch from origin/main, never from a stale local main.
//
//   Field contract: docs/loop/HARNESS.md "The item contract". Every field
//   required; `gated` is null or 'panel'.
// ════════════════════════════════════════════════════════════════════════════

ITEMS.push({
  id: 'Y0-A', prompt: 'Y0', branch: 'loop/y0-a-make-the-lint-gates-blocking', gated: null,
  title: 'Clippy becomes a blocking CI gate instead of `|| true` decoration (rustfmt stays informational — ledger)',
  preflight: `
    test 0 -eq "$(grep -c '|| true' .github/workflows/ci.yml)"
    grep -q -- '-D warnings' .github/workflows/ci.yml
    cd ${APP} && npm ci && cd src-tauri
    cargo clippy --all-targets --features custom-protocol -- -D warnings
  `,
  spec: `
    MEASURED at 4e8c9adf, .github/workflows/ci.yml:
      - name: rustfmt (informational)
        run: cargo fmt --all -- --check || true
      - name: clippy (informational)
        run: cargo clippy --all-targets --features custom-protocol || true
    and the comment above them: "Build + tests are the gate. Lints are
    informational until the tree is clean."

    The tree is never going to become clean on its own, and a loop whose merge
    gate cannot see a lint failure will spend eighty items accumulating them.
    This item makes them real, which means it must ALSO make them pass.

    Do, in this order:
      1. PANEL 2026-09-12: do NOT run \`cargo fmt --all\` here. MEASURED on main:
         126 hunks across 19 files, the top four being db.rs (40), dictation.rs
         (38), tests/formatting_fixtures.rs (14) and meeting_asr.rs (10) — i.e.
         exactly the files DB-A..E, Y4-A/C/D/E/F and Y3-B all edit. Reformatting
         them as merge #1 of 79 conflicts roughly fifty later branches (a
         conflicting PR is SKIPPED in build mode and never retried) and destroys
         \`git blame\` on the two largest files in the app. The rustfmt sweep is a
         SEPARATE mechanical commit landed on main BEFORE the loop launches, with
         a \`.git-blame-ignore-revs\` entry; only then does fmt become blocking.
         For this loop rustfmt stays informational, exactly as the ledger says.
      2. cargo clippy --all-targets --features custom-protocol -- -D warnings
         and fix every finding. Fix the CODE. \`#[allow(...)]\` is permitted ONLY
         with a one-line comment naming why the lint is wrong here; a bare
         \`#![allow(clippy::all)]\` at a crate root is a failed item.
      3. Delete both \`|| true\`s. Add \`-D warnings\` to the clippy invocation.
         Drop "(informational)" from both step names.
      4. Run clippy over the sidecars too: \`cargo clippy -p yap-polish
         -p yap-diarize --all-targets --release -- -D warnings\`, as its own step.

    Note \`#![allow(dead_code)]\` at src-tauri/src/transcription.rs:1. Leave it —
    removing it is Y6's dead-code item, and mixing the two makes both unmergeable.

    What NOT to do:
      - Do NOT add \`continue-on-error: true\` to the step. That is \`|| true\` with
        a different spelling and it still reports green.
      - Do NOT silence a lint by deleting the test that triggers it.
    PANEL 2026-09-12, and say this in the PR body: .github/workflows/ci.yml is
    NOT the gate this loop merges on. Actions is disabled account-wide, the
    harness resolves ci=local, and the clippy that actually gates all items is
    the one in the template's GATE_CMDS — which is invoked WITHOUT \`-D warnings\`.
    So this item hardens the CI file (correct, and it is what a future
    Actions-enabled world reads) but it does NOT by itself give the loop a real
    lint gate. Adding \`-- -D warnings\` to GATE_CMDS is a pre-launch harness
    amendment, recorded in docs/loop/PLAN.md §Panel revisions. Until it lands,
    treat every item's clippy=0 as evidence about the CI file only.

  `,
  acceptance: `
    test 0 -eq "$(grep -c '|| true' .github/workflows/ci.yml)"        # 0    (was 2)
    test 0 -eq "$(grep -c 'informational' .github/workflows/ci.yml)"  # 0    (was 2)
    grep -q -- '-D warnings' .github/workflows/ci.yml
    grep -q 'yap-polish -p yap-diarize --all-targets' .github/workflows/ci.yml
    cd ${APP} && npm ci && cd src-tauri
    cargo clippy --all-targets --features custom-protocol -- -D warnings          ; test $? -eq 0
    test 0 -eq "$(grep -rn 'allow(clippy::all)' src | wc -l)"
  `,
})

ITEMS.push({
  id: 'Y0-D', prompt: 'Y0', branch: 'loop/y0-d-per-instance-state-isolation-and-smoke-mode', gated: null,
  title: 'One documented state root override and a --smoke mode, so two lanes and Wilson\'s own install stop sharing one history and one hotkey',
  preflight: `
    grep -q 'YAP_DATA_DIR' desktop/src-tauri/src/lib.rs
    grep -q 'YAP_DATA_DIR' desktop/src-tauri/src/models.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test data_dir_override
  `,
  spec: `
    PANEL 2026-09-12, and this is a PRECONDITION of every item whose proof is
    "the running app". MEASURED at 4e8c9adf:
      desktop/src-tauri/src/lib.rs:551   .join("WilsonVoice")     hardcoded
      desktop/src-tauri/src/models.rs:716  the same
      git grep -iE 'YAP_DATA_DIR|WILSON_VOICE_DATA|data_dir_override|--data-dir'
        -- desktop/src-tauri/src   ->  NO MATCHES
    The harness isolates directories, cargo target dirs and preview ports. It
    does NOT isolate the app's state. So two concurrent lane launches plus
    Wilson's installed copy read and write ONE SQLite history, ONE settings
    store, ONE models dir and ONE recovery dir, all register the same global PTT
    binding, and all synthesize a paste into whatever window has focus on
    Wilson's machine. Y7-B's spec already assumes a flag that does not exist
    ("a TEMPORARY data dir, with a flag that seeds a deterministic fixture
    state"); Y6-D then adds a single-instance guard, which makes the second
    lane's app unlaunchable; DB-B, DB-C, Y3-A and PERM-A are destructive or
    interactive against that shared state.

    Do:
      * \`YAP_DATA_DIR\` — one documented env override honoured at BOTH sites
        (lib.rs:551 and models.rs:716), resolved ONCE into an \`AppPaths\` value
        that everything else reads. Absent -> today's behaviour, unchanged.
      * \`--smoke\` — a launch mode that (1) refuses to register any global
        hotkey, (2) refuses to synthesize a paste, (3) REFUSES TO START at all
        if YAP_DATA_DIR is unset or resolves to the default root, and (4) exits
        non-zero with a named message in each refusal rather than degrading.
      * Do NOT touch the bundle id (TCC) or the default data-dir NAME. This adds
        an override; it renames nothing.
      * Document both in ARCHITECTURE.md's Runtime Dependencies table, and state
        the run contract: no agent launches the app without YAP_DATA_DIR.

    What NOT to do:
      - Do NOT make YAP_DATA_DIR the only path and delete the default.
      - Do NOT let --smoke silently fall back to the real data dir. The whole
        point is that a smoke run cannot touch Wilson's dictation history.
  `,
  acceptance: `
    grep -q 'YAP_DATA_DIR' desktop/src-tauri/src/lib.rs
    grep -q 'YAP_DATA_DIR' desktop/src-tauri/src/models.rs
    grep -q 'YAP_DATA_DIR' ARCHITECTURE.md
    test -f desktop/src-tauri/tests/data_dir_override.rs
    grep -q 'override_relocates_history_settings_models_and_recovery' desktop/src-tauri/tests/data_dir_override.rs
    grep -q 'smoke_mode_refuses_to_start_against_the_default_root' desktop/src-tauri/tests/data_dir_override.rs
    grep -q 'smoke_mode_registers_no_global_hotkey_and_never_pastes' desktop/src-tauri/tests/data_dir_override.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test data_dir_override ; test $? -eq 0
    # non-vacuous: break the override and the suite must go red
    sed -i.bak 's/YAP_DATA_DIR/YAP_DATA_DIR_DISABLED/' src/lib.rs
    cargo test --features custom-protocol --test data_dir_override ; test $? -ne 0
    mv src/lib.rs.bak src/lib.rs
    git diff --exit-code src/lib.rs ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y0-E', prompt: 'Y0', branch: 'loop/y0-e-structural-windowed-smoke-before-the-ui-lane', gated: null,
  title: 'The instrument that can see "looks broken" ships BEFORE the nine UI items it judges — and it fails on today\'s tree',
  preflight: `
    test -x scripts/smoke-windowed.sh
    grep -q 'data-empty-state' scripts/smoke-windowed.sh
    node -e "process.exit(require('./desktop/package.json').scripts['smoke:windowed']?0:1)"
  `,
  spec: `
    PANEL 2026-09-12 — four seats converged on this. Wilson's defect (c) is a
    VISUAL report, the merge gate is eight headless commands (tsc, vitest, vite,
    two cargo builds, two cargo test runs) and build mode dispatches NO
    reviewer. Y7-B — the only pixel-level instrument in the plan — declares
    "Depends on Y5-B, Y5-C, Y5-G" and sits ten items after the whole Y5 lane,
    so nine UI items would land with no instrument that can see them, and
    Y5-G's own spec cites a screenshot gate that does not exist yet.

    So Y7-B is SPLIT. This item is the harness half and it runs here, in Y0:
      * \`scripts/smoke-windowed.sh\` (executable, \`set -euo pipefail\`), which
        launches the built app under \`YAP_DATA_DIR="$(mktemp -d)" --smoke\`
        (Y0-D), walks the seven Nav views at 980x700 and at 720x520, captures a
        PNG per view per size into docs/smoke-shots/<run>/, and FAILS on any of
        these five STRUCTURAL conditions:
          1. a view whose content region renders no text at all;
          2. a spinner still present after 10 s;
          3. a horizontal scrollbar on the document;
          4. any element's box outside the window bounds;
          5. a view rendering zero rows AND carrying no \`data-empty-state\`.
      * It must fail on TODAY's tree — that failure IS the baseline, and the PR
        body records which views fail and why. Do not weaken a detector to make
        the item green; record the red.
      * \`--check-only\` re-asserts against the last captured run. It is a
        convenience for a pre-flight, never a substitute for a capture, and it
        must exit NON-ZERO when no run exists. Add \`--self-test-must-fail\`,
        which injects a synthetic broken view and requires the detector to
        report it — the detector's own non-vacuity proof.
      * \`npm run smoke:windowed\` in desktop/package.json, and Y0-B's
        loop-smoke.sh calls it when a display is available. On this run a
        display IS available (it runs on Wilson's Mac), so "no display" is a
        FAILURE here, not a named skip.

    Do NOT assert pixel equality against a golden image. The failure conditions
    are structural, so a legitimate restyle never has to relitigate a PNG.

    Y7-B keeps the second half: extending coverage to the new pill phases and
    dock positions once Y5 has landed them.
  `,
  acceptance: `
    test -x scripts/smoke-windowed.sh
    grep -q 'set -euo pipefail' scripts/smoke-windowed.sh
    grep -q 'YAP_DATA_DIR' scripts/smoke-windowed.sh
    grep -q '720' scripts/smoke-windowed.sh
    grep -q '980' scripts/smoke-windowed.sh
    grep -q 'data-empty-state' scripts/smoke-windowed.sh
    grep -q 'self-test-must-fail' scripts/smoke-windowed.sh
    test 0 -eq "$(grep -c '| *tail' scripts/smoke-windowed.sh)"
    node -e "process.exit(require('./desktop/package.json').scripts['smoke:windowed']?0:1)"
    grep -q 'smoke-windowed' scripts/loop-smoke.sh
    test -f docs/loop/SMOKE-BASELINE.md
    ./scripts/smoke-windowed.sh --check-only ; test $? -ne 0
    ./scripts/smoke-windowed.sh --self-test-must-fail ; test $? -ne 0
  `,
})

ITEMS.push({
  id: 'Y0-B', prompt: 'Y0', branch: 'loop/y0-b-loop-smoke-and-fresh-clone-proof', gated: null,
  title: 'One command proves a fresh clone builds, tests and stages both sidecars — no host-found dependencies',
  preflight: `
    test -x scripts/loop-smoke.sh
    ./scripts/loop-smoke.sh
  `,
  spec: `
    Every item in this loop is built in a FRESH CLONE. The loop therefore needs
    one script that answers "does this tree build from nothing?" and that fails
    loudly when the answer is no. There is none today: \`ls scripts/\` at
    4e8c9adf is cicd-loop.mjs, rebuild_app.sh, start.sh,
    assert-weak-linked-14_4-symbols.sh.

    Create \`scripts/loop-smoke.sh\` (executable, \`set -euo pipefail\`), run from
    the repo root, which does exactly the SHARED PREAMBLE's standard gate and
    nothing more:
      npm ci in desktop · npm run build · npm test ·
      stage both sidecars for the host triple ·
      cargo test --features custom-protocol ·
      cargo clippy -- -D warnings · cargo fmt --check ·
      ./scripts/assert-weak-linked-14_4-symbols.sh desktop/target/release/wilson-voice
    It prints one PASS/FAIL line per stage and exits non-zero on the first
    failure. It must read every exit code directly — no pipes to tail/grep.

    RUNTIME-DEPENDENCY DISCIPLINE (the Yap rule, and the reason this item is
    early): nothing may be "found on the machine". The script must FAIL with a
    named message if any of these is missing rather than silently degrading:
    rustc/cargo, node, npm. And it must assert that the app's own runtime assets
    are shipped-or-managed, not assumed:
      * yap-polish and yap-diarize are BUILT here, from this tree (they are).
      * sherpa-onnx's build script FETCHES a 19.5 MB archive from GitHub
        Releases when SHERPA_ONNX_LIB_DIR is unset (ci.yml documents this at
        length). The script must print which of the three modes it used
        (SHERPA_ONNX_LIB_DIR / SHERPA_ONNX_ARCHIVE_DIR / network fetch) so an
        offline clone failure is diagnosable in one line instead of an hour.
      * ASR and polish MODEL WEIGHTS are downloaded by the app at runtime from
        the catalog (src-tauri/src/catalog.json). The script asserts no test
        requires a model on disk: \`cargo test\` must pass with an empty
        application-support dir. State that in the script's header.

    Add an npm script \`"smoke": "../scripts/loop-smoke.sh"\` in desktop/package.json.

    What NOT to do:
      - Do NOT make the script install anything (no brew, no rustup). It DETECTS
        and REPORTS. Installing behind the user's back is how a green smoke test
        stops describing the user's machine.
      - Do NOT have it download models to make tests pass. If a test needs a
        model, that test is wrong and belongs to Y7.
  `,
  acceptance: `
    test -x scripts/loop-smoke.sh
    grep -q 'set -euo pipefail' scripts/loop-smoke.sh
    grep -q 'SHERPA_ONNX_ARCHIVE_DIR' scripts/loop-smoke.sh
    test 0 -eq "$(grep -c '| *tail' scripts/loop-smoke.sh)"
    node -e "process.exit(require('./desktop/package.json').scripts.smoke?0:1)"
    ./scripts/loop-smoke.sh                      ; test $? -eq 0
    # non-vacuous: break it on purpose and watch it go red
    (cd desktop/src-tauri && printf '\\nfn __y0b(){let _x:u8=1;}\\n' >> src/latency.rs)
    ./scripts/loop-smoke.sh                      ; test $? -ne 0
    (cd desktop/src-tauri && git checkout src/latency.rs)    # PANEL: bundle.externalBin names BOTH sidecars, so both are a precondition
    # of every cargo build of the app. HARNESS.md's gate table stages only
    # yap-polish; on a fresh worktree that makes clippy and cargo test exit 101.
    test -f "desktop/src-tauri/binaries/yap-polish-$(rustc -vV | awk '/^host:/ {print $2}')"
    test -f "desktop/src-tauri/binaries/yap-diarize-$(rustc -vV | awk '/^host:/ {print $2}')"

  `,
})

ITEMS.push({
  id: 'Y0-C', prompt: 'Y0', branch: 'loop/y0-c-defaults-are-tested-not-just-code-paths', gated: null,
  title: 'A test asserts the SHIPPED defaults, so a feature that is off by default can never be called tested again',
  preflight: `
    test -f desktop/src-tauri/tests/shipped_defaults.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test shipped_defaults
  `,
  spec: `
    This item exists because of the single worst class of defect this audit
    found, and it is the mechanism behind Wilson's "formatting does not work
    whatsoever":

      desktop/src-tauri/src/lib.rs:417  cleanup_level: "light".into()
      desktop/src-tauri/src/dictation.rs:636  runs_format() => Medium | High
      desktop/src-tauri/src/lib.rs:418  polish_model: String::new()   // = OFF
      desktop/src-tauri/src/dictation.rs:644  runs_llm()   => High only

    So on a fresh install NO formatting stage runs at all — and the entire
    formatting corpus passes, because every fixture pins its own level:
      desktop/src-tauri/tests/fixtures/formatting/group-a-sentence-shape.jsonl
      line 4: {"level":"medium", ...}
    The tests exercise a configuration the product never ships.

    Create \`desktop/src-tauri/tests/shipped_defaults.rs\`: a table of the
    DEFAULT AppSettings values that user-visible behaviour depends on, each
    asserted against \`AppSettings::default()\`, each with a comment saying what
    the user sees if it changes. At minimum: cleanup_level, polish_model,
    auto_paste, show_floating_pill, pill_style, ptt_binding, dictation_mode,
    denoise, mute_while_dictating, check_updates, preload_model, signature_mode,
    snippet_scope.

    Then add the harder assertion, the one that has teeth:
      \`defaults_reach_every_cleanup_stage\` — build the cleanup pipeline from
      \`AppSettings::default()\` (NOT from a hand-written CleanupLevel) and assert
      each stage the product promises is REACHED. It must fail on today's main.
      Do not fix the default here — Y4-A owns that change. This test is the
      tripwire, and it is allowed to be \`#[ignore]\`d with an \`// UNBLOCKED BY
      Y4-A\` comment ONLY if the ignore is removed in Y4-A's diff. Prefer landing
      it red-then-green inside Y4-A's dependency order if the loop allows it.

    Also add, in the same file, \`formatting_fixtures_declare_the_shipped_level\`:
    assert that at least one fixture row in
    tests/fixtures/formatting/*.jsonl carries the level that
    \`AppSettings::default().cleanup_level\` names.

    PANEL 2026-09-12 — that assertion as first drafted is VACUOUS IN BOTH
    DIRECTIONS and the audit sentence under it is false. MEASURED on main: four
    rows already carry "light" (three in group-a, one in group-b, one of them
    literally named \`b17-level-light-runs-no-formatting\`) and 36 carry
    "medium". So it is green today and green after Y4-A flips the default. It
    can never go red. Build the assertion that has teeth instead:
    \`every_rule_has_a_fixture_row_at_the_shipped_level\` — enumerate the rule
    ids the formatting stage owns (the \`rules\` field already exists on every
    fixture row; take the id set from the rule table, never from a literal) and
    assert each one has at least one row whose \`level\` equals
    \`AppSettings::default().cleanup_level\`, failing by NAMING the missing rule
    ids. That is red on main, still red after a partial Y4-A, green only when
    the corpus covers the level the product ships.

    What NOT to do:
      - Do NOT assert defaults by re-reading the literal out of lib.rs with a
        grep. The test must construct \`AppSettings::default()\` so a changed
        default breaks it.
      - Do NOT change any default in this item. This item only makes them visible.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/shipped_defaults.rs
    grep -q 'AppSettings::default()' desktop/src-tauri/tests/shipped_defaults.rs
    grep -q 'defaults_reach_every_cleanup_stage' desktop/src-tauri/tests/shipped_defaults.rs
    grep -q 'every_rule_has_a_fixture_row_at_the_shipped_level' desktop/src-tauri/tests/shipped_defaults.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test shipped_defaults ; test $? -eq 0
    # non-vacuous: flip a default and the table must go red
    sed -i.bak 's/auto_paste: true/auto_paste: false/' src/lib.rs
    cargo test --features custom-protocol --test shipped_defaults ; test $? -ne 0
    mv src/lib.rs.bak src/lib.rs
    # PANEL: a failed restore must be LOUD, not a silently shipped default change
    git diff --exit-code src/lib.rs ; test $? -eq 0
  `,
})
