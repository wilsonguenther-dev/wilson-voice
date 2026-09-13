// Y5 — UI/UX. Wilson, 2026-09-12, verbatim: "the app looks broken, not smooth —
// lots of UI/UX problems." And, standing: "we got to really think about this
// thing end to end."
//
// AUDIT, structural, at 4e8c9adf:
//   desktop/src/App.tsx      3,439 lines  — ONE component holding seven views
//                            (Nav: home | permissions | meetings | insights |
//                            dictionary | scratchpad | settings, App.tsx:55-61)
//                            and eight settings sub-tabs (App.tsx:66-85).
//   desktop/src/App.css      2,789 lines  — one stylesheet, no token layer.
//   desktop/src/home/YappyHouse.tsx  919 lines
//   Empty / loading / error states:
//     git grep -c "empty-state\|EmptyState\|skeleton" -- desktop/src
//       -> App.css: 1, App.tsx: 1.  For SEVEN views. That is the "looks broken"
//          report: a view with no data renders a bare frame with no explanation
//          and no action.
//   The pill's whole state vocabulary is four values:
//     ClassicPill.tsx:23-24  {recording, busy, message} + a `done` flag
//     live.ts:252   LivePhase = idle | listening | thinking | done | sleepy
//   Wispr's Flow Bar has TWELVE states with exact geometry per dock
//   (reference_wispr_parity_research §4.2, [BUNDLE]): resting · ready ·
//   activePtt · activePopo · processing · polishProcessing · polishCompleted ·
//   autoCleanupCompleted · error · growthNudgeActive · navigationActive ·
//   postInstructBubble · instructCollapsing.
//   project_yap_pill_vision, Wilson's own words: "fill the dead time after
//   talking stops and before text appears (transcribe/think gap) and every
//   other micro-state — idle->listening->transcribing->polishing->pasting->
//   done->fold-back + errors/permission/model-loading/empty. Today only
//   listening/busy/done exist."
//
// MOTION CONSTANTS, primary-sourced, use these exact numbers
// (reference_wispr_parity_research §4.3 [BUNDLE]):
//   springs stiffness:600 damping:35 restDelta:0.05 (snappy morphs)
//   springs stiffness:300 damping:28 (the slower one)
//   cubic-bezier(0.05,0.6,0.4,0.95) @ 100ms for state changes
//   300-400ms for expand/collapse
//   hover hysteresis: an invisible ::before alpha margin at inset:-12px,
//   painted ONLY while expanded
//   rgba(0,0,0,0.004) background so the box is clickable while invisible
//
// AESTHETIC LOCK — not negotiable, do not re-litigate:
//   feedback_companion_must_be_cute: PIXEL ART on a little LCD screen/pod
//   (Tamagotchi / Bitzee). Chunky pixels, imageSmoothingEnabled=false, limited
//   retro palette. NOT smooth vector. NO angled "angry" eyebrows. Paper/origami
//   is REJECTED ("def a no on the paper").
//   feedback_no_generic_ui: reject AI-dashboard aesthetics.
//   feedback_ui_quality: truly native feel, no webview tells.
//   feedback_think_ux_first: controls first, prose last.
//
// ── OWNER DECISION 2026-09-13 (Wilson) — THE PILL IS A CHARACTER SYSTEM ──
//   Verbatim: "I thought we were gonna develop it and then make more characters
//   and make it more flexible ... there's a classic pill and there's a yappy
//   pill and there's gonna be different pills with the different creatures that
//   are coming."
//   So "which pill ships in v1" was the WRONG QUESTION and is closed: BOTH ship,
//   as the first two CHARACTERS of a pluggable system, and more creatures come
//   later. The panel's cost objection was real and is answered STRUCTURALLY,
//   not by picking one:
//     * THE SHELL owns everything that is not the creature — the window, the
//       dock, the geometry table, hover/hit-testing, motion, the phase state
//       machine, a11y names. Dock positions are handled ONCE, in the shell.
//     * A CHARACTER is a DATA-DRIVEN MODULE behind one interface: given a phase
//       and a tone it returns a sprite/animation and copy. It knows nothing
//       about docks, windows or license logic.
//     * TESTS RUN A FIXTURE MATRIX OVER THE REGISTERED CHARACTERS instead of
//       duplicating a code path per pill. Y5-C's "13 phases x 2 styles x 3 docks"
//       becomes 13 phases x 3 docks in the shell, plus one data completeness
//       sweep per registered character.
//     * A NEW CREATURE IS A NEW MODULE + A FIXTURE ROW. No shell change.
//   Y5-K builds that system and reinstates the living habitat (the killed Y5-H)
//   as its habitat layer. The aesthetic lock below is unchanged and binding.
//
// SHARED PREAMBLE + STANDARD GATE: see 00-y0-harness-and-gates.mjs.
// EVERY item here owes the two-size screenshots (980x700 and 720x520) plus the
// pill at three dock positions for each state it touches.

ITEMS.push({
  id: 'Y5-G', prompt: 'Y5', branch: 'loop/y5-g-split-app-tsx-into-views', gated: null,
  title: 'Split the 4,660-line App.tsx into seven view modules so a screen can be worked on at all',
  preflight: `
    test 900 -ge "$(wc -l < desktop/src/App.tsx)"
    test -d desktop/src/views
    cd ${APP} && npm ci && npm run build && npm test
  `,
  spec: `
    \`App.tsx\` is 4,660 lines holding seven views and eight settings sub-tabs
    (PANEL 2026-09-12: the audit's 3,439 was measured against an older tree and
    is 1,221 lines low — \`wc -l desktop/src/App.tsx\` at 4e8c9adf is 4,660, so
    this is a ~3,760-line move, not a ~2,500-line one).
    Every later UI item in this loop has to edit it, which makes them serially
    conflicting and makes each one hard to review. This is the enabling refactor.

    Do:
      * \`desktop/src/views/{Home,Permissions,Meetings,Insights,Dictionary,Scratchpad,Settings}.tsx\`,
        one per \`Nav\` value (App.tsx:55-61), and
        \`desktop/src/views/settings/\` for the eight \`SettingsTab\`s
        (App.tsx:66-85). App.tsx keeps the shell: nav, the license chip, the
        toast host, the event listeners.
      * PURE MECHANICAL MOVE. No behaviour change, no restyling, no renaming of
        a state field. The gate is that \`npm test\` and \`npm run build\` pass and
        Y0-E's structural smoke reports the same result before and after; a
        mixed refactor-plus-redesign diff
        is unreviewable and is how a regression ships.
      * Shared state that currently lives in one component body has to be lifted
        deliberately. Prefer props and a small number of explicit contexts over
        a global store; do not add a state-management dependency.
      * The event listeners (App.tsx:1190-1210 license, plus the take/status
        listeners) stay in ONE place in the shell. Seven views each subscribing
        to \`recording\` is seven listeners and a leak.

    Tests: existing suites must pass unchanged, and add
    \`desktop/src/views/views.test.tsx\` asserting each view module exports a
    default component and that no view module registers a Tauri \`listen\` —
    the sweep that keeps the listener discipline from eroding.

    PANEL 2026-09-12 — ORDER REVERSED. This item now runs FIRST in this file.
    Four seats converged: eleven items across both lanes edit App.tsx, this item
    empties it, and the build agent's LAND step is told to SKIP a conflicting PR
    rather than fix it. Doing the states first means Y5-B/Y5-F write markup into
    the monolith and then this item moves it again, with the cross-lane items
    (PERM-B, PERM-E, Y2-D, Y4-G, DB-D, Y10-E, Y7-D) all branched off the old
    shape. So: pure mechanical move FIRST, on the smallest possible diff, then
    every later UI item writes into desktop/src/views/<View>.tsx.
    Because it moves first, the gate is the existing suites plus Y0-E's
    structural windowed smoke (same assertions before and after) — NOT
    "the screenshots are pixel-identical", which named a golden-image gate that
    does not exist and that Y7-B explicitly forbids.
    Split the landing if the diff is unreviewable: views first, then settings/*,
    two PRs, identical gate on each.

    What NOT to do:
      - Do NOT restyle while moving.
      - Do NOT add Redux/Zustand/Jotai.
      - Do NOT leave a re-export shim that lets code keep importing views from
        App.tsx.
  `,
  acceptance: `
    test 900 -ge "$(wc -l < desktop/src/App.tsx)"                 # MEASURED baseline: 4660
    # PANEL: views.test.tsx used to live in views/ and matched this glob, so the
    # count was 8 and \`test 7 -eq\` could never pass however well the item was built.
    test 7 -eq "$(ls desktop/src/views/*.tsx | wc -l | tr -d ' ')"
    test 8 -eq "$(ls desktop/src/views/settings/*.tsx | wc -l | tr -d ' ')"
    test -f desktop/src/views/__tests__/views.test.tsx
    grep -q 'no_view_module_registers_a_listener' desktop/src/views/__tests__/views.test.tsx
    # a PURE MOVE adds no behaviour: the moved lines land, they do not multiply
    test 5200 -ge "$(cat desktop/src/App.tsx desktop/src/views/*.tsx desktop/src/views/settings/*.tsx | wc -l)"
    test 0 -eq "$(grep -c 'settingsTab === ' desktop/src/App.tsx)"
    cd ${APP} && npm ci
    npx tsc --noEmit ; test $? -eq 0
    npm run build ; test $? -eq 0
    npm test      ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y5-A', prompt: 'Y5', branch: 'loop/y5-a-design-tokens-and-one-visual-system', gated: null,
  title: 'A token layer so the seven views stop each inventing their own colours, spacing and radii',
  preflight: `
    test -f desktop/src/tokens.css
    test 0 -eq "$(grep -cE '#[0-9a-fA-F]{3,8}' desktop/src/App.css)"
    cd ${APP} && npm ci && npm run build
  `,
  spec: `
    \`App.css\` is 2,789 lines with literal colours, spacings and radii repeated
    throughout, which is mechanically why unrelated screens look like different
    apps — the "looks broken, not smooth" report is largely inconsistency, not
    any single broken screen.

    Do:
      * \`desktop/src/tokens.css\`: one \`:root\` block. Colour, elevation,
        radius, spacing (a 4px-based scale), type scale, motion durations and
        the two spring curves from the header. Name tokens by ROLE
        (--surface-raised, --text-muted, --accent-urgent), never by value
        (--gray-3). A role-named token survives a palette change; a
        value-named one guarantees the next inconsistency.
      * Migrate App.css and float.css to the tokens. Zero hex literals left in
        either. The gate greps for that, so a partial migration fails.
      * Keep the LOOK as it is in this item, to within a rounding error. This is
        a refactor whose whole value is that it is invisible; changing the
        palette at the same time makes every later visual diff unreadable.
      * Palette: pin the retro/LCD palette the companion already uses so the
        chrome and the character share one world instead of two
        (feedback_companion_must_be_cute — chunky pixels, limited retro
        palette). Read the palette off docs/prototypes/yappy-house.html and
        src/home/YappyHouse.tsx rather than inventing one.
      * Dark/light: whichever the app ships today is the one that must keep
        working. Define both token sets if both exist; define one and say so if
        only one does. Do not add a theme switcher in this item.

    PR body owes a before/after screenshot of all seven views at both window
    sizes, and the statement "no intentional visual change" with any unavoidable
    diff called out by name.

    What NOT to do:
      - Do NOT add Tailwind or a CSS framework. The CSP is strict
        (tauri.conf.json app.security.csp: style-src 'self' 'unsafe-inline',
        no external hosts) and a framework here buys nothing.
      - Do NOT restyle anything in this item. Y5-B..I do the visual work on top.
  `,
  acceptance: `
    test -f desktop/src/tokens.css
    test 0 -eq "$(grep -cE '#[0-9a-fA-F]{3,8}' desktop/src/App.css)"     # 0 literals left
    test 0 -eq "$(grep -cE '#[0-9a-fA-F]{3,8}' desktop/src/float.css)"
    grep -q -- '--surface' desktop/src/tokens.css
    grep -q -- 'cubic-bezier(0.05, *0.6, *0.4, *0.95)' desktop/src/tokens.css
    grep -q 'tokens.css' desktop/src/main.tsx desktop/src/float-main.tsx
    cd ${APP} && npm ci && npm run build ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y5-B', prompt: 'Y5', branch: 'loop/y5-b-every-view-has-empty-loading-and-error-states', gated: null,
  title: 'The "looks broken" fix: all seven views get a real empty state, a real loading state and a real error state',
  preflight: `
    test 7 -le "$(grep -ro 'data-empty-state' desktop/src --include=*.tsx | wc -l)"
    cd ${APP} && npm ci && npm test -- states
  `,
  spec: `
    MEASURED: \`git grep -c "empty-state\\|EmptyState\\|skeleton" -- desktop/src\`
    returns one match in App.tsx and one in App.css, for seven views
    (App.tsx:55-61: home, permissions, meetings, insights, dictionary,
    scratchpad, settings). A fresh install has no history, no meetings, no
    dictionary entries, no scratchpad notes and no insights — which is to say
    every view a new user opens is in its least-designed state. That is the
    first impression and it is the complaint.

    For EACH of the seven views ship three states:
      * EMPTY: one sentence saying what lives here, and ONE primary action that
        creates the first thing. Home's empty action is "hold fn and say
        something"; Dictionary's is "add a word"; Scratchpad's is "new note".
        Never a shrug, never a bare illustration with no action.
      * LOADING: a determinate state where the count is knowable and a calm
        indeterminate one where it is not. Not a full-page spinner. Not a
        skeleton that pulses forever (a skeleton with no timeout is how the
        Drivia audit found nineteen pages "still loading at 15s").
      * ERROR: what failed, in the user's terms, and the one button that retries
        or fixes it. A DB error is "Yap could not open its history file", not an
        SQLite code.
    Mark each with \`data-empty-state\` / \`data-loading-state\` /
    \`data-error-state\` so the gate can count them and a future browser walk
    can assert them.

    Two specific measured cases that must be covered by name:
      * Home before the model is downloaded. \`status.modelReady\` and
        \`needsPerms\` already gate a banner (App.tsx:2227) — make the whole view
        coherent in that state rather than a normal view with a warning strip.
      * Insights with zero takes. \`nav === "insights" && insights &&\`
        (App.tsx:2953) renders NOTHING when \`insights\` is falsy — a blank
        screen with a heading. That is a literal blank page in the shipped app.

    Copy rules: sentence case, no exclamation marks, name the action in the
    button ("Add a word", not "OK"), say what happens next rather than what went
    wrong (feedback_think_ux_first).

    Tests: extract each view's state decision into a pure function
    (\`viewState(data, loading, error)\`) in \`desktop/src/viewState.ts\` with
    \`viewState.test.ts\` covering the 3x7 matrix, so the assertions do not
    require rendering 3,439 lines of App.tsx.

    What NOT to do:
      - Do NOT ship an empty state without an action.
      - Do NOT use the same generic illustration for all seven. Generic is the
        thing being fixed (feedback_no_generic_ui).
      - Do NOT satisfy the gate by adding the attribute to a div that renders
        nothing. The gate counts attributes; the reviewer looks at the
        screenshots, and a hollow marker is a failed item.
    PANEL 2026-09-12 — three corrections.
      * Enumerate each view's REAL state set instead of demanding all three
        everywhere. A permissions screen with nothing in it is a bug, not an
        empty state, and a settings screen has no empty state either: those two
        get loading + error plus a settled "nothing to fix here" state. The list
        views (History, Meetings, Dictionary, Insights, Scratchpad) get all
        three. Lower the counts to match the enumeration — markers that exist
        only to satisfy a count are the hollow markers this item's own "What NOT
        to do" forbids, and build mode runs NO reviewer to catch them.
      * The gate that decides is Y0-E's structural smoke (it already fails a
        view that renders zero rows with no \`data-empty-state\`), not an
        attribute count. The counts are a cheap pre-flight.
      * Scratchpad's shape changes in DB-D (a real second window, versions), so
        its states are provisional here — say so in the PR body and do not build
        them twice.
      * Write the markers into the view MODULES (Y5-G has already moved them);
        never into the App.tsx shell.

  `,
  acceptance: `
    test 7 -le "$(grep -ro 'data-empty-state' desktop/src --include=*.tsx | wc -l)"
    test 7 -le "$(grep -ro 'data-loading-state' desktop/src --include=*.tsx | wc -l)"
    test 7 -le "$(grep -ro 'data-error-state' desktop/src --include=*.tsx | wc -l)"
    test -f desktop/src/viewState.ts
    test -f desktop/src/viewState.test.ts
    grep -q 'insights' desktop/src/viewState.test.ts
    cd ${APP} && npm ci
    npm test -- viewState ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build         ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y5-C', prompt: 'Y5', branch: 'loop/y5-c-the-full-pill-state-machine', gated: null,
  title: 'The pill gets every state the product has, including the transcribe/think gap Wilson named',
  preflight: `
    grep -q '"polishing"' desktop/src/pill/live.ts
    grep -q '"pasting"' desktop/src/pill/live.ts
    grep -q '"model_loading"' desktop/src/pill/live.ts
    cd ${APP} && npm ci && npm test -- pill/live
  `,
  spec: `
    \`LivePhase\` (live.ts:252) is \`idle | listening | thinking | done | sleepy\`.
    PERM-C adds \`blocked\`, Y2-C adds \`gated\`, Y3-C adds \`transcribing\`. This
    item completes the vocabulary against project_yap_pill_vision's list, which
    is Wilson's own enumeration:

      idle -> listening -> transcribing -> polishing -> pasting -> done
      plus: error · blocked (permission) · gated (license) · model_loading ·
            empty (nothing was said) · cancelled · sleepy

    Add the missing ones — \`polishing\`, \`pasting\`, \`error\`, \`model_loading\`,
    \`empty\`, \`cancelled\` — and make each REAL:
      * \`polishing\` is distinct from \`transcribing\`. It is the LLM stage and it
        has its own deadline (1200 ms, polish.rs:59), so its state has a
        knowable duration and must not look like an indefinite wait.
      * \`pasting\` exists because the paste is receipt-sequenced (YV39) and can
        fail on its own — a failure there is an Accessibility problem, not a
        transcription problem, and the pill must say the right one.
      * \`empty\` is the YV16 no-speech / hallucination-gate outcome: Yap
        correctly refuses to paste garbage, and today says nothing, so a user
        experiences a dead hotkey. This state is the whole visible payoff of
        that gate.
      * \`model_loading\` covers YV80's lazy arm: the first dictation after
        launch loads the engine while capture is already live (lib.rs:1156-1158).
        The pill should say the engine is warming rather than appear stuck.
      * \`error\` is the generic terminal state with a one-line reason from the
        take's \`last_error\` (lib.rs:1147 sets it) — never a code.

    Every phase needs: a duration policy (how long it holds), a next phase, and
    a rendering that the SHELL places at ALL THREE dock positions — once, not
    once per pill (OWNER DECISION 2026-09-13, top of this file). Both
    ClassicPill and YappyPill ship; they are the first two characters, so what
    each owes this item is PHASE COVERAGE AS DATA (a sprite/animation and copy
    for every phase in every tone), never a second copy of the phase logic or of
    the dock placement. Until Y5-K lands the registry, keep the per-character
    data in the component that already holds it and DO NOT add a third branch
    on \`pill_style\` anywhere outside those two components — Y5-K's first act
    is to lift exactly that data out.
    Put the policy in the pure state machine (live.ts) and only the rendering in
    the components — the
    file is already 310 lines of pure logic with 271 lines of tests precisely so
    this is possible, and ci.yml calls out that vitest is a gate because a
    regression here shipped once.

    Also fix the state-gap problem directly: Wilson's words are "fill the dead
    time after talking stops and before text appears". Assert in tests that
    there is NO reachable sequence in which the pill sits in a single
    undifferentiated phase across the whole post-hold pipeline. Concretely:
    \`listening -> done\` with no intervening phase is illegal.

    Tests, in \`desktop/src/pill/live.test.ts\`:
      * a transition table test — every phase has a defined successor set, and
        no phase is unreachable.
      * \`no_path_from_listening_to_done_without_an_intermediate_phase\`
      * \`every_phase_has_copy_in_every_tone\` — the tone presets are
        rude|friendly|rose (live.ts:29) and \`companion_tone: "friendly"\` is the
        default (lib.rs:412). A phase with no copy in one tone is a blank pill.
      * \`every_phase_renders_within_the_side_dock_strip\`
      * \`every_shipped_character_has_copy_and_art_for_every_phase\` — a table
        test driven off the list of characters that ship (classic, yappy), so
        adding a creature adds a row and not a test file. This is the fixture
        matrix the character system formalises in Y5-K.
      * precedence: blocked > gated > error > cancelled > the happy path.

    Depends on PERM-C, Y2-C, Y3-C.

    What NOT to do:
      - Do NOT add a phase without copy in all three tones.
      - Do NOT let a phase hold indefinitely with no timeout except \`idle\`,
        \`listening\`, \`blocked\` and \`gated\` (the four that legitimately wait on
        the user or the OS). Everything else has a deadline; say it in the table.
    PANEL 2026-09-12 — PERM-C now lands the COMPLETE \`LivePhase\` union in one
    commit (see PERM-C (b)), because six items across two unsynchronised lanes
    were each adding a variant to the same 310-line pure module from their own
    branch off main. So this item ADDS RENDERING AND COPY for phases that
    already exist in the union; it does not edit the union. If a phase is
    missing when this item starts, that is a signal PERM-C has not landed —
    report it, add the rendering against the union as PERM-C specifies it, and
    do not invent a differently-named variant.

  `,
  acceptance: `
    grep -q '"polishing"' desktop/src/pill/live.ts
    grep -q '"pasting"' desktop/src/pill/live.ts
    grep -q '"model_loading"' desktop/src/pill/live.ts
    grep -q '"empty"' desktop/src/pill/live.ts
    grep -q '"cancelled"' desktop/src/pill/live.ts
    grep -q '"error"' desktop/src/pill/live.ts
    grep -q 'no_path_from_listening_to_done_without_an_intermediate_phase' desktop/src/pill/live.test.ts
    grep -q 'every_phase_has_copy_in_every_tone' desktop/src/pill/live.test.ts
    grep -q 'every_phase_renders_within_the_side_dock_strip' desktop/src/pill/live.test.ts
    grep -q 'every_shipped_character_has_copy_and_art_for_every_phase' desktop/src/pill/live.test.ts
    cd ${APP} && npm ci
    npm test -- pill ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build    ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y5-D', prompt: 'Y5', branch: 'loop/y5-d-pill-physics-and-motion-from-the-parity-constants', gated: null,
  title: 'Soft-body pill motion using Wispr\'s measured spring constants, with Reduce Motion respected',
  preflight: `
    grep -q 'stiffness: 600' desktop/src/pill/motion.ts
    cd ${APP} && npm ci && npm test -- motion
  `,
  spec: `
    reference_wispr_parity_research P0 #2: "Soft-body pill physics (M · D).
    Wispr uses \`motion\` springs \`stiffness:600 damping:35 restDelta:0.05\` for
    snappy morphs and \`stiffness:300 damping:28\` for the slower one, plus
    \`cubic-bezier(0.05,0.6,0.4,0.95)\` at 100 ms for state changes and
    300-400 ms for expand/collapse. Yappy should go further: a squash-and-
    stretch response on click/drag and a settle bounce on dock. Why: Wilson's
    exact words — 'bounces when touched, feels soft not stiff'."

    Do:
      * \`desktop/src/pill/motion.ts\` — a hand-written critically-damped spring
        integrator (about forty lines) exposing the two named springs and the
        state-change easing, driven off the rAF loop the pill ALREADY runs
        (ClassicPill.tsx:48-62, which smooths \`--level\` and parks itself at
        rest). Do not add a motion library: the CSP blocks external hosts and
        the parked-rAF discipline from the YV81 energy pass must survive.
      * Squash-and-stretch on press and on drag release; a settle bounce on dock
        (the drag machinery is \`pill/drag.ts\`, YV65).
      * Every morph between the Y5-C phases uses the 100 ms state-change curve;
        expand/collapse uses 300-400 ms. One table, in motion.ts, so no
        component hardcodes a duration.
      * REDUCE MOTION: \`ClassicPill.tsx:44-47\` already paints one calm static
        frame under \`prefers-reduced-motion: reduce\`. All new motion must be
        behind the same check, and the information (phase, numeral, progress)
        must still be fully present in the static frame. Assert it.
      * ENERGY: the loop must still park when at rest. YV81 removed busy timers
        on purpose and YV24 idle-throttles canvases; a spring that never settles
        is a 60 fps rAF forever. Assert the integrator reaches rest and stops
        scheduling frames within a bounded number of ticks.

    Tests \`desktop/src/pill/motion.test.ts\`, pure and deterministic (inject the
    timestep, never use real time):
      * \`spring_600_35_settles_within_the_expected_tick_budget\`
      * \`spring_never_overshoots_past_the_soft_limit\`
      * \`reduce_motion_returns_the_target_immediately\`
      * \`integrator_reports_at_rest_and_stops\`
      * \`no_duration_literal_outside_motion_ts\` — a source sweep over
        desktop/src/pill.

    Depends on Y5-A (the motion tokens), Y5-C (the phases to morph between).

    PR body owes a screen recording of press, drag, dock and a phase morph, plus
    the same four with Reduce Motion on.

    What NOT to do:
      - Do NOT add framer-motion, motion, or GSAP.
      - Do NOT animate the trial numeral (Y2-B forbids it) or any other
        informational text.
      - Do NOT let the spring run while the pill is idle and off-screen.
  `,
  acceptance: `
    test -f desktop/src/pill/motion.ts
    test -f desktop/src/pill/motion.test.ts
    grep -q 'stiffness: 600' desktop/src/pill/motion.ts
    grep -q 'damping: 35' desktop/src/pill/motion.ts
    grep -q 'stiffness: 300' desktop/src/pill/motion.ts
    grep -q 'reduce_motion_returns_the_target_immediately' desktop/src/pill/motion.test.ts
    grep -q 'integrator_reports_at_rest_and_stops' desktop/src/pill/motion.test.ts
    node -e "const d=require('./desktop/package.json').dependencies;process.exit(Object.keys(d).some(k=>/framer|^motion$|gsap|popmotion/.test(k))?1:0)"
    cd ${APP} && npm ci
    npm test -- pill/motion ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build           ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y5-E', prompt: 'Y5', branch: 'loop/y5-e-hover-hysteresis-and-alpha-hit-testing', gated: null,
  title: 'The docked pill stops oscillating on the screen edge — the bug Wispr shipped a comment about',
  preflight: `
    grep -q 'inset: -12px' desktop/src/float.css
    cd ${APP} && npm ci && npm test -- hitbox
  `,
  spec: `
    reference_wispr_parity_research P0 #3, and the reason it is ranked P0:
    "an invisible \`inset: -12px\` alpha margin painted only in the expanded
    state, and a ~0.004-alpha background so the panel is clickable while
    invisible. Why: without it, an edge-docked pill oscillates expand/collapse
    when the cursor dwells on the screen edge — Wispr shipped a comment
    explaining they hit exactly this." §4.4 is titled "Hit-testing and hover
    (the part that breaks naive implementations)" and scores Yap ❌ with the note
    "Yap will hit this exact bug", plus 🟡 "NSPanel ignores margin clicks" on
    alpha hit-testing.

    Yap has half the machinery: \`ClassicPill.tsx:38-42\` publishes the capsule's
    rect via \`watchPillHitbox\` (YV65) "so the panel only takes the cursor over
    the pill itself; the transparent shadow margin stays click-through".

    Do:
      * The \`::before\` alpha margin at \`inset: -12px\`, painted ONLY while
        expanded. Painted always, it makes a 12 px dead zone around an idle
        pill; painted never, the boundary oscillates. The conditionality IS the
        fix.
      * \`rgba(0,0,0,0.004)\` background on the hot area so the box is clickable
        while visually absent.
      * Feed the expanded rect (capsule + margin) to \`watchPillHitbox\` so the
        NSPanel's ignore-mouse-events region matches what CSS is painting. A
        margin CSS believes in and the panel does not is worse than no margin.
      * Hysteresis in the state machine, not only in CSS: expand on enter,
        collapse only after the cursor has been outside the EXPANDED rect for a
        debounce. Put the thresholds in motion.ts's table.

    Tests \`desktop/src/pill/hitbox.test.ts\`, pure over a
    \`hoverState(rect, cursorPath)\` reducer:
      * \`dwell_at_the_dock_edge_produces_at_most_one_transition\` — the exact
        acceptance the parity note specifies: "simulated pointer dwell at the
        dock edge produces <=1 state transition". Drive a synthetic path that
        crosses the collapsed boundary repeatedly by one pixel.
      * \`collapsed_pill_has_no_margin_dead_zone\`
      * \`published_hitbox_matches_the_painted_rect_in_both_states\`
      * all three dock edges.

    Depends on Y5-D.

    What NOT to do:
      - Do NOT paint the margin in the collapsed state.
      - Do NOT fix oscillation with a long timeout. A 500 ms lag on expand makes
        the pill feel dead; hysteresis is a geometry fix, not a delay.
  `,
  acceptance: `
    grep -q 'inset: -12px' desktop/src/float.css
    grep -qE 'rgba\\(0, *0, *0, *0?\\.004\\)' desktop/src/float.css
    test -f desktop/src/pill/hitbox.test.ts
    grep -q 'dwell_at_the_dock_edge_produces_at_most_one_transition' desktop/src/pill/hitbox.test.ts
    grep -q 'collapsed_pill_has_no_margin_dead_zone' desktop/src/pill/hitbox.test.ts
    grep -q 'published_hitbox_matches_the_painted_rect_in_both_states' desktop/src/pill/hitbox.test.ts
    cd ${APP} && npm ci
    npm test -- hitbox ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build      ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y5-F', prompt: 'Y5', branch: 'loop/y5-f-error-toasts-that-say-what-to-do', gated: null,
  title: 'One error surface, one sentence per failure, one action — replacing raw strings and silent failures',
  preflight: `
    test -f desktop/src/toast.ts
    grep -q 'errorAdvice' desktop/src/errors.ts
    cd ${APP} && npm ci && npm test -- toast
  `,
  spec: `
    \`errors.ts\` is the right foundation and only half the job. It converts a
    rejection to a sentence (\`errorText\`, errors.ts:27-30) and falls back to
    \`String(e)\` — which is a raw Rust error string in front of a user. Nearly
    every Tauri command in Yap answers \`Result<_, String>\` (errors.ts:3-4 says
    so), so \`String(e)\` is the common path, not the rare one.

    Do:
      * An error CATALOGUE: \`errorAdvice(code) -> { line, action }\` in
        errors.ts, keyed on the structured codes the backend already returns
        (\`license_required\` exists at errors.ts:38; Y1/Y3 add
        \`mic_permission_required\`, \`silent_capture\`, \`cancelled\`). Every code
        Yap can emit gets a line and, where there is one, a button. An unknown
        code gets a generic line that still tells the user what to do (open the
        support bundle sheet, which already exists:
        src/support/SupportBundleSheet.tsx).
      * Make the backend emit codes where it emits strings on the take path.
        Do NOT boil the ocean: the take path, the paste path, the model path and
        the permission path. List the ones you converted in the PR body and
        leave the rest as strings with the generic advice.
      * ONE toast implementation, \`desktop/src/toast.ts\`, replacing whatever
        ad-hoc surfaces exist (audit them first and say in the PR body how many
        you found — \`git grep -n "note\\|setNote\\|banner" desktop/src/App.tsx\`
        is the starting point; Onboarding.tsx has its own \`note\` string).
        Queue, dedupe by code, auto-dismiss with a duration proportional to
        length, manual dismiss, and a cap so a storm cannot cover the app.
      * Errors also reach the PILL as \`error\` phase (Y5-C). Both surfaces, one
        catalogue: the pill shows the line, the main window adds the action.
      * A FAILURE MUST NEVER BE SILENT. Add a test that sweeps the take path for
        \`Err(...)\` returns that reach no emit and no toast. If a full sweep is
        impractical, enumerate the take path's error returns explicitly in the
        test and assert each is surfaced — an explicit list that must be updated
        is better than a clever grep that proves nothing.

    Tests: \`desktop/src/toast.test.ts\` (queue, dedupe, cap, duration) and
    \`desktop/src/errors.test.ts\` extended (it exists, 1 case at errors.test.ts:6)
    — every catalogued code has a non-empty line; no line contains a Rust type
    name, \`Error(\`, \`unwrap\` or a file path.

    What NOT to do:
      - Do NOT show a raw error string. If you have nothing better, say "Yap
        could not finish that take" and offer the support bundle.
      - Do NOT add Sentry or PostHog. Local crash capture (crash.rs) is the
        observability stack (feedback_queryguard).
      - Do NOT stack more than the cap. Three visible toasts is a broken app.
  `,
  acceptance: `
    test -f desktop/src/toast.ts
    test -f desktop/src/toast.test.ts
    grep -q 'errorAdvice' desktop/src/errors.ts
    grep -q 'every_catalogued_code_has_a_human_line' desktop/src/errors.test.ts
    grep -q 'no_line_leaks_a_rust_type_or_path' desktop/src/errors.test.ts
    test 0 -eq "$(git grep -ci 'sentry\\|posthog' -- desktop | wc -l)"
    cd ${APP} && npm ci
    npm test -- toast  ; test $? -eq 0
    npm test -- errors ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build      ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y5-J', prompt: 'Y5', branch: 'loop/y5-j-focus-names-announcements-and-contrast-floors', gated: null,
  title: 'The accessibility floor: a focus ring, an accessible name per state, one live region, a contrast bar on the token layer',
  preflight: `
    test 12 -le "$(grep -ro 'focus-visible' desktop/src --include=*.css | wc -l)"
    cd ${APP} && npm ci && npm test -- a11y
  `,
  spec: `
    PANEL 2026-09-12. This loop adds roughly thirteen pill phases and
    twenty-one view states and not one item requires a focus ring, an
    accessible name, an announcement or a contrast ratio. MEASURED at 4e8c9adf:
      grep -rniE 'aria|screen ?reader|focus-visible|contrast|WCAG' over all item
        files -> only prefers-reduced-motion hits
      desktop/src/App.css — three focus-related selectors in 2,789 lines
        (:957, :1408, :2397)
      desktop/src/pill/ClassicPill.tsx:142-144 — one aria-label covering three
        of the planned thirteen phases
      only pill/MeetingBadge.tsx:53 has aria-live
      desktop/src/App.tsx:2228 — a clickable <div className="banner warn">
      desktop/src-tauri/src/float_pill.rs:355 \`.focused(false)\` + the
        non-activating NSPanel: every pill affordance is mouse-only
    Y5-A freezes the look while centralising every colour, so the one cheap
    moment to fix contrast is the moment the plan forbids touching it. Hence a
    separate item, after Y5-G's split and after the states exist.

    Do:
      * A contrast test over the token pairs in tokens.css: 4.5:1 for body text,
        3:1 for large text and UI boundaries. A failing pair is a failed item,
        not a TODO — adjust the token, and say which.
      * Every state Y5-B added: its primary action is a real \`<button>\` and is
        keyboard reachable; \`:focus-visible\` is styled once, globally.
      * Every pill phase supplies an accessible name, and the pill root carries
        \`aria-live="polite"\` so a state change is announced once, not on every
        frame. Because the panel is non-activating, any affordance the pill
        gains must ALSO be reachable from the main window or a binding — state
        which, per affordance.
      * Sweep the clickable divs (App.tsx:2228 and its siblings) into buttons.

    What NOT to do:
      - Do NOT add an accessibility library or a linter plugin to satisfy this.
      - Do NOT put aria-live on the pill's frame-by-frame amplitude value.
  `,
  acceptance: `
    test -f desktop/src/a11y/contrast.test.ts
    grep -q 'every_token_pair_meets_its_contrast_floor' desktop/src/a11y/contrast.test.ts
    grep -q 'every_phase_has_an_accessible_name' desktop/src/pill/live.test.ts
    grep -rq 'aria-live' desktop/src/pill
    test 12 -le "$(grep -ro 'focus-visible' desktop/src --include=*.css | wc -l)"
    test 0 -eq "$(grep -rn 'className="banner warn"' desktop/src --include=*.tsx | wc -l)"
    cd ${APP} && npm ci
    npx tsc --noEmit ; test $? -eq 0
    npm test         ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y5-I', prompt: 'Y5', branch: 'loop/y5-i-vertical-dock-as-the-css-base', gated: null,
  title: 'Rebuild the docked pill with vertical as the CSS base — the architecture Wispr abandoned trying the other way',
  preflight: `
    grep -q 'data-bar-position' desktop/src/float.css
    grep -q -- '--flow-bar-length' desktop/src/float.css
    cd ${APP} && npm ci && npm test -- dock
  `,
  spec: `
    THE MOST IMPORTANT ARCHITECTURAL NOTE IN THE PARITY RESEARCH, verbatim from
    reference_wispr_parity_research (MUST-KNOW section, primary-sourced from the
    extracted bundle): "Wispr's vertical bar is NOT a rotated horizontal capsule
    — vertical/column layout is the CSS BASE and horizontal is the override,
    with orientation-neutral tokens (\`$flow-bar-length\`/\`$flow-bar-thickness\`);
    their first rotate-the-capsule attempt failed (their comment records the
    '30x6-always-horizontal collapse bug'). Only the primary listening states
    (ready/activePtt/activePopo) rotate into the strip — processing/error/
    completion banners STAY horizontal even when side-docked (fixed-width labels
    crush in a 30px column). Waveform inverts axis when docked (side dock = 2px
    bars animating on X). Yap's current YV53/65 move-the-capsule approach is the
    exact architecture Wispr abandoned — the design loop must rebuild with
    vertical-as-base."

    §4.2 gives the exact geometry table to build against, per state, with the
    side-dock overrides. Use it as the golden table; it is [BUNDLE]-sourced:
      resting 8x40 rgba(0,0,0,.5) 1px rgba(255,255,255,.5) border radius 6 ·
      ready 30x50 solid radius 22.5 · activePtt 30x73 ·
      activePopo 30x102.5 (+cancel/stop, row gap 8; padding 6px 0 on side docks) ·
      processing 30x98 padding 12px 6px, STAYS HORIZONTAL in both docks ·
      polishProcessing 136x30, on side docks column with label hidden and the
      progress fill flipping bottom-up · polishCompleted 30x152, side dock
      152x30 · error 30x91 stays horizontal · navigationActive 72x84 gap 4.

    Do:
      * Orientation-neutral tokens \`--flow-bar-length\` / \`--flow-bar-thickness\`
        in tokens.css (Y5-A). Column layout is the BASE. \`[data-bar-position]\`
        on the pill root supplies the bottom-dock horizontal override.
      * Map every Y5-C phase onto the table: which rotate into the strip and
        which stay horizontal. Yap has phases Wispr does not (blocked, gated,
        transcribing); decide and DOCUMENT each one's orientation, with the
        30 px-crush rule as the deciding test.
      * The waveform inverts axis on a side dock (2 px bars animating on X).
        ClassicPill.tsx drives 9 bars off a \`--level\` CSS var
        (ClassicPill.tsx:20, :52) — that is already the right seam; make the
        axis a token.
      * A golden geometry test: for each of \`left|right|bottom\` x each phase,
        assert the rendered bounding box matches the table, and that NO state
        exceeds the 30 px strip on a side dock. This is verbatim the parity
        note's own acceptance for P0 #1.

    Tests \`desktop/src/pill/dock.test.ts\` with the table as a committed
    fixture \`desktop/src/pill/dock-geometry.json\` so the numbers are reviewable
    as data.

    Depends on Y5-A, Y5-C, Y5-D, Y5-E. This item supersedes the YV53/65
    move-the-capsule approach; delete that code path rather than leaving both.

    What NOT to do:
      - Do NOT rotate the horizontal capsule with a CSS transform. That is the
        approach Wispr tried and abandoned, and their bug comment is the evidence.
      - Do NOT force the banner states into the column. They crush; the research
        says so explicitly and the golden table encodes it.
      - Do NOT keep the old positioning code alongside the new base.
    PANEL 2026-09-12, SUPERSEDED BY THE OWNER DECISION 2026-09-13 — THE
    GEOMETRY TABLE BELONGS TO THE SHELL AND IS CHARACTER-INDEPENDENT. The panel
    said "scope it to ClassicPill" because it read the two pills as two products
    and expected one to be cut. Both ship (top of this file), so scoping the
    table to one of them would have left the other with no dock contract at all.
    The correct target is the PILL SHELL: the measured Wispr per-state box
    (8x40 resting, 30x50 ready, 30x73, 30x102.5, 136x30 polishing ...) is the
    size and orientation of the WINDOW CONTENT BOX for a phase, and every
    character renders INSIDE that box. So:
      * \`dock-geometry.json\` is the shell's table, keyed by phase x dock, with
        NO style dimension in it. One table, forever, for every creature.
      * A character declares only how it fills the box it is given — a pixel
        character on an LCD pod scales or crops to the box, and a capsule paints
        it. If a character cannot render a phase inside the box the table gives
        it, that is a CHARACTER defect and the character's fallback covers it;
        it is never a reason to fork the table.
      * The golden test asserts the SHELL's boxes. The per-character sweep is
        Y5-C's completeness table and Y5-K's registry contract test, not this one.
      * Do not delete either renderer, and do not add a style dimension to the
        fixture.

  `,
  acceptance: `
    grep -q 'data-bar-position' desktop/src/float.css
    grep -q -- '--flow-bar-length' desktop/src/tokens.css
    grep -q -- '--flow-bar-thickness' desktop/src/tokens.css
    test -f desktop/src/pill/dock-geometry.json
    test 0 -eq "$(grep -c 'pill_style\\|pillStyle\\|classic\\|yappy' desktop/src/pill/dock-geometry.json)"
    test -f desktop/src/pill/dock.test.ts
    grep -q 'no_state_exceeds_the_thirty_pixel_strip_on_a_side_dock' desktop/src/pill/dock.test.ts
    grep -q 'banner_states_stay_horizontal_in_every_dock' desktop/src/pill/dock.test.ts
    grep -q 'waveform_axis_inverts_on_a_side_dock' desktop/src/pill/dock.test.ts
    test 0 -eq "$(grep -c 'transform: rotate' desktop/src/float.css)"
    cd ${APP} && npm ci
    npm test -- dock ; test $? -eq 0
    npm test         ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build    ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y5-K', prompt: 'Y5', branch: 'loop/y5-k-pill-character-system-and-habitat-layer', gated: null,
  title: 'The pill becomes a pluggable character system, and the habitat comes back as its habitat layer',
  preflight: `
    test -f desktop/src/pill/characters/registry.ts
    test -f desktop/src/pill/characters/characters.test.ts
    test -d desktop/src/home/habitat
    cd ${APP} && npm ci && npm test -- characters
  `,
  spec: `
    OWNER DECISION 2026-09-13 (Wilson), and it REINSTATES \`Y5-H\`, which the
    panel killed on a two-seat convergence. Wilson, verbatim: "I thought we were
    gonna develop it and then make more characters and make it more flexible ...
    there's a classic pill and there's a yappy pill and there's gonna be
    different pills with the different creatures that are coming."

    The panel's cost objection was CORRECT and is not waved away: a second pill
    style doubles the render and test surface of every pill item, and Y5-C alone
    was 13 phases x 2 styles x 3 docks. The remedy is structural. The shell
    stops knowing about creatures and the creatures stop knowing about the
    shell, so the matrix stops multiplying.

    THIS ITEM RUNS LAST IN THIS FILE ON PURPOSE: shell first (Y5-A tokens, Y5-C
    phases, Y5-D motion, Y5-E hit-testing, Y5-I dock geometry), then the
    characters, then the habitat. Do not start it before Y5-I has landed — the
    shell's box contract is the thing the character interface is defined
    against.

    (1) THE SHELL / CHARACTER SEAM.
      * \`desktop/src/pill/characters/types.ts\` — ONE interface. A character is
        \`{ id, label, render(frame) }\` where \`frame\` is what the shell already
        computed: \`{ phase, tone, level, box: {w,h}, dock, reducedMotion }\`.
        A character receives a BOX and paints inside it. It never reads
        \`pill_style\`, never reads settings, never reads license state, never
        positions a window, never knows a dock exists beyond the axis hint in
        \`frame\`.
      * \`registry.ts\` — \`registerCharacter()\` + \`characters()\`. The shell
        resolves \`settings.pill_style\` to a registered id ONCE, at the mount
        point, and falls back to \`classic\` for an unknown id rather than
        rendering nothing. \`pill_style\` stays a free string in
        \`AppSettings\` (lib.rs:234-235, default "classic", lib.rs:366/411) —
        do NOT turn it into a Rust enum: a new creature must be shippable
        without touching Rust.
      * PORT, do not rewrite: \`ClassicPill.tsx\` and \`YappyPill.tsx\` become
        \`characters/classic/\` and \`characters/yappy/\` with their phase art and
        copy as DATA, and everything that is not the creature — the capsule
        chrome, the waveform placement, dock geometry, hover hysteresis, the
        license chip placement, aria names — moves UP into the shell. The two
        characters must end up with NO duplicated shell logic between them; that
        deduplication is the whole point and it is measurable (see acceptance).
      * A NEW CREATURE IS A NEW MODULE PLUS A FIXTURE ROW, WITH NO SHELL CHANGE.
        Prove it: the item ships a third, deliberately minimal character
        (\`characters/example/\`) whose only purpose is to be the proof that the
        seam holds, and the contract test registers it with zero shell edits.

    (2) THE TEST MATRIX STOPS DUPLICATING.
      * The shell owns phase x dock. That suite runs ONCE, character-agnostic,
        against \`dock-geometry.json\` (Y5-I).
      * Each registered character is swept by ONE data-completeness contract
        test over the registry: every phase in the \`LivePhase\` union, in every
        tone (rude|friendly|rose, live.ts:29), has art and copy; nothing exceeds
        the box it was handed; \`imageSmoothingEnabled\` is false wherever a
        character paints to a canvas.
      * Registering an INCOMPLETE character must turn that contract test RED.
        That is the test's reason to exist and it is the item's mutation proof.

    (3) THE HABITAT LAYER — the reinstated \`Y5-H\`, with its design note
        preserved in docs/loop/DEFERRED.md §1. \`desktop/src/home/YappyHouse.tsx\`
        is 919 lines of working real-clock canvas scene with an ambient
        director; this is a REFACTOR PLUS A LAYER, not a rewrite.
      * \`desktop/src/home/habitat/\` — the habitat is the CHARACTER'S WORLD, and
        it is selected by the same registered character id, so a new creature
        brings its own pod. Split what exists into: the director (clock,
        routines, intent pathing), the scene (pod interior, dithered depth,
        parallax), and the character's own idle/reaction sprites, which come
        from the SAME character module the pill uses — one creature, two
        surfaces, one source of art.
      * EVENT-DRIVEN REACTIONS, from the design note: a take starting, a paste
        landing, a model finishing its download. The habitat subscribes to the
        same events the pill does; it never polls.
      * Routines on a real clock and intent pathing rather than a random walk.
      * AESTHETIC LOCK, unchanged and binding (top of this file): pixel art,
        chunky pixels, \`imageSmoothingEnabled = false\`, limited retro palette,
        Tamagotchi / Bitzee. Hand-coded. NOT smooth vector. No angled "angry"
        eyebrows. Paper/origami is REJECTED.
      * Wilson's taste is the real gate on the ART and cannot be automated in a
        build-first pass — which is why this item's acceptance gates the
        STRUCTURE (the seam, the completeness sweep, the deduplication, the
        no-shell-change proof) and the PR body carries the screenshots and a
        recording of the habitat for him to judge. Say that in the PR body.

    Depends on Y5-A, Y5-C, Y5-D, Y5-E, Y5-I. Consumes PERM-C's complete
    \`LivePhase\` union.

    What NOT to do:
      - Do NOT delete either shipped character. Both ship.
      - Do NOT let a character read settings, license state or dock position
        directly. Everything it needs arrives in \`frame\`.
      - Do NOT add a style dimension to \`dock-geometry.json\`.
      - Do NOT turn \`pill_style\` into a Rust enum or a TypeScript union of two
        literals — the whole point is that the next creature is additive.
      - Do NOT rewrite YappyHouse from scratch, and do not lose its ambient
        director.
      - Do NOT smooth the pixels.
  `,
  acceptance: `
    test -f desktop/src/pill/characters/types.ts
    test -f desktop/src/pill/characters/registry.ts
    test -d desktop/src/pill/characters/classic
    test -d desktop/src/pill/characters/yappy
    test -d desktop/src/pill/characters/example
    test -f desktop/src/pill/characters/characters.test.ts
    test -d desktop/src/home/habitat
    test -f desktop/src/home/habitat/habitat.test.ts
    grep -q 'registerCharacter' desktop/src/pill/characters/registry.ts
    grep -q 'every_registered_character_covers_every_phase_in_every_tone' desktop/src/pill/characters/characters.test.ts
    grep -q 'a_character_never_exceeds_the_box_the_shell_hands_it' desktop/src/pill/characters/characters.test.ts
    grep -q 'a_new_creature_needs_no_shell_change' desktop/src/pill/characters/characters.test.ts
    grep -q 'imageSmoothingEnabled' desktop/src/pill/characters/characters.test.ts
    grep -q 'the_director_runs_on_the_real_clock_not_a_random_walk' desktop/src/home/habitat/habitat.test.ts
    grep -q 'the_habitat_reacts_to_take_paste_and_model_events' desktop/src/home/habitat/habitat.test.ts
    test 0 -eq "$(grep -rc 'pill_style\\|pillStyle' desktop/src/pill/characters | grep -v ':0$' | wc -l | tr -d ' ')"
    test 0 -eq "$(grep -rl 'data-bar-position' desktop/src/pill/characters | wc -l | tr -d ' ')"
    test 0 -eq "$(grep -c 'classic\\|yappy' desktop/src/pill/dock-geometry.json)"
    cd ${APP} && npm ci
    npm test -- characters ; test $? -eq 0
    npm test -- habitat    ; test $? -eq 0
    npm test               ; test $? -eq 0
    npx tsc --noEmit       ; test $? -eq 0
    npm run build          ; test $? -eq 0
    printf '\\nregisterCharacter({ id: "mutant", label: "mutant", render: () => null });\\n' >> src/pill/characters/registry.ts
    npm test -- characters ; test $? -ne 0
    cd .. && git checkout -- desktop/src/pill/characters/registry.ts
    git diff --exit-code -- desktop/src/pill/characters/registry.ts
  `,
})
