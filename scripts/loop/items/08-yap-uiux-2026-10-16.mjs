// yap-uiux-2026-10-16 — THE UI/UX PASS. Written 2026-10-16 by the yap-prep seat.
// Wilson, 2026-10-06: "All of the Yap stuff: the UI/UX ... There are many items you'll find and
// improve on." Memory: feedback_ui_containment (every panel and shell ships contained),
// feedback_builder_e2e_product_walk ("working" = the full flow as the persona).
// Shape: W0 walks main's build as two personas and writes the ledger; one child per screen fixes
// that screen's rows; Z re-walks after the flush and IS the acceptance of the pass.
// Locked: the pixel-art pill stays (no vector mascot, no origami); NO NEW FEATURES; controls
// first, prose last; contained panels (surface + border, pinned action footer, no infinite scroll,
// no page-level horizontal scroll). Headless only: scripts/smoke-windowed.sh drives headless
// Chrome (--headless=new) against desktop/dist; it never launches the app and never opens a
// visible window. Lane port: export YAP_SMOKE_PORT to your lane's vite port (A 5273, B 5274).
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap-uiux-2026-10-16-W0', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-w0-first-run-and-daily-walk', gated: null,
  title: 'Walk the main build as a first-run user and a daily user, headless, and write down every screen that reads generic, uncontained, confusing or slow',
  preflight: `
    test -f docs/loop/UIUX-WALK-2026-10-16.md
    test -f scripts/smoke-fixtures-first-run.json
    test -f scripts/smoke-fixtures-daily.json
    grep -q 'YAP_SMOKE_FIXTURES' scripts/smoke-windowed.mjs
    grep -q 'YAP_SMOKE_VIEWS' scripts/smoke-windowed.mjs
  `,
  spec: `
    Panel: not required — a walk that writes a ledger and changes no view
    DEPENDS: yap-flakes-2026-10-16 (a green main); Y0-E's smoke (scripts/smoke-windowed.sh, on main)
      QA TOOLS (yap-qa-harness, which sorts ahead of this file): also run bash scripts/qa/perm-probe.sh and
      bash scripts/qa/hotkey-walk.sh --self-test and paste both outputs into the ledger header and the PR
      body. The full hotkey-walk (it posts fn into the installed Yap) is Wilson's walk step, never an agent's.

    EVIDENCE
      - scripts/smoke-windowed.sh + scripts/smoke-windowed.mjs walk the seven Nav views at 980x700
        and 720x520 in headless Chrome against desktop/dist with a shimmed __TAURI_INTERNALS__,
        and read every answer from ONE file, scripts/smoke-fixtures.json (smoke-windowed.mjs:74).
        One fixture set cannot show both the empty first run and a lived-in day.
      - The 2026-09-15 builders logged "NOT visually QA'd" on Y2-B, Y5-B, Y5-C and Y5-D.
      - Screens: desktop/src/views/Home.tsx (+ src/home/), Dictionary.tsx, Insights.tsx,
        Meetings.tsx (+ src/meetings/), Permissions.tsx, Scratchpad.tsx, Settings.tsx +
        views/settings/{Advanced,Audio,Companion,Dictation,License,Privacy,Shortcut,Snippets}.tsx,
        src/Onboarding.tsx, src/ModelSetup.tsx, src/PermissionHealthRow.tsx.

    DO
      1. smoke-windowed.mjs reads YAP_SMOKE_FIXTURES (a path; default scripts/smoke-fixtures.json,
         so behaviour is unchanged when it is unset) and YAP_SMOKE_VIEWS (a comma list of Nav
         labels, case-insensitive; unset = all views). It also captures the first-run overlay
         (Onboarding / ModelSetup) as the pseudo-view "Onboarding" when the fixture set shows it.
      2. Two fixture sets, only shapes already certain (the _README rule in smoke-fixtures.json):
         scripts/smoke-fixtures-first-run.json (no history, no meetings, permissions not granted,
         no model installed, trial day 14) and scripts/smoke-fixtures-daily.json (40 dictations,
         3 meetings with one still transcribing, every permission granted, trial day 5). Every
         string is invented and bland: never real dictation text (RESUME §11.10).
      3. Walk both personas with YAP_SMOKE_SHOTS pointing at
         ~/Obsidian/Wilson-Brain/Projects/Loop-Logs/yap-uiux-2026-10-16/first-run/ and .../daily/.
         PNGs stay out of git.
      4. Write docs/loop/UIUX-WALK-2026-10-16.md: a header, then one table row per finding, each
         row starting with "| <Screen> |" where Screen is one of Home, Dictionary, Insights,
         Meetings, Scratchpad, Settings, Onboarding:
         | Screen | view file | persona | verdict | evidence (PNG path + what it shows) | fix | child id | status |
         verdict is one or more of generic / uncontained / confusing / slow; fix is named and
         scoped to the view file; status is open, fixed or none (a screen with no finding gets
         one "none" row). Judge by: controls first, prose last; every panel contained; empty
         states say what to do next; nothing spins past 10 s; the pill is out of scope here (it
         stays pixel-art; yap24-PILL owns its states). A finding that needs a new capability goes
         to an "Out of scope (new feature)" list at the bottom, never into a row.
      5. This item changes no view. It ships the env vars, the fixture sets, the ledger and the
         baseline structural findings for both personas.
  `,
  acceptance: `
    test -f docs/loop/UIUX-WALK-2026-10-16.md
    test -f scripts/smoke-fixtures-first-run.json
    test -f scripts/smoke-fixtures-daily.json
    node -e "for (const f of ['scripts/smoke-fixtures-first-run.json','scripts/smoke-fixtures-daily.json']) JSON.parse(require('fs').readFileSync(f,'utf8'))"
    grep -q 'YAP_SMOKE_FIXTURES' scripts/smoke-windowed.mjs
    grep -q 'YAP_SMOKE_VIEWS' scripts/smoke-windowed.mjs
    for s in Home Dictionary Insights Meetings Scratchpad Settings Onboarding; do grep -q "^| $s |" docs/loop/UIUX-WALK-2026-10-16.md || exit 1; done
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
  `,
})

ITEMS.push({
  id: 'yap-uiux-2026-10-16-HOME', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-home', gated: null,
  title: 'Home reads contained and obvious for both personas: every walk row on Home fixed',
  preflight: `
    grep -q '^| Home |' docs/loop/UIUX-WALK-2026-10-16.md
    ! grep '^| Home |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
  `,
  spec: `
    Panel: not required — fixes the W0 ledger names, no new features
    DEPENDS: yap-uiux-2026-10-16-W0
    FILES: desktop/src/views/Home.tsx, desktop/src/home/ (YappyHouse stays pixel-art)
    DO: read every "| Home |" row in docs/loop/UIUX-WALK-2026-10-16.md and fix each in the named
      file only. Controls first, prose last; contained surfaces; the empty state says what to do
      next. Never show a real transcript list in any capture. Re-walk Home for both fixture sets,
      put the after PNGs in the Obsidian walk dir under after/home/, attach one before/after pair
      to the PR, and flip the rows to fixed with the PR number.
  `,
  acceptance: `
    ! grep '^| Home |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Home YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-first-run.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
    YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Home YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-daily.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
  `,
})

ITEMS.push({
  id: 'yap-uiux-2026-10-16-DICT', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-dictionary', gated: null,
  title: 'Dictionary reads contained and obvious for both personas: every walk row on Dictionary fixed',
  preflight: `
    grep -q '^| Dictionary |' docs/loop/UIUX-WALK-2026-10-16.md
    ! grep '^| Dictionary |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
  `,
  spec: `
    Panel: not required — fixes the W0 ledger names, no new features
    DEPENDS: yap-uiux-2026-10-16-W0
    FILES: desktop/src/views/Dictionary.tsx
    DO: read every "| Dictionary |" row in docs/loop/UIUX-WALK-2026-10-16.md and fix each in the
      named file only. Controls first (add word, search), prose last; a contained list with a
      pinned action footer, no infinite scroll. Re-walk Dictionary for both fixture sets, after
      PNGs to the Obsidian walk dir under after/dictionary/, one before/after pair on the PR, rows
      flipped to fixed with the PR number.
  `,
  acceptance: `
    ! grep '^| Dictionary |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Dictionary YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-first-run.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
    YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Dictionary YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-daily.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
  `,
})

ITEMS.push({
  id: 'yap-uiux-2026-10-16-INSIGHTS', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-insights', gated: null,
  title: 'Insights reads contained and obvious for both personas: every walk row on Insights fixed',
  preflight: `
    grep -q '^| Insights |' docs/loop/UIUX-WALK-2026-10-16.md
    ! grep '^| Insights |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
  `,
  spec: `
    Panel: not required — fixes the W0 ledger names, no new features
    DEPENDS: yap-uiux-2026-10-16-W0
    FILES: desktop/src/views/Insights.tsx
    DO: read every "| Insights |" row in docs/loop/UIUX-WALK-2026-10-16.md and fix each in the
      named file only. The first-run persona has no data: the empty state says what produces
      insights instead of a zero-filled chart. Re-walk Insights for both fixture sets, after PNGs
      to the Obsidian walk dir under after/insights/, one before/after pair on the PR, rows flipped
      to fixed with the PR number.
  `,
  acceptance: `
    ! grep '^| Insights |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Insights YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-first-run.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
    YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Insights YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-daily.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
  `,
})

ITEMS.push({
  id: 'yap-uiux-2026-10-16-MEETINGS', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-meetings', gated: null,
  title: 'Meetings reads contained and obvious for both personas: every walk row on Meetings fixed',
  preflight: `
    grep -q '^| Meetings |' docs/loop/UIUX-WALK-2026-10-16.md
    ! grep '^| Meetings |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
  `,
  spec: `
    Panel: not required — fixes the W0 ledger names, no new features
    DEPENDS: yap-uiux-2026-10-16-W0; coordinate with yap24-NT3/NT4 if they are open (same files):
      rebase onto whichever merged first and keep both
    FILES: desktop/src/views/Meetings.tsx, desktop/src/meetings/
    DO: read every "| Meetings |" row in docs/loop/UIUX-WALK-2026-10-16.md and fix each in the
      named files only. Record/stop is the first control; a transcribing meeting shows progress,
      never a bare spinner past 10 s; a needs-model state offers the one-click install, never an
      error. Re-walk Meetings for both fixture sets, after PNGs to the Obsidian walk dir under
      after/meetings/, one before/after pair on the PR, rows flipped to fixed with the PR number.
  `,
  acceptance: `
    ! grep '^| Meetings |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Meetings YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-first-run.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
    YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Meetings YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-daily.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
  `,
})

ITEMS.push({
  id: 'yap-uiux-2026-10-16-SCRATCH', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-scratchpad', gated: null,
  title: 'Scratchpad reads contained and obvious for both personas: every walk row on Scratchpad fixed',
  preflight: `
    grep -q '^| Scratchpad |' docs/loop/UIUX-WALK-2026-10-16.md
    ! grep '^| Scratchpad |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
  `,
  spec: `
    Panel: not required — fixes the W0 ledger names, no new features
    DEPENDS: yap-uiux-2026-10-16-W0
    FILES: desktop/src/views/Scratchpad.tsx
    DO: read every "| Scratchpad |" row in docs/loop/UIUX-WALK-2026-10-16.md and fix each in the
      named file only. Controls first, prose last; the editor surface is contained and its actions
      sit in a pinned footer. Re-walk Scratchpad for both fixture sets, after PNGs to the Obsidian
      walk dir under after/scratchpad/, one before/after pair on the PR, rows flipped to fixed with
      the PR number.
  `,
  acceptance: `
    ! grep '^| Scratchpad |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Scratchpad YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-first-run.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
    YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Scratchpad YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-daily.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
  `,
})

ITEMS.push({
  id: 'yap-uiux-2026-10-16-SETTINGS', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-settings', gated: null,
  title: 'Settings is contained: bordered surfaces, a pinned footer, every walk row on Settings and its eight panes fixed',
  preflight: `
    grep -q '^| Settings |' docs/loop/UIUX-WALK-2026-10-16.md
    ! grep '^| Settings |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
  `,
  spec: `
    Panel: not required — fixes the W0 ledger names, no new features (10-06 new item "settings
      containment (borders/surfaces/pinned footer)")
    DEPENDS: yap-uiux-2026-10-16-W0; follow the Y5-A token layer if #190 has merged, otherwise use
      the existing App.css variables and add none
    FILES: desktop/src/views/Settings.tsx, desktop/src/views/settings/*.tsx
    DO: read every "| Settings |" row in docs/loop/UIUX-WALK-2026-10-16.md and fix each in the
      named files only. Each pane is one contained surface; the toggle or picker comes first and
      its explanation follows in one line; save/reset actions sit in a pinned footer; nothing
      overflows at 720x520. Re-walk Settings for both fixture sets, after PNGs to the Obsidian walk
      dir under after/settings/, one before/after pair on the PR, rows flipped to fixed with the
      PR number.
  `,
  acceptance: `
    ! grep '^| Settings |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Settings YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-first-run.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
    YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Settings YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-daily.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
  `,
})

ITEMS.push({
  id: 'yap-uiux-2026-10-16-ONBOARD', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-onboarding-and-permissions', gated: null,
  title: 'First run is obvious: Onboarding, ModelSetup and Permissions say what to grant and why, one step at a time; every walk row fixed',
  preflight: `
    grep -q '^| Onboarding |' docs/loop/UIUX-WALK-2026-10-16.md
    ! grep '^| Onboarding |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
  `,
  spec: `
    Panel: not required — fixes the W0 ledger names, no new features (10-06 new item "first-run /
      permissions flow (mic, accessibility, input monitoring) with real screenshots"; the real
      screenshots are the headless walk's PNGs, never a visible app window)
    DEPENDS: yap-uiux-2026-10-16-W0; the Y1 permission items if open (same files)
    FILES: desktop/src/Onboarding.tsx, desktop/src/ModelSetup.tsx, desktop/src/views/Permissions.tsx,
      desktop/src/PermissionHealthRow.tsx
    DO: read every "| Onboarding |" row (and any "| Permissions |"-file finding W0 filed under
      Onboarding) in docs/loop/UIUX-WALK-2026-10-16.md and fix each in the named files only. One
      permission per step, the grant button first, one line of why after it; the model download
      shows size and progress; a denied grant says exactly where to fix it in System Settings.
      Never touch the bundle id (TCC grants). Re-walk with the first-run set (YAP_SMOKE_VIEWS
      Onboarding,Permissions), after PNGs to the Obsidian walk dir under after/onboarding/, one
      before/after pair on the PR, rows flipped to fixed with the PR number.
  `,
  acceptance: `
    ! grep '^| Onboarding |' docs/loop/UIUX-WALK-2026-10-16.md | grep -q '| open |'
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Onboarding,Permissions YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-first-run.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
    YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_VIEWS=Permissions YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-daily.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
  `,
})

ITEMS.push({
  id: 'yap-uiux-2026-10-16-Z', prompt: 'yap-uiux', branch: 'loop/yap-uiux-2026-10-16-z-rewalk-after-flush', gated: null,
  title: 'Re-walk after the flush: both personas, every screen, zero open rows, the full smoke green — the acceptance of the UI/UX pass',
  preflight: `
    test -f docs/loop/UIUX-REWALK-2026-10-16.md
    ! grep -q '| open |' docs/loop/UIUX-WALK-2026-10-16.md
  `,
  spec: `
    Panel: not required — verification of the pass
    DEPENDS: every yap-uiux-2026-10-16 child above merged (HOME, DICT, INSIGHTS, MEETINGS, SCRATCH,
      SETTINGS, ONBOARD)
      QA TOOLS (yap-qa-harness, which sorts ahead of this file): also run bash scripts/qa/perm-probe.sh and
      bash scripts/qa/hotkey-walk.sh --self-test and paste both outputs into the ledger header and the PR
      body. The full hotkey-walk (it posts fn into the installed Yap) is Wilson's walk step, never an agent's.
    DO: re-run the FULL walk (no YAP_SMOKE_VIEWS) for both fixture sets, PNGs to
      ~/Obsidian/Wilson-Brain/Projects/Loop-Logs/yap-uiux-2026-10-16/rewalk-first-run/ and
      .../rewalk-daily/. Write docs/loop/UIUX-REWALK-2026-10-16.md: per screen, the before and
      after verdict, the PR that fixed it, and the PNG paths. A NEW finding on the re-walk becomes a
      ledger row and is fixed here when it stays inside one view file; anything wider becomes a
      numbered entry in docs/loop/DEFERRED.md. The full smoke exiting 0 for both fixture sets is
      the acceptance of the whole UI/UX pass.
  `,
  acceptance: `
    bash scripts/qa/perm-probe.sh --self-test
    bash scripts/qa/hotkey-walk.sh --self-test
    test -f docs/loop/UIUX-REWALK-2026-10-16.md
    ! grep -q '| open |' docs/loop/UIUX-WALK-2026-10-16.md
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-first-run.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
    YAP_SMOKE_SKIP_BUILD=1 YAP_SMOKE_FIXTURES=scripts/smoke-fixtures-daily.json YAP_SMOKE_SHOTS="$(mktemp -d)" bash scripts/smoke-windowed.sh
  `,
})
