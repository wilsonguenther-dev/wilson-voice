// Y9 — THE P1 PARITY QUEUE: "closes the 'intelligence' gap without leaving the
// machine." Source of record:
// ~/Obsidian/Wilson-Brain/Notes/Wispr-Full-Parity-Research-2026-08-09.md §3 P1
// items 7-14, quoted verbatim per item below. Every ❌/🟡 score cited there was
// re-verified against origin/main @ 4e8c9adf by grep, and the greps are named.
//
// These are a separate file from Y8 because they touch the polish sidecar, the
// AX layer and the dictionary — not the pill — so the two groups can run on
// opposite lanes without conflicting.
//
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.
// Depends on Y4 (the formatting stage must actually run before any of this is
// observable) — sequence this file after 20-y4.

ITEMS.push({
  id: 'Y9-A', prompt: 'Y9', branch: 'loop/y9-a-named-transform-library', gated: null,
  title: 'A named transform library over the existing sidecar, with an observable status enum',
  preflight: `
    test -f desktop/src-tauri/src/transforms.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test transforms
  `,
  spec: `
    P1 #7, VERBATIM: "Transform library. (M · L) Named prompts (built-ins:
    *Polish*, *Prompt Engineer*, *Concise*, *Formal*), each with an optional
    shortcut, run through the existing \`yap-polish\` sidecar. Persist the
    Wispr-shaped status enum (\`succeeded | timeout | error | no_changes |
    not_editable | …\`) so failures are observable rather than silent. Accept
    (L): golden fixtures per transform; a forced sidecar timeout yields
    \`status=timeout\` **and the original text unchanged** (never-lose-text)."

    VERIFIED ABSENT: \`git grep "transform_library\\|named_transform"
    -- desktop\` -> 0. The teardown scores "LLM transform over selection" 🟡
    "sidecar exists (YV60/61), not exposed as a transform library" and "Named
    transforms w/ per-transform hotkeys" ❌.

    What DOES exist and must be reused: YV49's nine pure deterministic Rust
    transforms over a selection (\`command_mode.rs\`, 625 lines — the teardown
    scores those ✅), and the whole validated polish stage
    (\`polish.rs\`: \`PolishClient\` seam, \`validate_polish\` V1-V7,
    \`SidecarPool\`, the 1200 ms deadline).

    Do:
      * \`desktop/src-tauri/src/transforms.rs\`: a TABLE of named transforms
        (id, label, prompt, whether it is deterministic or model-backed),
        so a new transform is a row and not a new code path. The four built-ins
        above, plus the nine existing deterministic ones registered in the same
        table so there is ONE list the UI reads.
      * Model-backed transforms go through \`polish::polish_stage\` — the SAME
        validator, the SAME deadline, the SAME never-lose-text rule. A transform
        must not be a second, unvalidated path to the pasteboard.
      * The status enum, persisted per invocation:
        \`succeeded | no_changes | timeout | rejected | error | not_editable\`.
        \`rejected\` is Yap-specific and important: it is what
        \`validate_polish\` returning None means, and conflating it with \`error\`
        would hide the validator's work.
      * Per-transform shortcuts are registered through Y8-A's validated table.
      * Golden fixtures per transform under
        \`tests/fixtures/transforms/*.jsonl\`, following the conventions in
        tests/fixtures/README.md.

    Tests \`tests/transforms.rs\`, using polish.rs's injectable clients:
      * a golden fixture per transform
      * a forced timeout yields \`timeout\` AND byte-identical input
      * a garbage-returning client yields \`rejected\` AND byte-identical input
      * a panicking client yields \`error\` AND byte-identical input
      * \`no_changes\` when the model returns the input
      * every table row has a label and a non-empty prompt

    Depends on SEC-C (a model to run), Y8-A (the shortcut table).

    What NOT to do:
      - Do NOT add a second path to the pasteboard that skips \`validate_polish\`.
      - Do NOT let a transform silently fail. The enum exists so it cannot.
  `,
  acceptance: `
    test -f desktop/src-tauri/src/transforms.rs
    test -d desktop/src-tauri/tests/fixtures/transforms
    test -f desktop/src-tauri/tests/transforms.rs
    grep -q 'not_editable' desktop/src-tauri/src/transforms.rs
    grep -q 'rejected' desktop/src-tauri/src/transforms.rs
    grep -q 'a_forced_timeout_leaves_the_input_byte_identical' desktop/src-tauri/tests/transforms.rs
    grep -q 'a_garbage_client_yields_rejected_not_error' desktop/src-tauri/tests/transforms.rs
    grep -q 'every_table_row_has_a_label_and_a_prompt' desktop/src-tauri/tests/transforms.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test transforms ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y9-B', prompt: 'Y9', branch: 'loop/y9-b-writing-samples-local-style-profile', gated: null,
  title: 'Writing samples become a local style profile injected into the polish prompt',
  preflight: `
    grep -q 'writing_samples' desktop/src-tauri/src/db.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test style_profile
  `,
  spec: `
    P1 #9, VERBATIM: "Writing samples → local style profile. (M · L) A
    \`user_context(writing_samples, custom_rules)\` table, samples pasted by the
    user, injected as few-shot context into the polish prompt. **Fully local —
    Wispr sends yours to Baseten.** Accept (L): the built prompt contains the
    samples; with zero samples the prompt is byte-identical to today's (no
    regression on the fixture corpus)."

    VERIFIED ABSENT: \`git grep "writing_samples" -- desktop\` -> 0. The
    teardown's §2.5 row: Wispr's \`UserContext.writingSamples\` with "max N, min
    word count, 'paste an email you've written'" feeding their Polish prompt;
    Yap ❌.

    Do:
      * A \`user_context\` table (migration, idempotent) with writing samples and
        free-text custom rules. Bounded: a max sample count and a max total
        length, both named constants, because the prompt has a token budget
        (\`polish_protocol.rs\`'s \`max_out_for\` already reasons about budgets —
        read it and respect the same accounting).
      * A settings surface to paste samples in, with the min-word-count guard
        Wispr uses (a two-word "sample" is noise in a few-shot prompt).
      * Injected into \`polish::build_request\` as few-shot context. THE
        REGRESSION GUARD IS THE ACCEPTANCE: with zero samples the built request
        must be byte-identical to today's, so the whole existing formatting
        corpus is untouched. Assert that directly against a serialized request.
      * Samples are user text: they must be excluded from the support bundle and
        from every log, like transcripts already are
        (tests/support_bundle_redaction.rs). Extend that test.
      * They never leave the machine — PRIV-A's outbound sweep must still pass,
        and the samples table must be named in PRIVACY.md.

    Tests \`tests/style_profile.rs\`: zero samples -> byte-identical request;
    one sample -> it appears in the request; over-budget samples are truncated
    deterministically (oldest dropped, said in the doc comment); samples never
    appear in a support bundle.

    Depends on SEC-C, PRIV-A.

    What NOT to do:
      - Do NOT send a sample anywhere.
      - Do NOT let samples grow the prompt past the budget — a silently
        truncated prompt is a silently different formatter.
      - Do NOT include samples in a crash report.
  `,
  acceptance: `
    grep -q 'writing_samples' desktop/src-tauri/src/db.rs
    grep -q 'user_context' desktop/src-tauri/src/db.rs
    test -f desktop/src-tauri/tests/style_profile.rs
    grep -q 'zero_samples_yields_a_byte_identical_request' desktop/src-tauri/tests/style_profile.rs
    grep -q 'samples_never_appear_in_a_support_bundle' desktop/src-tauri/tests/style_profile.rs
    grep -q 'writing samples' PRIVACY.md
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test style_profile           ; test $? -eq 0
    cargo test --features custom-protocol --test formatting_fixtures     ; test $? -eq 0
    cargo test --features custom-protocol --test support_bundle_redaction ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y9-C', prompt: 'Y9', branch: 'loop/y9-c-spoken-preference-rules', gated: null,
  title: 'Spoken preference rules: say a rule once, it applies where it matches — with an explicit Apply step',
  preflight: `
    grep -q 'voice_preferences' desktop/src-tauri/src/db.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test voice_preferences
  `,
  spec: `
    P1 #10, VERBATIM: "Spoken preference rules (the \`UserVoicePreferences\`
    idea). (M · L) Let the user say 'never use exclamation marks in email' and
    store \`(preference, filter)\` where filter is \`app:*\` / \`mode:email\` /
    \`lang:*\`; apply matching rules to the polish prompt. Requires an explicit
    **Apply** step (Wispr does this too) so nothing changes behaviour silently.
    Accept (L): rule with \`mode:email\` alters the email fixture output and
    leaves the chat fixture unchanged."

    The teardown calls this "the single most interesting mechanic found" and
    quotes the competitor's own migration doc comment as the source
    (§2.5, [BUNDLE] \`20260528120000-create-user-voice-preferences-table.js\`):
    append-only spoken rules — "never use 'awesome' in German", "sound formal
    with John in Gmail" — turned into structured filter strings like
    \`app:gmail\`, \`language:de\`. And: "fully implementable locally"
    (memory index line on this reference).

    VERIFIED ABSENT: \`git grep "user_voice_pref" -- desktop\` -> 0.

    Do:
      * A \`voice_preferences\` table, APPEND-ONLY: the raw spoken sentence, the
        derived filter, the derived rule text, a timestamp, and an active flag.
        Append-only matters — it is what lets a user see what they said and
        retract it, rather than trusting an opaque profile.
      * Derivation is LOCAL: the spoken sentence goes through the polish sidecar
        with a dedicated prompt that emits a structured
        \`{filter, rule}\`, validated against an allowlist of filter shapes
        (\`app:<bundle-id>\` / \`mode:<one of the six>\` / \`lang:<code>\` / \`*\`).
        An unparseable rule is REJECTED and shown back to the user, never
        stored half-derived.
      * AN EXPLICIT APPLY STEP. The derived rule is shown — "I heard: never use
        exclamation marks. I'll apply it to: email" — and only takes effect when
        the user confirms. The teardown is explicit that this is required so
        nothing changes behaviour silently, and a dictation app that silently
        rewrites your voice on a misheard sentence is the worst version of this
        feature.
      * Matching rules are appended to the polish prompt for takes whose
        mode/app/language match. Bounded, sharing Y9-B's token budget.

    Tests \`tests/voice_preferences.rs\`:
      * the acceptance verbatim: a \`mode:email\` rule alters the email fixture
        output and leaves the chat fixture unchanged
      * an unparseable rule is rejected and not stored
      * a filter outside the allowlist is rejected
      * no rule takes effect before Apply
      * retracting a rule stops its effect and leaves the append-only history
      * zero active rules -> byte-identical request (the Y9-B guard again)

    Depends on Y9-B (the budget accounting and the request-identity test).

    What NOT to do:
      - Do NOT apply a rule before the user confirms it.
      - Do NOT delete rows on retract. Append-only means append-only.
      - Do NOT let a rule reach the prompt without passing the filter allowlist.
  `,
  acceptance: `
    grep -q 'voice_preferences' desktop/src-tauri/src/db.rs
    test -f desktop/src-tauri/tests/voice_preferences.rs
    grep -q 'a_mode_email_rule_alters_email_and_leaves_chat_unchanged' desktop/src-tauri/tests/voice_preferences.rs
    grep -q 'no_rule_takes_effect_before_apply' desktop/src-tauri/tests/voice_preferences.rs
    grep -q 'a_filter_outside_the_allowlist_is_rejected' desktop/src-tauri/tests/voice_preferences.rs
    grep -q 'retract_leaves_the_append_only_history' desktop/src-tauri/tests/voice_preferences.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test voice_preferences   ; test $? -eq 0
    cargo test --features custom-protocol --test formatting_fixtures ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'PERM-F', prompt: 'Y9', branch: 'loop/perm-f-deeper-ax-context-selection-and-after-caret', gated: null,
  title: 'Deeper accessibility context: the selection and the text after the caret, read in-process',
  preflight: `
    grep -q 'AXSelectedText' desktop/src-tauri/src/focus.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test ax_context
  `,
  spec: `
    P1 #11, VERBATIM: "Deeper AX context: selected + after-caret + app URL.
    (M · L) Yap reads before-caret today (YV50). Add \`AXSelectedText\`, text
    after the caret, and (for browsers) the focused window's URL, all consumed
    **in-process**. Accept (L): harness reads a known selection from a test
    target; absent AX permission the pipeline no-ops to today's path."

    MEASURED: \`git grep "AXSelectedText" -- desktop\` -> ONE hit, and it is a
    comment, not a call. The teardown's §2.4 scores "Selected text + text after
    caret" 🟡 "selection only in Command Mode" and "Browser URL resolution" ❌.

    THE PRIVACY FRAME IS THE POINT and must be in the PR body: the teardown's
    §2.4 table shows the competitor sends \`textbox_contents\`, \`axText\`,
    \`axHTML\` and a base64 screenshot to their API. Yap reads the same AX fields
    and they "die in a Rust function" (§5.2). This item widens what Yap reads,
    so it also widens the promise it has to keep.

    Do:
      * \`focus.rs\` gains \`AXSelectedText\` and after-caret text, alongside the
        existing before-caret read (YV50). In-process, no helper, no subprocess
        — the bundle stays the only TCC row (permissions.rs:4-6 explains why
        that matters).
      * Browser URL resolution for the focused window, for the three big
        browsers, used ONLY to disambiguate the dictation mode (Y4-F left this
        as a named limitation — this item closes it). Never stored, never
        logged, never in a support bundle, and never sent.
      * NO-OP WITHOUT THE GRANT. Absent Accessibility, the pipeline must behave
        exactly as it does today — asserted, not assumed, because a new AX read
        on a path that previously did not need one is a new way for a
        permission failure to break dictation.
      * Bound the reads: a huge AX field (a whole document after the caret) must
        be truncated to a named budget before it touches the prompt.
      * PRIV-A's outbound sweep must still pass, and PRIVACY.md must name the
        three new fields and the URL, with the sentence that they never leave.

    Tests \`tests/ax_context.rs\`: the no-grant no-op is byte-identical; an
    oversized field is truncated to the budget; the URL never reaches the DB, a
    log or a support bundle; the browser-mode disambiguation resolves Gmail to
    email and a docs URL to document.

    Depends on PERM-A/PERM-E (the grant model), Y4-F (the mode table), PRIV-A.

    What NOT to do:
      - Do NOT read \`axHTML\` or take a screenshot. The teardown flags both as
        the competitor's uploads and Yap's local promise makes them a liability
        (§2.4 marks screen OCR "❌ ➖ would break the local promise").
      - Do NOT store the URL.
      - Do NOT make the dictation path depend on a successful AX read.
  `,
  acceptance: `
    grep -q 'AXSelectedText' desktop/src-tauri/src/focus.rs
    test -f desktop/src-tauri/tests/ax_context.rs
    grep -q 'without_the_grant_the_pipeline_is_byte_identical' desktop/src-tauri/tests/ax_context.rs
    grep -q 'an_oversized_ax_field_is_truncated_to_the_budget' desktop/src-tauri/tests/ax_context.rs
    grep -q 'the_url_never_reaches_the_db_a_log_or_a_bundle' desktop/src-tauri/tests/ax_context.rs
    test 0 -eq "$(grep -c 'axHTML' desktop/src-tauri/src/focus.rs)"
    grep -q 'after the caret' PRIVACY.md
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test ax_context                        ; test $? -eq 0
    cargo test --features custom-protocol --test no_outbound_on_the_dictation_path ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'PERM-G', prompt: 'Y9', branch: 'loop/perm-g-vibe-coding-identifier-bias', gated: null,
  title: 'IDE context: the identifiers in the open file bias the transcription — the highest personal-ROI item',
  preflight: `
    grep -q 'ide_identifiers\\|vibe_context' desktop/src-tauri/src/vocab.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test ide_bias
  `,
  spec: `
    P1 #12, VERBATIM: "Vibe-coding context. (M · L) File tagging + variable
    recognition for VS Code / Cursor / Windsurf, via the IDE's screen-reader AX
    mode — exactly Wispr's mechanism, but the terms never leave the box. Feed
    identifiers into the whisper \`initial_prompt\` bias (the YV47 machinery
    already exists). Why: Wilson lives in Claude Code / Cursor; this is the
    highest personal-ROI item on the list. Accept (L): with a file open
    containing \`learn_from_transcript_tx\`, that identifier appears in the built
    bias prompt and is transcribed correctly in a fixture WAV."

    The machinery is genuinely all there:
      \`asr_engine.rs:37-49\` — the bias window, joined with ", ", "the shape
      upstream Handy feeds \`initial_prompt\`", with an explicit note that
      "overflowing it would silently drop the terms at the head".
      \`asr_engine.rs:549\` — "200 twelve-character terms is ~2.6 KB — far past
      the window", so the budget is already measured.
      \`vocab.rs\`, \`vocab_extract.rs\` — the YV47 dictionary/bias machinery.
      \`focus.rs\` — the frontmost app, and PERM-F's AX reads.

    Do:
      * When the frontmost app is VS Code, Cursor or Windsurf, read the open
        file's visible text via AX and extract IDENTIFIERS (snake_case,
        camelCase, PascalCase, SCREAMING_SNAKE tokens over a length floor).
      * Rank and TRUNCATE to the measured bias window. The window comment is
        explicit that overflow silently drops the terms at the HEAD, so ranking
        matters: identifiers near the caret first. Assert the truncation is
        deterministic and that the window is never exceeded — a silently
        dropped bias is a silently worse transcript.
      * Merge with the user's dictionary terms without either starving the
        other: a named split of the window between dictionary terms and IDE
        identifiers, stated in the doc comment.
      * Fixture test with the exact acceptance above:
        \`learn_from_transcript_tx\` in the open file appears in the built bias
        prompt. Use a committed synthetic "open file" fixture, not a live IDE.
      * The identifiers never leave the machine and never enter the DB, a log or
        a support bundle. They are the most sensitive thing on this list — they
        are the user's source code.
      * No-op without AX, and no-op for any app not on the IDE list.

    Tests \`tests/ide_bias.rs\`: the named identifier appears; the window is
    never exceeded; truncation is deterministic and caret-proximate;
    identifiers never reach the DB or a bundle; a non-IDE frontmost app
    contributes nothing; no-grant is byte-identical.

    Depends on PERM-F.

    What NOT to do:
      - Do NOT store the identifiers.
      - Do NOT exceed the bias window. Measure it from the constant, do not
        guess it.
      - Do NOT read a file from disk. AX only — reading the project off disk is
        a different and much larger promise.
  `,
  acceptance: `
    grep -qE 'ide_identifiers|vibe_context' desktop/src-tauri/src/vocab.rs
    test -f desktop/src-tauri/tests/ide_bias.rs
    grep -q 'learn_from_transcript_tx' desktop/src-tauri/tests/ide_bias.rs
    grep -q 'the_bias_window_is_never_exceeded' desktop/src-tauri/tests/ide_bias.rs
    grep -q 'truncation_is_deterministic_and_caret_proximate' desktop/src-tauri/tests/ide_bias.rs
    grep -q 'identifiers_never_reach_the_db_or_a_bundle' desktop/src-tauri/tests/ide_bias.rs
    grep -q 'a_non_ide_frontmost_app_contributes_nothing' desktop/src-tauri/tests/ide_bias.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test ide_bias                 ; test $? -eq 0
    cargo test --features custom-protocol --test asr_capabilities_probe   ; test $? -eq 0
    cargo test --features custom-protocol --test vocab_corpus_is_literals_only ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y9-D', prompt: 'Y9', branch: 'loop/y9-d-blocked-apps-and-focus-denylist', gated: null,
  title: 'A denylist where the hotkey is inert — the honest complement to reading your context',
  preflight: `
    grep -q 'denylist' desktop/src-tauri/src/focus.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test denylist
  `,
  spec: `
    P1 #14, VERBATIM: "Blocked apps / blocked URLs. (S · L) A denylist where the
    hotkey is inert. Why: the honest complement to 'we read your context' — and
    a natural home for password managers and banking sites. Accept (L):
    frontmost app on the denylist → PTT records nothing and shows a muted-bar
    state."
    And P2 #21: "Focus Mode. (M · L) \`opt+F\` blocks distracting apps/sites.
    Adjacent to #14; ships on the same denylist plumbing."

    VERIFIED ABSENT: \`git grep "blocked_apps\\|denylist\\|blocklist" -- desktop\`
    -> 0. The teardown scores it ❌ against Wispr's Experimental → Blocked Apps /
    Blocked URLs.

    Do:
      * A denylist of bundle identifiers and (with PERM-F's URL read) URL
        patterns. SEEDED with sensible defaults the user can remove: the common
        password managers. A denylist that ships empty is a feature nobody turns
        on.
      * On a denylisted frontmost app, the PTT is INERT: no capture is started,
        nothing is written, and the pill shows a muted state (Y5-C's vocabulary
        — reuse \`blocked\`, or add \`muted\` if the distinction reads better;
        decide and say why). Never a silent no-op, which is indistinguishable
        from a broken hotkey.
      * The check runs in Y1/PERM-C's single gate in \`start_recording\`
        alongside the microphone and license checks, in a stated order. Three
        gates in one place, one refusal reason each.
      * Secure-input fields are ALREADY refused by \`secure_input.rs\` at PASTE
        time (Y6-C tests it). The denylist is the complement at RECORD time.
        State the difference in the doc comment so the two are not merged.
      * Focus Mode rides the same list with an inverted sense and its own
        binding through Y8-A's table. Ship the plumbing; the blocking behaviour
        is a small step once the list exists.

    Tests \`tests/denylist.rs\`: a denylisted app starts no recorder; the pill
    receives a muted state; the default seed is non-empty; a URL pattern matches
    only with PERM-F's read available and no-ops without it; the three gates
    refuse in the stated order.

    Depends on PERM-C, PERM-F, Y5-C, Y8-A.

    What NOT to do:
      - Do NOT record and then discard. Nothing may be captured.
      - Do NOT ship an empty default list.
      - Do NOT log the denylisted app's name with the user's context attached.
  `,
  acceptance: `
    grep -q 'denylist' desktop/src-tauri/src/focus.rs
    test -f desktop/src-tauri/tests/denylist.rs
    grep -q 'a_denylisted_app_starts_no_recorder' desktop/src-tauri/tests/denylist.rs
    grep -q 'the_default_seed_is_not_empty' desktop/src-tauri/tests/denylist.rs
    grep -q 'the_three_gates_refuse_in_the_stated_order' desktop/src-tauri/tests/denylist.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test denylist ; test $? -eq 0
    cargo test --features custom-protocol --test mic_gate ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y9-E', prompt: 'Y9', branch: 'loop/y9-e-dictionary-and-snippet-bulk-io', gated: null,
  title: 'CSV round-trip for the dictionary and snippets, and the usage-frequency ranking that is only half there',
  preflight: `
    grep -q 'import_csv' desktop/src-tauri/src/db.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test bulk_io
  `,
  spec: `
    P1 #13, VERBATIM: "Bulk import/export (CSV) for dictionary + snippets.
    (S · L) Round-trip test."
    Plus the two 🟡s from the same §2.5 table, which belong with it:
    "Usage-frequency ranking 🟡" and "Replacement rules incl. HTML replacement
    🟡 plain text".

    VERIFIED ABSENT: \`git grep "csv_import\\|bulk_import" -- desktop\` -> 0.

    Do:
      * CSV import and export for the dictionary and for snippets, with a strict
        parser: a documented column set, a row that fails validation is
        REPORTED with its line number and skipped, and the import is atomic
        per-file (either the valid rows all land or none do — a half-imported
        dictionary is worse than a failed one). Write the parser; do not add a
        CSV dependency for a comma-splitter you can test.
      * Quoting and escaping are the whole difficulty: a replacement value
        containing a comma, a quote, a newline. Round-trip every one of those in
        the test, which is the acceptance the teardown asks for.
      * Usage-frequency ranking: the dictionary already learns from corrections
        (YV47) — add the frequency column and use it to rank the bias terms fed
        to PERM-G's window, where ranking now matters because the window
        overflows. That makes the 🟡 a ✅ and improves the transcript.
      * Rich-text/HTML snippet replacement is P2 #18 and is NOT in this item:
        the teardown notes it "interacts with the YV39 receipt-sequenced paste —
        needs its own slice". Say so in the doc comment so it is not
        half-attempted here.

    Tests \`tests/bulk_io.rs\`: round-trip with commas, quotes and newlines in
    values; a malformed row is reported with its line number; the import is
    atomic; export contains no license key and no home path; frequency ranking
    orders the bias terms.

    Depends on PERM-G (the ranking consumer).

    What NOT to do:
      - Do NOT add an xlsx or CSV library.
      - Do NOT half-import on a bad row.
      - Do NOT attempt HTML/rich-text replacement here.
  `,
  acceptance: `
    grep -q 'fn import_csv' desktop/src-tauri/src/db.rs
    grep -q 'fn export_csv' desktop/src-tauri/src/db.rs
    test -f desktop/src-tauri/tests/bulk_io.rs
    grep -q 'round_trips_commas_quotes_and_newlines' desktop/src-tauri/tests/bulk_io.rs
    grep -q 'a_malformed_row_is_reported_with_its_line_number' desktop/src-tauri/tests/bulk_io.rs
    grep -q 'the_import_is_atomic' desktop/src-tauri/tests/bulk_io.rs
    node -e "const d=require('./desktop/package.json').dependencies;process.exit(Object.keys(d).some(k=>/csv|xlsx|papaparse/.test(k))?1:0)"
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test bulk_io  ; test $? -eq 0
    cargo test --features custom-protocol --test ide_bias ; test $? -eq 0
  `,
})
