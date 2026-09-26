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
    Panel: pending
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
  `,
  spec: `
    Panel: pending
    DEPENDS: yap24-PILL1..PILL4, yap24-UI1, Y5-B/Y5-C/Y5-D

    EVIDENCE: the 2026-09-15 run's builders logged "NOT visually QA'd" on Y2-B, Y5-B, Y5-C, Y5-D
    (memory project_yap_loop_state_20260912; STATUS-yap.md). A green gate cannot see a pill.

    DO: scripts/pill-matrix.mjs reuses smoke-windowed.mjs's headless Chrome (--headless=new) and
    vite preview, renders float.html with each phase forced through a dev-only query parameter,
    captures PNGs for every phase x {bottom,left,right} x {classic,yappy}, and fails on the same
    structural conditions (empty text, clipped text, overflow). Output under
    docs/pr-screenshots/yap24-UI3/. Never launches the app, never opens a visible window.
  `,
  acceptance: `
    test -f scripts/pill-matrix.mjs
    node --check scripts/pill-matrix.mjs
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
  `,
})
