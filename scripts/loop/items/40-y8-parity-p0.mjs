// Y8 — THE P0 PARITY QUEUE. Six items Wilson explicitly asked for, left on the
// line when the yap23 loop stopped. Source: the Wispr parity teardown
// (docs source of record: ~/Obsidian/Wilson-Brain/Notes/Wispr-Full-Parity-Research-2026-08-09.md,
// §3 "Prioritized implementation backlog for Yap", P0 items 1-6), quoted below.
// Every claim there is tagged [BUNDLE] = read out of the shipping competitor
// bundle, which is why the numbers are exact and must be used as given.
//
// P0 #1 (vertical reflow) is NOT here — it is Y5-I, because it depends on the
// token layer and the full state machine. P0 #2 and #3 are Y5-D and Y5-E for the
// same reason. This file is the remaining three: Scratchpad, Flow-Bar affordance
// slots, and the hotkey suite. Grouped in one file because all three land on the
// pill/window surface and must not race each other.
//
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'Y8-A', prompt: 'Y8', branch: 'loop/y8-a-hotkey-suite-completion', gated: null,
  title: 'The hotkey suite: hands-free, cancel, copy-last, paste-last, scratchpad, with real validation rules',
  preflight: `
    grep -q 'pub const HANDS_FREE' desktop/src-tauri/src/shortcuts.rs
    grep -q 'pub const COPY_LAST' desktop/src-tauri/src/shortcuts.rs
    grep -q 'pub const OPEN_SCRATCHPAD' desktop/src-tauri/src/shortcuts.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test shortcut_validation
  `,
  spec: `
    P0 #6, VERBATIM: "Hotkey suite completion. (S · L) Add: hands-free toggle,
    cancel, copy-last-transcript, paste-last-transcript, open-scratchpad, and
    per-transform shortcuts. Reuse the YV15 capture control; port Wispr's
    validation rules (<=3 keys, modifier required, reject left/right duplicates,
    detect overlap with the PTT chord). Accept (L): pure-fn tests for each
    validation rule; an overlap warning fires for \`fn\` vs \`fn+ctrl\`."

    MEASURED at 4e8c9adf: \`shortcuts.rs:143 ALL\` contains four bindings —
    PASTE_LAST (:86), UNDO_AI_EDIT (:98), DICTATION_TOGGLE_LEGACY (:110),
    MEETING_TOGGLE (:129). \`git grep "hands_free_binding\\|cancel_binding\\|
    copy_last"\` finds no hands-free hotkey, no cancel hotkey and no
    copy-last command. §2.1 of the teardown scores hands-free 🟡 "tray toggle
    only, no dedicated hotkey", cancel ❌, copy-last ❌, paste-last ❌ "hotkey +
    menu item" (the command exists at lib.rs:2714; the MENU item is Y6-B).

    Do:
      * Add to the existing \`shortcuts.rs\` table — it exists precisely so a
        binding is declared once (shortcuts.rs:3-8 explains the duplication bug
        it was created to kill): HANDS_FREE, COPY_LAST, OPEN_SCRATCHPAD.
        CANCEL is Y3-D's; if Y3-D has landed, do not add a second one.
      * A pure validation module \`shortcuts::validate\` with the four Wispr
        rules above, each its own function and its own test:
        at most 3 keys · a modifier is required · left/right variants of one
        modifier are not two keys · a chord that overlaps the PTT chord warns.
        The overlap case must fire for \`fn\` vs \`fn⌃\` specifically, which is
        Yap's own default pair (\`ptt_binding: "fn_control"\`, lib.rs:404) and
        therefore the one a user will actually hit.
      * Every new binding is remappable through the YV15 capture control and
        PERSISTS — the 2026-07-24 audit's top user bug was that half of settings
        never persist, PTT remap included. Y4-H's exhaustive round-trip test
        must cover each new binding; if Y4-H has not landed, add them to
        \`tests/settings_kv.rs\`.
      * Per-transform shortcuts are OUT of scope here: the transform library is
        Y9-A and a shortcut for a transform that does not exist is dead config.
        Say so in the doc comment.

    Tests \`tests/shortcut_validation.rs\` (pure, no app) plus keep
    \`tests/tray_hotkey_no_collision.rs\` green — it is the existing guard
    against two bindings claiming one chord and it must now cover seven.

    What NOT to do:
      - Do NOT register a global shortcut without a modifier. A bare letter
        global hotkey breaks typing everywhere.
      - Do NOT add per-transform shortcuts in this item.
  `,
  acceptance: `
    grep -q 'pub const HANDS_FREE' desktop/src-tauri/src/shortcuts.rs
    grep -q 'pub const COPY_LAST' desktop/src-tauri/src/shortcuts.rs
    grep -q 'pub const OPEN_SCRATCHPAD' desktop/src-tauri/src/shortcuts.rs
    test -f desktop/src-tauri/tests/shortcut_validation.rs
    grep -q 'at_most_three_keys' desktop/src-tauri/tests/shortcut_validation.rs
    grep -q 'a_modifier_is_required' desktop/src-tauri/tests/shortcut_validation.rs
    grep -q 'left_and_right_variants_are_not_two_keys' desktop/src-tauri/tests/shortcut_validation.rs
    grep -q 'fn_versus_fn_control_warns_of_overlap' desktop/src-tauri/tests/shortcut_validation.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test shortcut_validation      ; test $? -eq 0
    cargo test --features custom-protocol --test tray_hotkey_no_collision ; test $? -eq 0
    cargo test --features custom-protocol --test settings_kv              ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'DB-D', prompt: 'Y8', branch: 'loop/db-d-scratchpad-as-a-real-second-window', gated: null,
  title: 'Scratchpad: a second window on a hotkey, dictate-into-note, versions — the half-built feature finished',
  preflight: `
    grep -q 'note_versions' desktop/src-tauri/src/db.rs
    grep -q 'scratchpad' desktop/src-tauri/tauri.conf.json
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test scratchpad
  `,
  spec: `
    P0 #4, VERBATIM: "Scratchpad. (M · L) Second Tauri window, global hotkey
    (\`⌥S\`), \`notes(id,title,content,preview,pinned,created,modified)\` +
    \`note_versions\`, tabs, search, and dictate-into-note using the existing PTT
    path. Ship a Flow-Bar Scratchpad button (see #5). Rich text can be v2 —
    plain text + markdown first. Why: the #1 thing Wilson named, and it is the
    cheapest large-surface win Yap has left. Accept (L): hotkey opens the panel
    without stealing focus from the prior app; a dictation started inside the
    note lands in the note, not the previous app (this is the YV21 paste-target
    guard, re-pointed); notes survive restart; version rows written on every
    finalize."

    MEASURED: Yap has HALF of this and it is invisible. \`db.rs:648\` creates a
    \`scratchpad\` table; \`db.rs:2667-2721\` are its list/insert/delete queries;
    \`src/App.tsx:60\` has \`"scratchpad"\` in the \`Nav\` union, so there is a
    view inside the main window. There is NO second window
    (\`tauri.conf.json app.windows\` has exactly one, label "main"), no ⌥S
    hotkey, and no \`note_versions\` table.

    Do:
      * A second window in tauri.conf.json, label \`scratchpad\`, its own HTML
        entry (the repo already builds two entries — \`index.html\` and
        \`float.html\` — so follow that vite multi-entry pattern exactly).
      * NON-ACTIVATING open on ⌥S (Y8-A's OPEN_SCRATCHPAD binding). The
        acceptance above is explicit: it must not steal focus from the prior
        app, because the whole point is to jot without leaving what you are
        doing.
      * \`note_versions\` migration + a version row on every finalize. Migrations
        must stay idempotent (tests/db_migration_idempotent.rs).
      * DICTATE INTO THE NOTE. This is the load-bearing part and the one that
        can go wrong dangerously: a dictation started while the scratchpad has
        focus must land in the NOTE, and a dictation started anywhere else must
        still land in the previous app. That is the YV21 paste-target guard
        re-pointed, not bypassed — and Y6-C's paste_target_e2e tests must be
        extended to cover it rather than a second target mechanism appearing.
      * Tabs, a notes rail, search over \`searchable_content\` via the existing
        FTS setup. Plain text + markdown. No rich text, no editor library.
      * \`db.rs:1191\` already classifies the scratchpad as user-typed text —
        so Clear History's treatment of notes, which DB-C forces a decision on,
        applies here. Honour whatever DB-C decided; do not decide it twice.

    Tests \`tests/scratchpad.rs\`: notes survive restart; a version row per
    finalize; FTS finds a word in a note body; delete cascades to versions;
    and the target test named above.

    Depends on Y8-A (the binding), Y6-C (the paste-target tests), DB-C.

    What NOT to do:
      - Do NOT add Lexical, ProseMirror, TipTap or any editor library. The
        teardown says plain text first and the CSP blocks external assets.
      - Do NOT let the scratchpad window activate and steal focus.
      - Do NOT sync anything anywhere. Local-only is the brand
        (the teardown marks Wispr's note sync as explicitly not wanted).
  `,
  acceptance: `
    grep -q 'note_versions' desktop/src-tauri/src/db.rs
    grep -q '"scratchpad"' desktop/src-tauri/tauri.conf.json
    test -f desktop/scratchpad.html
    test -f desktop/src-tauri/tests/scratchpad.rs
    grep -q 'notes_survive_restart' desktop/src-tauri/tests/scratchpad.rs
    grep -q 'a_version_row_is_written_on_every_finalize' desktop/src-tauri/tests/scratchpad.rs
    grep -q 'dictation_in_the_note_lands_in_the_note' desktop/src-tauri/tests/scratchpad.rs
    grep -q 'opening_the_scratchpad_does_not_steal_focus' desktop/src-tauri/tests/scratchpad.rs
    node -e "const d=require('./desktop/package.json').dependencies;process.exit(Object.keys(d).some(k=>/lexical|prosemirror|tiptap|slate|quill/.test(k))?1:0)"
    cd ${APP} && npm ci
    npx tsc --noEmit ; test $? -eq 0
    npm run build    ; test $? -eq 0
    cd src-tauri
    cargo test --features custom-protocol --test scratchpad              ; test $? -eq 0
    cargo test --features custom-protocol --test db_migration_idempotent ; test $? -eq 0
    cargo test --features custom-protocol --test paste_target_e2e        ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y8-B', prompt: 'Y8', branch: 'loop/y8-b-flow-bar-affordance-slots', gated: null,
  title: 'The pill becomes a bar: five affordance slots, each with a tooltip and a vertical-dock layout',
  preflight: `
    grep -q 'AffordanceSlot' desktop/src/pill/slots.tsx
    cd ${APP} && npm ci && npm test
  `,
  spec: `
    P0 #5, VERBATIM: "Flow-Bar affordance slots. (M · D+L) Wispr's bar hosts 8
    named widgets. Yap's minimum viable set: Dictate (center) · Scratchpad ·
    Cancel/Stop · Word-count + live WPM · Mic/Settings menu. Each must have a
    tooltip and each must have a vertical-dock layout. Why: this is what makes
    the bar a *bar* and not a dot. Accept (L): every affordance renders in all
    three dock positions within the 30 px strip; tooltips flip side based on
    dock edge."

    MEASURED: the pill is dictate-only (the teardown scores in-bar affordances
    ❌ "pill is dictate-only"). \`ClassicPill.tsx\` renders a mic/stop glyph, a
    9-bar waveform and — since YV95 — a \`MeetingBadge\`. That badge is the
    existing precedent for a slot; generalise it rather than bolting on four
    more one-off children.

    Do:
      * \`desktop/src/pill/slots.tsx\`: a slot contract
        \`{ id, glyph, tooltip, visible(ctx), onPress }\` and a layout that
        places slots along the bar's LENGTH axis — which Y5-I made
        orientation-neutral (\`--flow-bar-length\` / \`--flow-bar-thickness\`), so
        one layout serves all three docks.
      * The five slots: Dictate (centre, always), Scratchpad (only when DB-D
        landed and the user enabled it — Wispr gates theirs behind an "Add to
        Flow Bar" toggle and so should Yap), Cancel/Stop (only while a take is
        live — Y3-D's cancel), Word-count + live WPM (only while listening or
        transcribing; the number must come from Y3-C's REAL chunk words during
        transcribing and may be the estimate only while listening — the same
        honesty rule), and a Mic/Settings menu.
      * Tooltips flip side based on dock edge. A tooltip that opens off-screen
        is the specific failure the acceptance calls out.
      * Slot visibility is a pure function of context so it is testable: no slot
        may occupy space when invisible (a 30 px strip has no room for a gap),
        and the trial numeral from Y2-B is NOT a slot — it rides the capsule.
        Say so, so the two systems do not fight over the same pixels.

    Tests \`desktop/src/pill/slots.test.ts\`:
      * every slot renders within the thickness bound in all three docks
        (reuse Y5-I's \`dock-geometry.json\`, do not write a second table)
      * tooltip side flips per dock edge
      * an invisible slot occupies zero length
      * the WPM slot uses real words while transcribing and the estimate only
        while listening

    Depends on Y5-I, Y3-C, Y3-D, DB-D.

    What NOT to do:
      - Do NOT ship eight slots. Five, per the spec, and no empty ones.
      - Do NOT show a slot whose feature has not landed.
      - Do NOT let a slot steal the drag gesture from \`pill/drag.ts\` (YV65).
  `,
  acceptance: `
    test -f desktop/src/pill/slots.tsx
    test -f desktop/src/pill/slots.test.ts
    grep -q 'tooltip_side_flips_per_dock_edge' desktop/src/pill/slots.test.ts
    grep -q 'an_invisible_slot_occupies_zero_length' desktop/src/pill/slots.test.ts
    grep -q 'wpm_uses_real_words_while_transcribing' desktop/src/pill/slots.test.ts
    grep -q 'dock-geometry.json' desktop/src/pill/slots.test.ts
    test 5 -eq "$(grep -c 'id: "' desktop/src/pill/slots.tsx)"
    cd ${APP} && npm ci
    npm test         ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build    ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y8-C', prompt: 'Y8', branch: 'loop/y8-c-earcons-and-sound-design', gated: null,
  title: 'Optional earcons for start, stop, paste and achievement — off by default, Yappy-voiced',
  preflight: `
    test -d desktop/src-tauri/assets/sounds
    grep -q 'earcons' desktop/src-tauri/src/lib.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test earcons
  `,
  spec: `
    P2 #17, VERBATIM: "Earcons. (S · D) start/stop/paste/achievement sounds, off
    by default, Yappy-voiced." The teardown's §2.1 row records what Wispr ships
    and why it matters: "\`dictation-start.wav\`, \`dictation-stop.wav\`,
    \`paste.wav\`, \`achievement.wav\`, \`popo-lock.wav\`, \`Notification.wav\` + 20
    versioned variants (\`v1…v12\`) — they A/B-tested earcons", against Yap's
    "❌ no audio feedback".

    It is listed P2 and it is in this P0 file for one reason: it is the cheapest
    fix available for "the app looks broken". A hold-to-talk app with no audible
    confirmation gives the user nothing to trust when the pill is in the corner
    of their eye, and Wilson's complaint is largely about not knowing what the
    app is doing.

    Do:
      * Four short sounds, BUNDLED in the app (no CDN, no download — the CSP
        blocks external media and a missing asset must be impossible). Generate
        them procedurally or author them; either way they ship in the repo and
        \`tauri.conf.json\`'s bundle resources include them.
      * OFF by default, one setting, with the level respecting the system
        volume. Add it to Y4-H's exhaustive settings round-trip.
      * INTERACTION WITH YV28, which is the trap: Yap MUTES the whole Mac's
        output while dictating (\`mute_while_dictating: true\` by default,
        lib.rs:421) and restores the exact prior state on stop/cancel/error/exit.
        A start earcon must therefore play BEFORE the mute and a stop earcon
        AFTER the restore, or the user hears nothing and the feature looks
        broken in a new way. Assert the ordering; this is the whole item.
      * Never play during a meeting recording — it would land on the recording.
        \`tests/meeting_no_automute.rs\` already encodes the sibling rule for
        muting; follow its shape.
      * Yappy-voiced, chunky and short (under 200 ms for start/stop), matching
        the pixel-pet aesthetic. No orchestral swells, no default macOS sounds.

    Tests \`tests/earcons.rs\`: assets exist and are bundled; off by default;
    start plays before the mute and stop after the restore; nothing plays while
    a meeting records; nothing plays when the setting is off.

    Depends on Y4-H.

    What NOT to do:
      - Do NOT play a sound on every state change. Four events, no more.
      - Do NOT default them on.
      - Do NOT fetch audio at runtime.
  `,
  acceptance: `
    test -d desktop/src-tauri/assets/sounds
    test 4 -le "$(ls desktop/src-tauri/assets/sounds | wc -l | tr -d ' ')"
    test -f desktop/src-tauri/tests/earcons.rs
    grep -q 'earcons_are_off_by_default' desktop/src-tauri/tests/earcons.rs
    grep -q 'start_plays_before_the_mute_and_stop_after_the_restore' desktop/src-tauri/tests/earcons.rs
    grep -q 'nothing_plays_while_a_meeting_records' desktop/src-tauri/tests/earcons.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test earcons             ; test $? -eq 0
    cargo test --features custom-protocol --test meeting_no_automute ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y8-D', prompt: 'Y8', branch: 'loop/y8-d-coaching-nudges-in-yappys-voice', gated: null,
  title: 'Coaching nudges: the bar teaches the app, in Yappy\'s voice, without becoming nagware',
  preflight: `
    test -f desktop/src/pill/nudge.ts
    cd ${APP} && npm ci && npm test
  `,
  spec: `
    P2 #20, VERBATIM: "Coaching nudges (pixel-voiced). (M · D) Yappy's version
    of \`hub_status_pulsar_*\`: 'click a textbox and hold fn'. This is Yappy's
    natural job and it is free personality."
    The teardown's §2.2 row on what Wispr's bar actually says: "Bar teaches you:
    'Click a textbox, hold \`fn\` to dictate', 'Open Cursor or any IDE', 'Try
    dictating by double tapping' — app-type-aware suggestions", Yap ❌. §4.2
    lists \`growthNudgeActive\` as a first-class bar state with its own geometry
    (fit-content x 32, a row on side docks).

    Do:
      * \`desktop/src/pill/nudge.ts\`: a pure engine
        \`nextNudge(ctx, shown) -> Nudge | null\` where ctx is what the app
        already knows — frontmost app category (\`mode_for_app\`), whether a
        text field has focus, takes so far, days since install, which features
        have never been used.
      * A nudge fires at most once per condition, ever, and the whole engine is
        capped: at most one nudge per session and none at all after the first
        week or after N successful takes. The teardown's own "explicitly not
        wanted" list includes "trial nag surfaces" — the discipline is the
        feature. Encode both caps as named constants and test them.
      * The nudge that matters most, and the one Wilson's report implies: the
        first take when NO text field has focus. Today the take succeeds, the
        paste has nowhere to go, and the user concludes Yap is broken. "Click
        into a text field, then hold fn" turns that into a lesson.
      * Copy in Yappy's voice across the three tones (rude|friendly|rose,
        live.ts:29), through the existing \`pill/tone.ts\` so there is one voice
        system. Never a sales line, never a price — Y2 owns commerce.
      * Renders as Y5-C's \`growthNudge\` phase with Y5-I's geometry, dismissible
        with one click, and never while a take is live.

    Tests \`desktop/src/pill/nudge.test.ts\`: one per session; never twice for a
    condition; silent after the caps; the no-focus nudge fires on the first
    no-target take; no nudge mentions money; every nudge has copy in all three
    tones; none fires during a take.

    Depends on Y5-C, Y5-I, Y8-B.

    What NOT to do:
      - Do NOT nudge more than once per session.
      - Do NOT put upgrade copy in a nudge.
      - Do NOT keep nudging a user who is clearly fluent.
  `,
  acceptance: `
    test -f desktop/src/pill/nudge.ts
    test -f desktop/src/pill/nudge.test.ts
    grep -q 'at_most_one_nudge_per_session' desktop/src/pill/nudge.test.ts
    grep -q 'never_fires_twice_for_one_condition' desktop/src/pill/nudge.test.ts
    grep -q 'no_nudge_mentions_money' desktop/src/pill/nudge.test.ts
    grep -q 'the_no_focus_nudge_fires_on_the_first_no_target_take' desktop/src/pill/nudge.test.ts
    grep -q 'every_nudge_has_copy_in_all_three_tones' desktop/src/pill/nudge.test.ts
    cd ${APP} && npm ci
    npm test         ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build    ; test $? -eq 0
  `,
})
