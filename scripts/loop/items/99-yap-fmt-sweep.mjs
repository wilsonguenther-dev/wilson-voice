// yap-fmt-sweep-2026-10-26 — written 2026-10-26 by the pre-launch fix seat after review pass w7sv7tozp was
// stopped. Named 99 so it sorts LAST and builds last: a tree-wide `cargo fmt --all` conflicts every open
// branch that edits the large Rust files, so it must run only after every other open PR has landed.
// Panel: not required (trivial, mechanical, format-only). Class STANDARD.
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap-fmt-sweep-2026-10-26', prompt: 'yap-fmt-sweep-2026-10-26', branch: 'loop/yap-fmt-sweep-2026-10-26', gated: null,
  title: 'Format-only sweep: `cargo fmt --all` on main, zero semantic change; then rustfmt goes back to BLOCKING in the land gate, loop-smoke and CI',
  preflight: `
    (cd desktop/src-tauri && cargo fmt --all -- --check) && ! grep -q 'fmt IS ADVISORY' scripts/loop/template.mjs
  `,
  spec: `
    Panel: not required — trivial, mechanical. Class STANDARD. deps: none.
    ORDER: build only after every other open PR has landed; the loop runs it last by file order (99-). If any
    loop-build PR is still OPEN when you reach this item, stop and return status "needs-human" with the open
    PR numbers — a sweep under open branches conflicts all of them.

    WHY (2026-10-26): the b4b8d06 sweep (152 hunks / 21 files) drifted; on a740614 \`cargo fmt --all -- --check\`
    exits 1 with 65 hunks in src-tauri/src/{db,hotkey_table,hotkeys,lib,perm_truth,ptt_macos,record}.rs and
    tests/{db_migration_idempotent,hotkey_no_tcc,matrix_row1_tap_permission_denied}.rs. Requiring it in the
    LAND gate turned every PASS/PASS PR ci-red (review pass w7sv7tozp), so the gate made fmt ADVISORY per the
    Y0-A ledger (PR #234 ci.yml). This item restores it.

    DO
      1. Commit 1, format only: cd desktop/src-tauri && cargo fmt --all. Prove zero semantic change:
         \`cargo fmt --all -- --check\` exits 0; \`git diff --stat\` touches only .rs files; \`git diff -w
         --ignore-blank-lines\` is empty or only re-wrapped lines (paste the stat); cargo build and clippy
         produce the same result as on the base sha (paste both exit codes, before and after); the full gate's
         tests pass. Append the commit sha to .git-blame-ignore-revs in commit 2.
      2. Commit 2, flip fmt back to blocking, all three places, same PR:
         - scripts/loop/template.mjs GATE_CMDS: fmt rejoins the gate ("ALL ELEVEN MUST EXIT 0"), the
           ADVISORY wording, the fmtHunks line, the "fmt: advisory" gate-comment line and the "(fmt is
           advisory and excluded)" / "a non-zero fmt ALONE is NEVER ci-red" land-outcome clauses come out.
         - scripts/loop-smoke.sh stage_fmt: blocking by default again (drop YAP_SMOKE_FMT_STRICT or default it to 1).
         - .github/workflows/ci.yml: the rustfmt step loses \`continue-on-error\` (the Y0-A comment says this
           sweep is the trigger).
         - scripts/loop/land-gate-fmt.test.mjs: invert it to assert fmt is BLOCKING (keep the clippy assertions).
         Then npm run loop:build && npm run loop:validate in desktop/, both exit 0, and commit the regenerated parts.
    WHAT NOT TO DO: no code change of any kind in commit 1; no rustfmt.toml change; no #[rustfmt::skip].
  `,
  acceptance: `
    (cd desktop/src-tauri && cargo fmt --all -- --check)
    ! grep -q 'fmt IS ADVISORY' scripts/loop/template.mjs
    grep -q 'ALL ELEVEN MUST EXIT 0' scripts/loop/template.mjs
    ! sed -n '/rustfmt/,/run: cargo fmt/p' .github/workflows/ci.yml | grep -q 'continue-on-error: true'
    node --test scripts/loop/land-gate-fmt.test.mjs
    (cd desktop && npm run loop:validate)
    (cd desktop && npm ci && npx tsc --noEmit && npm test)
  `,
})
