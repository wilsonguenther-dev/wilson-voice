// Y4 — FORMATTING. Wilson, 2026-09-12, verbatim: "formatting does not work
// whatsoever."
//
// He is exactly right, and the reason is not a broken algorithm. It is TWO
// DEFAULTS. The formatting pipeline is well built, heavily tested, and
// UNREACHABLE in the shipping configuration:
//
//   src-tauri/src/lib.rs:417    cleanup_level: "light".into()        <- shipped default
//   src-tauri/src/dictation.rs:636
//       fn runs_format(self) -> bool {
//           matches!(self, CleanupLevel::Medium | CleanupLevel::High) }
//   src-tauri/src/dictation.rs:644
//       fn runs_llm(self) -> bool { self == CleanupLevel::High }
//   src-tauri/src/lib.rs:418    polish_model: String::new()          <- "" = OFF
//   src-tauri/src/lib.rs:264-267 doc: with no id "`polish::polish_llm` never
//                                spawns the sidecar and the cleanup pipeline is
//                                exactly what it is today."
//
// CleanupLevel::Light (dictation.rs:607-616) runs the dictionary and backtrack
// ONLY. So on every fresh install: no list detection, no spoken punctuation
// marks (`apply_spoken_marks`), no email shape (`format_email_shape`, R13), no
// trailing-period tone rule (R3/R14), no LLM polish. Saying "new line" types the
// words "new line". That is "does not work whatsoever", literally.
//
// AND THE TESTS ALL PASS, because every fixture pins its own level:
//   tests/fixtures/formatting/group-a-sentence-shape.jsonl line 4:
//     {"id":"a04-r1-quotation-pair", ... "level":"medium", ...}
// The corpus proves a configuration the product does not ship. This is the
// canonical case from the "verification that verifies nothing" rule and it
// is why Y0-C (a shipped-defaults test) is a prerequisite.
//
// SECOND, INDEPENDENT DEFECT — long-form gets no polish even at High:
//   src-tauri/src/polish.rs:73   const MAX_POLISH_WORDS: usize = 400;
//   its doc, :71-72: "Long-form is rules-only: chunking is a later item".
//   Y3-B builds that chunking; Y4-E consumes it.
//
// THIRD — the polish model is never installed. Onboarding's STEP_ORDER
// (src/Onboarding.tsx:41) is welcome | permissions | calibration | done; it
// downloads an ASR model and never mentions a polish model, so `polish_model`
// stays "" forever even if the level is raised. SEC-C fixes the install path.
//
// SHARED PREAMBLE + STANDARD GATE: see 00-y0-harness-and-gates.mjs.
// Depends on Y0-C.

ITEMS.push({
  id: 'Y4-A', prompt: 'Y4', branch: 'loop/y4-a-formatting-on-by-default', gated: null,
  title: 'Formatting is on for a fresh install: the shipped default reaches the formatting stage',
  preflight: `
    grep -qE 'cleanup_level: "(medium|high)"' desktop/src-tauri/src/lib.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test shipped_defaults
    cargo test --features custom-protocol --test formatting_fixtures
  `,
  spec: `
    THE ONE-LINE FIX AND ITS FULL COST. Change lib.rs:417
    \`cleanup_level: "light"\` to \`"medium"\` so \`runs_format()\`
    (dictation.rs:636) is true out of the box, and then earn it:

      1. Run the WHOLE formatting corpus at the new default level and fix what
         breaks. The corpus is
         tests/fixtures/formatting/{group-a-sentence-shape,group-b-lists,group-c-llm-shape}.jsonl
         driven by tests/formatting_fixtures.rs. Rows pinning "medium" already
         pass; the work is rows that assumed "light".
      2. Add a fixture row at the SHIPPED level for every rule the formatting
         stage owns, because Y0-C's
         \`formatting_fixtures_declare_the_shipped_level\` now requires at least
         one and one is not enough. Cover, each with its own row: list
         detection; \`apply_spoken_marks\` punctuation-by-name ("period",
         "comma", "question mark", "quotation mark" pairing —
         dictation.rs:1203, :2845, :2892 show the table and its tests); the
         line/paragraph commands ("new line", "new paragraph"); email shape
         (R13, \`format_email_shape\`); the tone-dialled trailing period (R3/R14).
      3. \`polish_model\` STAYS "" in this item. LLM polish is SEC-C/C. This item
         is the RULES stage only, and separating them is what makes a regression
         attributable.
      4. Update Y0-C's \`shipped_defaults.rs\` table to the new value and remove
         any \`#[ignore]\` on \`defaults_reach_every_cleanup_stage\`.
      5. MIGRATE, UNCONDITIONALLY. PANEL 2026-09-12 overrides the first draft
         of this step, which said "migrate ONLY a value that is absent or was
         never explicitly set ... if you cannot, do not migrate". That condition
         is UNIMPLEMENTABLE and therefore resolves to "do nothing":
         \`save_settings\` persists the WHOLE \`AppSettings\` struct
         (lib.rs:2281-2298) under a container-level \`#[serde(default)]\`, so
         \`cleanupLevel\` is present in every store the moment onboarding saves
         and carries no "the user chose this" bit anywhere. And
         \`apply_settings_migrations\` returns early for any store at the current
         version (lib.rs:2253-2256, \`CURRENT_SETTINGS_SCHEMA_VERSION = 1\`).
         MEASURED on this machine: ~/Library/Application Support/WilsonVoice/
         settings.json holds \`"cleanupLevel": "light"\` with
         \`"schemaVersion": 1\` — i.e. the reporter's own install. A fix that
         only reaches fresh installs does not fix Wilson's complaint.
         So: bump \`CURRENT_SETTINGS_SCHEMA_VERSION\` to 2, add a v1 -> v2 arm
         that rewrites \`cleanup_level\` "light" -> "medium" with no condition
         (the old value was a default nobody chose), show the user ONE quiet
         line saying formatting is now on and where to turn it off, and start
         recording provenance from here on (\`cleanup_level_set_by_user: bool\`,
         set only by the settings command that writes the picker) so the NEXT
         default change can be honest. A v2 store with
         \`cleanupLevelSetByUser: true\` is never rewritten.

    Also fix the Settings copy. The picker offers none|light|medium|high with no
    explanation of what any of them do; the level names are engineer words. Give
    each option a one-line description in the user's terms, generated from the
    same \`runs_*\` predicates so the copy cannot drift from the behaviour.

    What NOT to do:
      - Do NOT gate the migration on a provenance bit that does not exist yet.
      - Do NOT set the default to "high". High runs the LLM, which needs a model
        that is not installed (SEC-C), and a default that silently no-ops is the
        exact class of bug being deleted.
      - Do NOT make formatting unconditional and delete CleanupLevel. "none" is
        a real user need (verbatim mode) and a real test axis.
      - Do NOT edit fixture EXPECTATIONS to make a failing row pass. If the
        formatter is wrong, fix the formatter.
  `,
  acceptance: `
    grep -qE 'cleanup_level: "medium"' desktop/src-tauri/src/lib.rs      # was "light"
    grep -qE 'polish_model: String::new\\(\\)' desktop/src-tauri/src/lib.rs  # unchanged in this item
    # PANEL: the two lines that used to sit here (>0 medium rows; a 'new paragraph'
    # substring) are GREEN ON UNMODIFIED MAIN — 36 medium rows and one matching
    # row already exist. They proved nothing. The real bar is Y0-C's per-rule
    # coverage test plus the migration proof below.
    grep -q 'every_rule_has_a_fixture_row_at_the_shipped_level' desktop/src-tauri/tests/shipped_defaults.rs
    grep -q 'a_v1_store_with_light_loads_as_medium' desktop/src-tauri/tests/settings_migration.rs
    grep -q 'an_explicit_user_choice_is_never_rewritten' desktop/src-tauri/tests/settings_migration.rs
    grep -qE 'CURRENT_SETTINGS_SCHEMA_VERSION: u32 = 2' desktop/src-tauri/src/lib.rs
    test 0 -eq "$(grep -rn '#\\[ignore\\]' desktop/src-tauri/tests/shipped_defaults.rs | wc -l)"
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test shipped_defaults     ; test $? -eq 0
    cargo test --features custom-protocol --test formatting_fixtures  ; test $? -eq 0
    cargo test --features custom-protocol --test settings_kv          ; test $? -eq 0    cargo test --features custom-protocol --test settings_migration   ; test $? -eq 0

  `,
})

ITEMS.push({
  id: 'Y4-I', prompt: 'Y4', branch: 'loop/y4-i-measure-the-polish-sidecar-against-the-real-weights', gated: null,
  title: 'Measure the polish stage against the real weights before anything is built on its envelope',
  preflight: `
    test -f docs/BUDGETS.md
    grep -q 'polish_latency' docs/BUDGETS.md
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test polish_envelope
  `,
  spec: `
    PANEL 2026-09-12. Every number the Y4 LLM lane is built on is a derived
    guess and nothing in the repo can falsify one: \`cargo test -p yap-polish
    --release\` passes 11 tests, all prompt- and protocol-level, and NOT ONE
    loads a model. MEASURED in the panel session on an M4 Pro, warm sidecar,
    deadline 600 s (i.e. pure model cost):
      qwen2.5-1.5B q4_k_m: 30w/423ms ok · 60w/527ms ok · 100w/864ms ok ·
        150w/1186ms ok · 200w/2880ms ERR=max_out · 300w/1526ms ok ·
        400w/5671ms ERR=max_out ; cold ready 7,323 ms
      qwen2.5-0.5B q4_k_m: 30w ok · 60w ok · then ERR=max_out at 100, 150, 200,
        300 and 400 words
    Constants for contrast: polish.rs:59 DEFAULT_POLISH_DEADLINE_MS = 1200,
    :63-64 bounds 100..5000, :73 MAX_POLISH_WORDS = 400, and
    yap-polish/src/main.rs:544-550 turns a max_out overrun into a HARD ERROR
    that discards the whole rewrite for KIND_POLISH. So the stage's real working
    envelope at the shipped deadline is roughly a paragraph, the 0.5B "fast
    tier" is worse than no model, and MAX_POLISH_DEADLINE_MS (5000) is below the
    measured 400-word cost.

    Do:
      * \`desktop/src-tauri/tests/polish_envelope.rs\`: drive the REAL sidecar
        against the REAL GGUF and record p50/p95 ms against input words, plus
        cold-ready ms. SKIP with a NAMED reason when the weights are absent
        (the Y11-F pattern) — never silently pass.
      * Write the curve into docs/BUDGETS.md under a \`polish_latency\` heading,
        with the machine it was measured on named.
      * Derive the per-chunk word budget for Y4-E FROM THE CURVE, not from 400,
        and state the derived number in BUDGETS.md so Y4-E can cite it.
      * Add the ceiling this loop was missing: peak resident bytes with the ASR
        engine and the polish child both loaded (Y3-G asserts it).

    Runs immediately after Y4-A and BEFORE SEC-C and Y4-E, both of which are
    currently specced against numbers nobody took.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/polish_envelope.rs
    grep -q 'p95' desktop/src-tauri/tests/polish_envelope.rs
    grep -q 'skipped_with_a_named_reason_when_the_weights_are_absent' desktop/src-tauri/tests/polish_envelope.rs
    grep -q 'polish_latency' docs/BUDGETS.md
    grep -qE '[0-9]+ ?ms' docs/BUDGETS.md
    grep -q 'per_chunk_word_budget' docs/BUDGETS.md
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test polish_envelope ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'SEC-C', prompt: 'Y4', branch: 'loop/sec-c-polish-model-is-actually-installable', gated: null,
  title: 'The polish model gets an install path, so the LLM stage can exist on a real machine',
  preflight: `
    grep -q 'polish' desktop/src/ModelSetup.tsx
    grep -q 'polish' desktop/src/Onboarding.tsx
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test polish_model_install
  `,
  spec: `
    \`models::polish_models()\` and a catalog exist (src-tauri/src/catalog.json,
    models.rs 762 lines, referenced by lib.rs:264). The sidecar exists and is
    bundled (\`bundle.externalBin: ["binaries/yap-polish", ...]\`,
    tauri.conf.json). \`SidecarPool\` handles the warm child, the readiness
    handshake and a restart budget (polish.rs:26-33, YV75 — and its doc records
    that a missing handshake made a cold child "indistinguishable from a wedged
    one", so this path has already been debugged once).

    What does not exist is any way for a user to get the WEIGHTS. Onboarding
    never mentions polish (STEP_ORDER, Onboarding.tsx:41) and
    \`polish_model: ""\` is the permanent default. So the entire LLM formatting
    stage is dead code on every installed copy.

    RUNTIME-DEPENDENCY RULE (the Yap rule, and the reason this is its own item):
    nothing may be "found on the machine". The model is either SHIPPED in the
    bundle or its install is MANAGED by the app. A GGUF is too large to ship, so
    it is managed:
      * Surface the polish model in \`ModelSetup.tsx\` next to the ASR model, with
        its size, what it changes, and that it is optional. Reuse the existing
        download machinery (\`models.rs\` already downloads, verifies and reports
        \`native_ready\`; the catalog already pins digests — check
        catalog.json for the polish entries and add them if absent, with a
        digest, never a bare URL).
      * A resumable, cancellable download with a digest check on completion. A
        half-downloaded GGUF that loads and produces garbage is worse than no
        model, and \`validate_polish\` (polish.rs) would then be the only thing
        between garbage and the user's document.
      * On success, set \`polish_model\` to the catalog id. That is the ONLY way
        it should ever become non-empty — never a hardcoded default.
      * If the model is absent, the stage no-ops exactly as it does today
        (lib.rs:264-267 documents this and \`polish_stage\`'s off-gate is already
        tested with zero model bytes). Assert that the no-op is unchanged.
      * Make the SIDECAR's absence diagnosable too: if the staged binary is
        missing (a broken build), say so once in the log with the expected path,
        rather than timing out every take against a deadline that can never be met.

    Tests \`tests/polish_model_install.rs\`, all with zero model bytes:
      * \`catalog_polish_entries_carry_a_digest\`
      * \`interrupted_download_is_not_marked_ready\`
      * \`digest_mismatch_is_rejected_and_deleted\`
      * \`absent_model_leaves_the_pipeline_byte_identical\` — the regression that
        matters most; compare against the rules-only output on the fixture corpus.
      * \`missing_sidecar_binary_is_reported_once_not_per_take\`

    What NOT to do:
      - Do NOT download a model during onboarding by default. It is optional and
        large; offering it is right, forcing it is not.
      - Do NOT set \`polish_model\` to a catalog id that is not verified present
        on disk. That is how a default becomes a silent 1200 ms timeout on
        every take.
      - Do NOT add a network call anywhere on the dictation path.
    PANEL 2026-09-12 — this item is aimed at the wrong half of the gap, and its
    headline acceptance passes on unmodified main.
      * The catalog is ALREADY done: src/catalog.json polish_models holds
        qwen2.5-1.5b-instruct-q4_k_m (1,117,320,736 B, sha256 pinned,
        recommended_rank 1) and qwen2.5-0.5b-instruct-q4_k_m (491,400,032 B,
        rank 2). \`grep -qE 'polish' catalog.json\` is green today. Drop the
        "add them if absent" instruction.
      * The REAL gap is narrower and unasserted: \`download_polish_model_with\`
        exists (models.rs:209) and there is NO Tauri command and ZERO frontend
        references to polishModel anywhere in desktop/src
        (\`grep -rn 'polishModel\\|polish_model' desktop/src/*.tsx\` -> 0;
        \`grep -c polish ModelSetup.tsx\` -> 0). The weights can be fetched by
        Rust and never by a user. Build: a registered command in the invoke
        handler, a ModelSetup row, and a test that \`polish_model\` becomes
        non-empty ONLY after a digest-verified file exists on disk.
      * ADD A FREE-SPACE PRECONDITION and a named refusal. A 1.12 GB download
        with no disk check and no low-RAM behaviour is how "optional polish"
        becomes "polish silently does nothing" on exactly the machines least
        able to tell.
      * DO NOT offer the 0.5B as a tier yet. MEASURED (see Y4-I): it returns
        err=max_out at 100, 150, 200, 300 and 400 words, i.e. it is worse than
        no model. Ship the 1.5B only; the fast tier is deferred until the
        measured curve says otherwise.
      * The readiness budget is DERIVED, not measured: polish.rs
        SIDECAR_READY_BUDGET = 10 s, and cold ready for the 1.5B measured 7,323
        ms on an M4 Pro. A base M-series Air paging in the same 1.12 GB will
        cross 10 s and be declared "stuck" while it is merely loading. Take the
        number from Y4-I's measurement, or make "model loading" a real pill
        state (Y5-C) so lateness is reported instead of guessed.

  `,
  acceptance: `
    grep -q 'polish' desktop/src/ModelSetup.tsx
    grep -qE 'polish' desktop/src-tauri/src/catalog.json
    test -f desktop/src-tauri/tests/polish_model_install.rs
    grep -q 'absent_model_leaves_the_pipeline_byte_identical' desktop/src-tauri/tests/polish_model_install.rs
    grep -q 'digest_mismatch_is_rejected_and_deleted' desktop/src-tauri/tests/polish_model_install.rs
    grep -q 'missing_sidecar_binary_is_reported_once_not_per_take' desktop/src-tauri/tests/polish_model_install.rs
    grep -qE 'polish_model: String::new\\(\\)' desktop/src-tauri/src/lib.rs   # still never defaulted on
    cd ${APP} && npm ci && npm run build ; test $? -eq 0
    cd src-tauri
    cargo test --features custom-protocol --test polish_model_install ; test $? -eq 0
    cargo test --features custom-protocol --test formatting_fixtures  ; test $? -eq 0    grep -rq 'polish_model' desktop/src-tauri/src/lib.rs
    grep -rq 'polish' desktop/src/ModelSetup.tsx
    grep -q 'refuses_when_there_is_not_enough_free_space' desktop/src-tauri/tests/polish_model_install.rs
    grep -q 'polish_model_is_set_only_after_a_digest_verified_file_exists' desktop/src-tauri/tests/polish_model_install.rs

  `,
})

ITEMS.push({
  id: 'Y4-C', prompt: 'Y4', branch: 'loop/y4-c-paragraphing-the-rule-that-does-not-exist', gated: null,
  title: 'Paragraphing: long speech becomes paragraphs by rule, not one wall of text',
  preflight: `
    grep -q 'fn paragraph_breaks\\|fn insert_paragraphs' desktop/src-tauri/src/dictation.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test paragraphing
  `,
  spec: `
    Wilson named paragraphing specifically. The rules stage does list detection
    and spoken marks (dictation.rs's \`format_dictation\` +
    \`apply_spoken_marks\`); it has no paragraph rule at all —
    \`git grep -n "paragraph" desktop/src-tauri/src/dictation.rs\` finds the
    spoken COMMAND ("new paragraph") and no automatic rule. A five-minute
    dictation therefore arrives as one unbroken block, which is the single most
    visible way formatting "does not work".

    Build it as a PURE rule with real inputs, not a guess:
      * \`insert_paragraphs(text, &[PauseSpan], mode, style) -> String\`.
      * The strongest signal available is SILENCE, and Yap already measures it:
        \`vad.rs\` (803 lines, Silero VAD) and the RMS series at record.rs:881.
        The ASR path also carries timestamps (\`max_timestamp_kind\`,
        asr_engine.rs:382). Thread the pause spans from capture through to the
        rules stage — today they are computed and thrown away. A pause over a
        threshold at a sentence boundary is a paragraph break; a pause mid-clause
        is not.
      * Secondary, when timing is unavailable (a recovered or imported take):
        sentence count and discourse markers ("so", "anyway", "okay so",
        "moving on"). These must be a FALLBACK with its own test, not the
        primary mechanism — a marker-only rule breaks paragraphs on speech
        habits and is the reason so many dictation apps feel arbitrary.
      * Mode-aware, via the existing \`DictationMode\` (6 modes, \`mode_for_app\`):
        Email gets paragraphs and a blank line between them; chat modes get
        NONE — a paragraph break in Slack sends the message. That is a
        correctness bug, not a taste call. Assert it.
      * Idempotent: running the rule twice must produce the same string, and it
        must never touch text the user already broke with "new paragraph".

    Tests \`tests/paragraphing.rs\` plus new fixture rows in
    group-a-sentence-shape.jsonl at the shipped level:
      * \`pause_at_a_sentence_boundary_breaks\`
      * \`pause_mid_clause_does_not_break\`
      * \`chat_mode_never_inserts_a_break\`
      * \`explicit_new_paragraph_is_preserved_and_not_doubled\`
      * \`idempotent_on_second_application\`
      * \`marker_fallback_only_fires_without_timing\`
      * \`a_short_take_is_unchanged\` — the no-regression floor.

    Depends on Y4-A (the stage must actually run).

    What NOT to do:
      - Do NOT paragraph on a fixed word count. "Every 60 words" is the
        arbitrary behaviour users notice and hate.
      - Do NOT put this in the LLM stage. It must work with no model installed.
      - Do NOT break paragraphs inside a detected list.
  `,
  acceptance: `
    grep -q 'fn insert_paragraphs' desktop/src-tauri/src/dictation.rs
    test -f desktop/src-tauri/tests/paragraphing.rs
    grep -q 'chat_mode_never_inserts_a_break' desktop/src-tauri/tests/paragraphing.rs
    grep -q 'pause_mid_clause_does_not_break' desktop/src-tauri/tests/paragraphing.rs
    grep -q 'idempotent_on_second_application' desktop/src-tauri/tests/paragraphing.rs
    grep -q 'marker_fallback_only_fires_without_timing' desktop/src-tauri/tests/paragraphing.rs
    grep -q 'paragraph' desktop/src-tauri/tests/fixtures/formatting/group-a-sentence-shape.jsonl
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test paragraphing        ; test $? -eq 0
    cargo test --features custom-protocol --test formatting_fixtures ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y4-D', prompt: 'Y4', branch: 'loop/y4-d-lists-and-punctuation-at-the-shipped-level', gated: null,
  title: 'Lists, nesting and spoken punctuation proven at the level the product ships, with the gaps filled',
  preflight: `
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test formatting_fixtures
    test 0 -lt "$(grep -h '"level":"medium"' tests/fixtures/formatting/group-b-lists.jsonl | wc -l)"
  `,
  spec: `
    \`group-b-lists.jsonl\` exists and the list detector exists
    (\`runs_format\` -> \`format_dictation\` "(list detection)",
    dictation.rs:636/:652). What is unproven is behaviour at the shipped level
    and the cases a real dictation hits.

    Audit the fixture file first and write down, in the PR body, which of these
    is already covered and which is new. Then cover every gap:
      * "one, two, three" as PROSE vs "number one ... number two" as a LIST.
        Over-eager list detection is the failure users report as "it keeps
        making bullets out of my sentences".
      * Ordered vs unordered; a list that starts mid-paragraph; a list of ONE
        item (must not become a list).
      * Nesting one level ("sub-bullet" / "indent"), and the explicit decision
        NOT to support deeper nesting if that is the call — write the decision
        into the doc comment either way.
      * Spoken marks in a list item without terminating the item.
      * The full spoken-punctuation table: period, comma, question mark,
        exclamation point, colon, semicolon, dash, quotation mark (which
        ALTERNATES open/close — group-a line 4 already pins this pairing
        behaviour, extend it to three pairs in one utterance), apostrophe,
        parentheses, at symbol, percent, degree. \`formatting_fixtures.rs:217\`
        lists the mark vocabulary the corpus knows; make sure the fixture rows
        exercise it rather than the constant merely naming it.
      * The literal-escape case: how does a user type the WORD "period"? There
        must be an answer and it must be tested. If the answer is "say 'literal
        period'", implement it; if it is "we accept the ambiguity", write that
        in the doc comment so the next engineer does not re-litigate it.
      * Capitalization after a spoken period; no capitalization after an
        abbreviation ("e.g.", "Dr.").
      * R5 lead casing from caret context must still win. polish.rs:35-41
        explains at length that \`dictation::join_with_context\` runs AFTER
        \`run_cleanup\` so the context's casing decision lands last, asserted by
        \`polish_never_overrides_the_r5_lead_case\`. Keep that test green and add
        the rules-stage equivalent.

    Every new case is a FIXTURE ROW at \`"level":"medium"\`, not a bespoke unit
    test, so the corpus stays the single source of truth
    (tests/fixtures/README.md documents the corpus conventions — follow them).

    Depends on Y4-A.

    What NOT to do:
      - Do NOT add a case by relaxing an existing expectation.
      - Do NOT make list detection LLM-dependent. It is a rules-stage feature
        and must work with no model.
  `,
  acceptance: `
    # PANEL: a medium-row COUNT is already 36 on main — vacuous. The bar is
    # per-rule coverage at the shipped level, enumerated from the rule table.
    grep -q 'every_rule_has_a_fixture_row_at_the_shipped_level' desktop/src-tauri/tests/shipped_defaults.rs
    grep -q 'every_rule_id_is_covered_and_names_the_missing_ones' desktop/src-tauri/tests/formatting_fixtures.rs
    grep -q 'literal' desktop/src-tauri/tests/fixtures/formatting/group-a-sentence-shape.jsonl
    grep -q 'sub-bullet\\|indent' desktop/src-tauri/tests/fixtures/formatting/group-b-lists.jsonl
    grep -q 'single_item' desktop/src-tauri/tests/fixtures/formatting/group-b-lists.jsonl
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test formatting_fixtures ; test $? -eq 0
    cargo test --features custom-protocol polish_never_overrides_the_r5_lead_case ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y4-E', prompt: 'Y4', branch: 'loop/y4-e-polish-long-form-by-chunking', gated: null,
  title: 'Long-form gets polished: the 400-word cliff becomes a chunked pass that keeps the deadline',
  preflight: `
    test 0 -eq "$(grep -c 'MAX_POLISH_WORDS: usize = 400' desktop/src-tauri/src/polish.rs)"
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test polish_long_form
  `,
  spec: `
    MEASURED: \`polish.rs:73\` \`const MAX_POLISH_WORDS: usize = 400;\` with the
    doc at :71-72: "Long-form is rules-only: chunking is a later item, and a
    single pass over 400+ words cannot hold the deadline (§2.3)." So the longest
    and most valuable dictations — the ones Wilson actually does — get the least
    formatting. The deadline is \`DEFAULT_POLISH_DEADLINE_MS = 1200\`
    (polish.rs:59), bounded 100..5000 (:64-65).

    Do:
      * Chunk the polish pass at PARAGRAPH boundaries (Y4-C now produces them)
        with a small overlap of context, each chunk under the per-chunk word
        budget, each with its own slice of the deadline. The whole-take deadline
        scales with chunk count; do NOT give one long take 1200 ms total.
      * \`validate_polish\` (polish.rs, V1-V7) runs PER CHUNK. A rejected chunk
        keeps that chunk's rules text and the rest still benefits. Today a
        rejection loses the whole rewrite; per-chunk rejection is strictly
        better and preserves the never-lose-text property, which is the module's
        stated reason to exist (polish.rs:4-8).
      * The content-word retention floor (\`RETENTION_FLOOR = 0.80\`) and the
        length band (\`MAX_LENGTH_RATIO 2.5\` / \`MIN_LENGTH_RATIO 0.45\`) are
        computed per chunk AND across the whole take. A chunk-wise pass that
        each time stays inside the band can still drift the document; assert
        the aggregate.
      * Seam hygiene: a chunk boundary must not duplicate a sentence, must not
        capitalize mid-sentence, and must not drop the join whitespace. This is
        the same class of bug Y3-B's ASR seams have; write the test, do not
        assume.
      * \`SidecarPool\`'s restart budget (YV75) must not be exhausted by a
        12-chunk take. Assert the budget is per-take-scaled or per-chunk-reset,
        and say which.
      * Keep the off/too-short gates exactly as they are: under
        \`MIN_POLISH_WORDS = 4\` still costs no round trip.

    Tests \`tests/polish_long_form.rs\`, driven through the injectable
    \`PolishClient\` seam (polish.rs:31-33 — the tests already inject a client
    that sleeps past the deadline, one that panics, one that returns garbage;
    reuse all three at length):
      * \`a_two_thousand_word_take_is_polished_not_skipped\`
      * \`one_rejected_chunk_keeps_only_that_chunk_raw\`
      * \`aggregate_retention_floor_holds_across_chunks\`
      * \`seam_does_not_duplicate_or_drop_a_sentence\`
      * \`deadline_scales_with_chunk_count\`
      * \`a_panicking_client_on_chunk_seven_still_returns_the_whole_document\`
      * \`short_take_path_is_byte_identical\`

    Depends on SEC-C (a model to run), Y4-C (paragraph boundaries), Y3-B
    (chunking precedent to follow — reuse its geometry helpers where they fit).

    What NOT to do:
      - Do NOT raise the deadline instead of chunking.
      - Do NOT abandon per-chunk validation to save time. The validator is the
        only thing standing between an untrusted local model and the user's
        document; polish.rs:3-5 is explicit that sidecar output is UNTRUSTED.
      - Do NOT re-apply R5 lead casing inside polish. polish.rs:35-41 explains
        why that is deliberately absent.
    ── PANEL 2026-09-12, BINDING: build this against MEASURED numbers ──────
    One seat measured the stage this item chunks. On an M4 Pro with an
    effectively unlimited deadline, the 1.5B returns a usable rewrite only up to
    ~150 words and hard-errors (err=max_out, which for KIND_POLISH DISCARDS the
    whole rewrite — yap-polish/src/main.rs:544-550) at 200 and 400 words; the
    0.5B errors at every length from 100 words up. At the shipped
    DEFAULT_POLISH_DEADLINE_MS = 1200 every input over ~150 words fails, and
    MAX_POLISH_DEADLINE_MS = 5000 is itself BELOW the measured 400-word cost
    (5,671 ms). So this item's seven tests — all of which drive the injectable
    PolishClient seam with fakes, including \`deadline_scales_with_chunk_count\`
    — can be green while the real model never once meets the deadline.
      * Y4-I runs FIRST and writes the curve into docs/BUDGETS.md. Take the
        per-chunk word budget FROM THAT CURVE, not from MAX_POLISH_WORDS = 400,
        and cite the number in a comment.
      * Raise MAX_POLISH_DEADLINE_MS above the measured per-chunk cost, or the
        deadline discipline is decorative. (This is not "raise the deadline
        instead of chunking" — it is making the ceiling bigger than the floor.)
      * A max_out overrun stops being a discard: keep the RULES text for that
        chunk, keep the successful chunks, and record the reason. A 2,000-word
        take chunked to a 150-word envelope is 13-14 sequential round trips
        (~17 s after the hold ends on the fastest Apple laptop silicon), so the
        per-chunk fallback is the difference between late and lost.
      * At least ONE test must drive the real sidecar, skipping with a NAMED
        reason when the weights are absent. Fakes cannot see this failure.

  `,
  acceptance: `
    test 0 -eq "$(grep -c 'MAX_POLISH_WORDS: usize = 400' desktop/src-tauri/src/polish.rs)"   # 0 (was 1)
    grep -q 'POLISH_CHUNK' desktop/src-tauri/src/polish.rs
    test -f desktop/src-tauri/tests/polish_long_form.rs
    grep -q 'a_two_thousand_word_take_is_polished_not_skipped' desktop/src-tauri/tests/polish_long_form.rs
    grep -q 'one_rejected_chunk_keeps_only_that_chunk_raw' desktop/src-tauri/tests/polish_long_form.rs
    grep -q 'aggregate_retention_floor_holds_across_chunks' desktop/src-tauri/tests/polish_long_form.rs
    grep -q 'a_panicking_client_on_chunk_seven_still_returns_the_whole_document' desktop/src-tauri/tests/polish_long_form.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test polish_long_form    ; test $? -eq 0
    cargo test --features custom-protocol --test formatting_fixtures ; test $? -eq 0    grep -q 'per_chunk_word_budget' docs/BUDGETS.md
    grep -q 'a_max_out_chunk_keeps_its_rules_text_and_says_so' desktop/src-tauri/tests/polish_long_form.rs
    grep -q 'skipped_with_a_named_reason_when_the_weights_are_absent' desktop/src-tauri/tests/polish_long_form.rs

  `,
})

ITEMS.push({
  id: 'Y4-F', prompt: 'Y4', branch: 'loop/y4-f-app-aware-formatting-that-is-actually-applied', gated: null,
  title: 'App-aware formatting: the six modes change the output, proven per app, and auto mode picks correctly',
  preflight: `
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test app_aware_formatting
  `,
  spec: `
    \`mode_for_app\` and 6 \`DictationMode\`s exist, the shipped default is
    \`dictation_mode: "auto"\` (lib.rs:415), \`focus.rs\` (342 lines) resolves the
    frontmost app, and \`polish_styles\` is a per-mode tone map (lib.rs:279).
    reference_wispr_parity_research §2.4 scores Yap ✅ on app-category detection.
    What is NOT proven is that the mode changes the OUTPUT at the shipped
    configuration, and Wilson's complaint says it does not.

    Do:
      * A table test over the six modes with ONE input utterance, asserting the
        outputs are pairwise DIFFERENT where the rules say they should be and
        IDENTICAL where they should not. That single assertion is what catches
        "mode is detected, threaded, and then ignored" — which is the shape of
        a defect nothing currently tests for.
      * Email: greeting/sign-off shape (R13, \`format_email_shape\`), paragraphs,
        full punctuation, trailing period per the tone dial.
      * Chat / work chat: no paragraph breaks (a newline sends), no sign-off,
        trailing period per R3/R14's casual setting, emoji left alone.
      * Code / terminal context: NO spoken-mark expansion of characters that are
        syntax, no auto-capitalization, no trailing period. If there is no such
        mode today, this item adds it — Wilson lives in Claude Code and Cursor
        (reference_wispr_parity_research P1 #12 calls IDE context "the highest
        personal-ROI item on the list"), and a dictation that capitalizes his
        identifiers is worse than no dictation.
      * Document / notes: paragraphs, lists, full punctuation.
      * \`auto\` resolution: a fixture table of real bundle identifiers
        (com.apple.mail, com.tinyspeck.slackmacgap, com.google.Chrome,
        com.microsoft.VSCode, com.apple.Terminal, com.apple.TextEdit) mapping to
        modes, with an explicit DEFAULT for an unknown bundle id, and a test
        that the default is the CONSERVATIVE mode — an unknown app must not get
        the most aggressive formatting.
      * Browser ambiguity is real (Gmail in Chrome is email; Google Docs in
        Chrome is a document). \`AXSelectedText\`/URL resolution is P1 #11 in the
        parity backlog and is OUT of scope here. State the limitation in the doc
        comment and make the browser's default mode the conservative one so the
        wrong guess is cheap.

    Tests \`tests/app_aware_formatting.rs\` + fixture rows carrying a \`mode\`
    field (the corpus already has \`"mode":"document"\` — group-a line 4 — so the
    axis exists; populate it).

    Depends on Y4-A, Y4-C.

    What NOT to do:
      - Do NOT read the browser URL in this item.
      - Do NOT make an unknown app default to Email. A sign-off appearing in a
        random text field is the kind of thing a user uninstalls over.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/app_aware_formatting.rs
    grep -q 'unknown_bundle_id_gets_the_conservative_mode' desktop/src-tauri/tests/app_aware_formatting.rs
    grep -q 'chat_mode_output_differs_from_email_mode_output' desktop/src-tauri/tests/app_aware_formatting.rs
    grep -q 'com.microsoft.VSCode' desktop/src-tauri/tests/app_aware_formatting.rs
    grep -q '"mode":"chat"' desktop/src-tauri/tests/fixtures/formatting/group-a-sentence-shape.jsonl
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test app_aware_formatting ; test $? -eq 0
    cargo test --features custom-protocol --test formatting_fixtures  ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y4-G', prompt: 'Y4', branch: 'loop/y4-g-formatting-is-visible-and-reversible', gated: null,
  title: 'The user can see what formatting did and undo it in one key — the trust mechanism',
  preflight: `
    grep -rq 'DiffView\\|formatting_diff' desktop/src
    cd ${APP} && npm ci && npm test -- diff
  `,
  spec: `
    Formatting that silently rewrites what you said is only acceptable if you
    can see it and revert it. Yap stores \`raw_text\` already (YV10/51) and has
    \`undo_ai_edit_text\` plus a \`UNDO_AI_EDIT\` global binding
    (shortcuts.rs:98). There is no diff UI:
    \`git grep -n "DiffView\\|diff" desktop/src\` -> nothing.
    reference_wispr_parity_research §2.6 lists "Diff/'see what changed' viewer +
    undo + feedback" as ✅ Wispr / 🟡 Yap, and P1 #8 says it is "mostly UI"
    because raw_text is already stored — and that Wispr "found this necessary
    for trust".

    Do:
      * A pure diff module \`desktop/src/diff.ts\` (word-level, no dependency —
        a small Myers or LCS implementation; do NOT add a library for this, the
        CSP forbids CDN loads and the bundle should not grow for a 60-line
        algorithm) returning a token list of {equal|added|removed}.
      * A "see what changed" panel in History and in the post-take surface,
        showing raw -> formatted, with the stages that ran named (dictionary /
        backtrack / rules / polish) so a surprising change is attributable to a
        stage. The stage list is already knowable from \`CleanupLevel\` and the
        polish outcome; thread it through as metadata on the take.
      * Undo restores BYTE-IDENTICAL raw text through the clipboard path. The
        parity backlog's acceptance for this is exactly that
        ("undo restores byte-identical raw text through the clipboard path") and
        it must be asserted, not assumed — the paste path is receipt-sequenced
        (YV39) and a naive re-paste can interleave.
      * Local-only thumbs up/down per take, stored in SQLite, used for nothing
        yet. It costs nothing now and is the only way a future rules change can
        be evaluated against real dissatisfaction. Never transmitted.

    Tests: \`desktop/src/diff.test.ts\` (pure, incl. identical strings -> all
    equal, and a pathological case that must not be quadratic-blow-up slow on a
    2,000-word take), and \`tests/undo_restores_raw.rs\` asserting byte-identical
    restoration including trailing whitespace and the signature block, which
    \`snippets::append_signature\` copies BYTE FOR BYTE after polish (lib.rs:283)
    and which undo must therefore also handle exactly.

    Depends on Y4-A.

    What NOT to do:
      - Do NOT add a diff library.
      - Do NOT send feedback anywhere. Local, forever. No Sentry, no PostHog.
      - Do NOT make undo re-run the pipeline. It replays a stored string.
    PANEL 2026-09-12 — two additions, and one word deleted.
    (a) THE SILENT-SKIP SIGNAL. Both polish failure modes are invisible by
        construction: a missed deadline returns err="deadline" and the parent
        falls back to rules text (polish.rs:299-300), and a max_out overrun
        discards the rewrite outright. As measured (Y4-I) those are the NORMAL
        outcomes for any take over ~150 words. So a user who installs a 1.12 GB
        model and sets High gets rules-only output on every real dictation with
        no indication the model was consulted — which is exactly how the current
        defect stayed invisible for a release cycle. Every take therefore
        records WHICH stages ran, and when the LLM stage was enabled but
        produced nothing, the REASON (deadline / max_out / no model / no
        sidecar). Show it in this item's diff view and once, quietly, on the
        pill. One counter per reason in the local log.
    (b) "the post-take surface" is deleted from the spec: THERE IS NO SUCH
        SURFACE. After \`done\` the pill is a small non-activating NSPanel with
        no text region (float_pill.rs:343-371) and no window appears. This item
        is scoped to History plus the existing ⌃⌘Z undo. An in-the-moment diff
        panel needs real geometry (size per dock, dismiss rules, how it avoids
        stealing focus from a non-activating panel, what happens if the user
        keeps typing) and is DEFERRED to docs/loop/DEFERRED.md rather than left
        as prose a builder has to invent.

  `,
  acceptance: `
    test -f desktop/src/diff.ts
    test -f desktop/src/diff.test.ts
    grep -rq 'formatting_diff\\|DiffView' desktop/src
    test -f desktop/src-tauri/tests/undo_restores_raw.rs
    grep -q 'undo_restores_byte_identical_raw_including_signature' desktop/src-tauri/tests/undo_restores_raw.rs
    # no new frontend dependency was added for the diff
    node -e "const d=require('./desktop/package.json').dependencies;process.exit(Object.keys(d).some(k=>/diff|jsdiff|myers/.test(k))?1:0)"
    cd ${APP} && npm ci
    npm test -- diff ; test $? -eq 0
    npx tsc --noEmit ; test $? -eq 0
    npm run build    ; test $? -eq 0
    cd src-tauri && cargo test --features custom-protocol --test undo_restores_raw ; test $? -eq 0    grep -q 'a_skipped_llm_stage_records_its_reason' desktop/src-tauri/tests/formatting_diff.rs
    grep -rq 'stages_that_ran' desktop/src-tauri/src

  `,
})

ITEMS.push({
  id: 'Y4-H', prompt: 'Y4', branch: 'loop/y4-h-formatting-settings-a-person-can-use', gated: null,
  title: 'The formatting settings stop being engineer words, and every one of them persists',
  preflight: `
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test settings_kv
    cargo test --features custom-protocol --test formatting_settings_persist
  `,
  spec: `
    Two problems, one screen.

    (1) THE NAMES. The formatting controls are \`cleanup_level\`
    (none|light|medium|high), \`polish_model\`, \`polish_deadline_ms\`,
    \`polish_styles\` (a per-mode map of very casual|casual|default|formal),
    \`snippet_scope\`, \`signature_mode\` (off|cue|auto). Every one of them is
    named for its implementation. A user cannot tell what "high" does, that
    "high" needs a downloaded model, or that \`polish_deadline_ms\` exists at all.
    Restructure the screen around what the user wants — how much should Yap
    clean up? should it use the local model? how should it sound per app? —
    with each option's one-line description derived from the same \`runs_*\`
    predicates Y4-A exposed, so copy cannot drift from behaviour. Controls
    first, prose last (feedback_think_ux_first).

    (2) PERSISTENCE. The 2026-07-24 senior audit's top user-facing bug was
    "half of Settings never persist (incl. PTT remap)" (project_yap_open_bugs),
    addressed as M6 in yap8. \`tests/settings_kv.rs\` exists. What does NOT exist
    is a test that every field of \`AppSettings\` round-trips. Write the
    exhaustive one: for each field, set a non-default value, restart the store,
    read it back, assert equality — and make it FAIL when a new field is added
    without being included, by asserting the field count against a constant
    that a new field forces you to update. A serde-driven round-trip over a
    fully-populated struct is the cheapest form of this; take that shape.
      * Include \`polish_styles\` (a BTreeMap — maps are the classic
        never-persisted field) and \`calibration_sample\` (an Option).
      * Assert \`schema_version\` migration: an old settings blob missing the new
        fields loads with the documented defaults and does not reset the fields
        it does carry.

    Tests \`tests/formatting_settings_persist.rs\` + keep settings_kv.rs green.

    Depends on Y4-A, SEC-C.

    What NOT to do:
      - Do NOT expose \`polish_deadline_ms\` as a raw millisecond number field. If
        it stays user-visible it is a named choice (Fast / Balanced / Careful)
        mapped to the existing bounded range (100..5000, polish.rs:64-65).
      - Do NOT drop any setting while renaming its label. Rename the LABEL, keep
        the KEY, or migrate it with the schema version.
      - Do NOT overinvest in the settings surface beyond making it correct and
        legible (feedback_dont_overinvest_settings).
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/formatting_settings_persist.rs
    grep -q 'every_app_settings_field_round_trips' desktop/src-tauri/tests/formatting_settings_persist.rs
    grep -q 'polish_styles' desktop/src-tauri/tests/formatting_settings_persist.rs
    grep -q 'field_count_forces_this_test_to_be_updated' desktop/src-tauri/tests/formatting_settings_persist.rs
    grep -q 'old_blob_migrates_without_resetting_carried_fields' desktop/src-tauri/tests/formatting_settings_persist.rs
    cd ${APP} && npm ci && npm run build ; test $? -eq 0
    cd src-tauri
    cargo test --features custom-protocol --test formatting_settings_persist ; test $? -eq 0
    cargo test --features custom-protocol --test settings_kv                 ; test $? -eq 0
  `,
})
