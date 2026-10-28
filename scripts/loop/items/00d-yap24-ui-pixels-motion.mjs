// yap24-UI — CLEARER PIXELS, SMOOTHER MOTION. Written 2026-09-26 by the architecture audit
// (docs/ARCHITECTURE-AUDIT-2026-09-26.md §5). Wilson, 2026-09-26: "make the UI better, the pixels
// clearer, smoother." Locked aesthetic: pixel-art Tamagotchi on an LCD/pod (memory
// feedback_companion_must_be_cute) — no vector mascot, no origami, no angry eyebrows. Characters
// are data behind the shell (owner decision 2026-09-13, Y5-K).
// Headless verification only: vitest + the headless-Chrome structural smoke (scripts/smoke-windowed.mjs
// runs Chrome with --headless=new). SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap24-UI1', prompt: 'yap24-UI', branch: 'loop/yap24-ui1-integer-pixel-grid-on-every-display', gated: 'panel',
  title: 'Every sprite pixel lands on whole device pixels at every dock size and backing scale — no half-pixel blur',
  preflight: `
    test -f desktop/src/pill/pixelGrid.ts
    test -f desktop/src/pill/pixelGrid.test.ts
  `,
  spec: `
    Panel: pending
    DEPENDS: Y5-I (vertical-as-base geometry) and Y5-K (character shell) when merged; standalone otherwise

    EVIDENCE
      - pill/YappyPill.tsx:262 DPR = Math.min(devicePixelRatio || 1, 2) and the canvas is sized
        from the CSS box; nothing guarantees the art's cell size is an integer number of device
        pixels, so at some dock sizes a 1-art-pixel line straddles two device pixels (soft edges).
      - float.css:379 and App.css:1969-1975 set image-rendering: pixelated / crisp-edges on the
        upscaled canvases — correct, but it cannot fix a non-integer scale.

    DO: a pure pixelGrid.ts — given CSS box, art grid (w,h) and DPR, return the largest integer
    device-pixel cell, the canvas backing size and the centring offset in whole device pixels.
    YappyPill, ClassicPill (where it draws pixel art) and YappyHouse use it; the leftover margin is
    filled with the scene colour, never stretched. Tests cover DPR 1 and 2, all three docks, the
    30 px side dock.
  `,
  acceptance: `
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/pill/pixelGrid.test.ts
    npm test
    npm run build
  `,
})

ITEMS.push({
  id: 'yap24-UI2', prompt: 'yap24-UI', branch: 'loop/yap24-ui2-one-animation-clock-that-parks', gated: 'panel',
  title: 'One animation clock for the pill and the habitat: frame-rate independent, parks at idle, honours Reduce Motion',
  preflight: `
    test -f desktop/src/pill/clock.ts
    test -f desktop/src/pill/clock.test.ts
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26, DEFERRED to the next release (not in panelApproved).
    DEPENDS: Y5-D (open PR #196 — spring constants) when merged

    EVIDENCE
      - ClassicPill was fixed to settle-and-park (memory: yap audit [0] "60fps rAF-forever"); the
        Yappy pill and YappyHouse each run their own loops (YappyPill.tsx canvas loop,
        home/YappyHouse.tsx) — two clocks, two idle policies.
      - ProMotion displays run rAF at 120 Hz; animation that advances per frame instead of per
        millisecond runs twice as fast there (UNVERIFIED for each loop — the builder measures).

    DO: pill/clock.ts — a single requestAnimationFrame scheduler with dt in ms, subscribers,
    automatic park when no subscriber is animating, and a prefers-reduced-motion switch. Port the
    three loops onto it; every motion is expressed per millisecond. Tests with a fake rAF: equal
    motion at 60 and 120 Hz; parks after settle; reduced motion snaps.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - Y5-D (#196, the spring constants) is MERGED (5f3c852), not open. The animation loops are in FIVE files, not three: ClassicPill, YappyPill,
        home/YappyHouse, license/YappySprite.tsx and pill/live.ts. The port covers all five. DEFER: next release (idle-CPU hygiene, no user-visible wow).
  `,
  acceptance: `
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/pill/clock.test.ts
    npm test
    npm run build
  `,
})

ITEMS.push({
  id: 'yap24-UI3', prompt: 'yap24-UI', branch: 'loop/yap24-ui3-pay-the-visual-qa-debt-headless', gated: 'panel',
  title: 'Pay the visual-QA debt: a headless screenshot matrix of every pill phase x dock x character, attached to the PR',
  preflight: `
    test -f scripts/pill-matrix.mjs
    grep -q -- '--only' scripts/pill-matrix.mjs
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26; BUILD THIS FIRST (every PILL item and NT6/NT7 run its phase through this matrix).
    DEPENDS: Y5-B/Y5-C/Y5-D (merged). [REVERSED 2026-10-26: the PILL items depend on THIS item, not the other way round; the matrix reads its phase list from the pill's own phaseVisual keys, so phases the PILL items add appear automatically.]

    EVIDENCE: the 2026-09-15 run's builders logged "NOT visually QA'd" on Y2-B, Y5-B, Y5-C, Y5-D
    (memory project_yap_loop_state_20260912; STATUS-yap.md). A green gate cannot see a pill.

    DO: scripts/pill-matrix.mjs reuses smoke-windowed.mjs's headless Chrome (--headless=new) and
    vite preview, renders float.html with each phase forced through a dev-only query parameter,
    captures PNGs for every phase x {bottom,left,right} x {classic,yappy}, and fails on the same
    structural conditions (empty text, clipped text, overflow). --out <dir> (a scratch dir; attach the PNGs to the PR body or a PR comment, NEVER commit them) and --only <phase> to render one phase across 3 docks x 2 characters. Never launches the app, never opens a visible window.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - The old acceptance only ran node --check, so the matrix this item exists to produce was never produced; a green gate could not see a pill.
        The acceptance now RUNS it and counts the PNGs (at least 24 = phases x 3 docks x 2 characters). The PNGs go to the PR, not the repo. Each PILL
        item then runs scripts/pill-matrix.mjs --only <its phase>.
  `,
  acceptance: `
    test -f scripts/pill-matrix.mjs
    node --check scripts/pill-matrix.mjs
    grep -q -- '--only' scripts/pill-matrix.mjs
    test -z "$(git ls-files docs/pr-screenshots/yap24-UI3)"
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd .. && O="$(mktemp -d)" && node scripts/pill-matrix.mjs --out "$O" && test "$(ls "$O"/*.png | wc -l)" -ge 24
  `,
})
