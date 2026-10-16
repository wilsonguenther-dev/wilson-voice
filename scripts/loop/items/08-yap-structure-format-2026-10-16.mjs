// yap-format-structure — WISPR-STYLE STRUCTURE FROM SPEECH. Written 2026-10-16 by the
// yap-format-item seat. Wilson dictated a long review by voice today; Wispr Flow turned it into an
// intro paragraph, a bulleted list, prose, and a second list of "topic: verdict" bullets. His words:
//   "I just use Wispr Flow for that. You see how it does the formatting? That's what Yap needs to be
//    able to do. It knew I was doing a list of things."
// Yap pastes the same take as flat prose. FIX, no panel (unambiguous intent, Wispr parity).
// ORDER: this file sorts after 08-yap-perm-hotkey (PR #208) and BEFORE 08-yap-uiux, so this item runs
// ahead of yap-uiux-2026-10-16-W0 — the same convention as PR #208.
// WHY A NEW ITEM, NOT AN EXTENSION OF Y4: every Y4 item (10-y4-formatting.mjs) is MERGED or
// ALREADY-DONE and retires at pre-flight. Y4-D proves EXPLICIT-cue lists only ("one, two, three",
// "first/second", the word "bullet": dictation.rs detect_and_format_list ~:1776, test
// list_bullets_only_on_explicit_cue) and guards against over-eager bullets; Y4-C paragraphs at pauses
// (insert_paragraphs ~:2606); Y4-F maps apps to six DictationModes (mode_for_app ~:65). None of them
// INFERS structure from parallel clauses or "topic: verdict" speech, and none emits Markdown per
// target. This item EXTENDS those modules (same pipeline, same fixture discipline) — it never builds
// a parallel formatter.
// Review: Opus only (review-opus + review-opus2), ADVERSARIAL on "did it invent content". No Codex,
// no Gemini, no third-party reviewer. SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.
//
// HOW YAP POLISHES TODAY (main, 2026-10-16, read-only):
//   desktop/src-tauri/src/dictation.rs run_cleanup (~:856): dictionary -> clean_backtrack ->
//     format_dictation (~:123, explicit-cue list detection only, else prose) -> apply_spoken_marks ->
//     format_email_shape (Email) -> insert_paragraphs -> trailing-period rule -> optional LLM polish.
//   desktop/yap-polish/src/main.rs SYSTEM_BASE (~:62) + mode_overlay (~:80): a llama.cpp rewriter
//     that "preserves existing line breaks and list numbering"; it never infers structure, and with
//     the shipped polish_model "" (lib.rs ~:476) it never runs at all. Paste is clipboard + synthetic
//     Cmd-V (paste.rs), so newlines in a paste never SEND a chat message.

ITEMS.push({
  id: 'yap-format-structure', prompt: 'yap-format-structure', branch: 'loop/yap-format-structure-wispr-style-lists-and-paragraphs', gated: null,
  title: 'Wispr-style structure from speech: enumerations become lists, topic-then-verdict becomes "Topic: verdict" bullets, topic shifts become paragraphs, formatted for the paste target',
  preflight: `
    test -f desktop/src-tauri/src/structure.rs
    grep -q 'src-tauri/src/structure.rs' desktop/yap-polish/src/main.rs
    test 8 -le "$(ls -d desktop/yap-polish/fixtures/structure/*/ | wc -l)"
    grep -q 'fn structure_fixture_01_two_lists_and_topic_verdict_bullets' desktop/src-tauri/src/structure.rs
  `,
  spec: `
    Panel: not required — FIX (Wilson 2026-10-16: "That's what Yap needs to be able to do. It knew I
    was doing a list of things.")
    DEPENDS: a green main; Y4-C, Y4-D, Y4-F (all on main). Runs ahead of yap-uiux-2026-10-16-W0.

    GOAL (Wilson): dictate a review the way he talks and get what Wispr Flow gives him — the lists
    are lists, the "this thing: verdict" lines are bullets, the topic shifts are paragraphs — pasted
    in the form the app he is in actually renders.

    WHERE IT LIVES (one module, two crates)
      - desktop/src-tauri/src/structure.rs: a PURE, rules-only, deterministic module. No LLM, no
        clock in the output path, no randomness, no HashMap iteration order in the output. It runs in
        run_cleanup right after format_dictation and before insert_paragraphs (insert_paragraphs must
        never break inside a list it produced), at the shipped level (Medium) and with NO polish model
        installed. Y4-D stays law: list detection is never LLM-dependent.
      - desktop/yap-polish/src/main.rs includes it with
        #[path = "../../src-tauri/src/structure.rs"] mod structure;  — the exact pattern already used
        for polish_protocol.rs — so the module is compiled and tested in BOTH crates and the sidecar
        can re-apply structure if the LLM flattens it. Add a SYSTEM_BASE rule "Preserve every bullet
        and numbered line exactly" (system_prompt_is_stable_per_mode_and_style must stay green). The
        module therefore depends only on std (no serde, no crate-local paths).
      - Tests live INSIDE structure.rs (#[cfg(test)] mod tests, every fn named structure_*) because a
        bin-only crate has no integration-test surface. Fixtures are read from
        concat!(env!("CARGO_MANIFEST_DIR"), "/../yap-polish/fixtures/structure") — a path that
        resolves from both crates because they are siblings under desktop/.

    RULES
      1. Enumerations become bulleted lists. Cues: "one, two, three"; "first / second / then / also /
         and then"; three or more parallel clauses or noun phrases in one run ("the security, the data
         pulling, the telemetry"); repeated sentence openers ("make sure X. make sure Y."); a topic
         followed by a colon-like pause (a PauseSpan, or an ASR comma/period) then a verdict.
      2. Numbered list when order matters ("first ... then ... finally", "step one").
      3. Paragraph breaks at topic shifts and after a verdict sentence that closes a list; reuse
         insert_paragraphs for pause-based breaks, do not duplicate it.
      4. "Topic: comment" bullets when he names a thing and then judges it:
         "Account and licence, perfect." -> "- Account and licence: perfect."
      5. Headings ONLY when he says "heading" or "title" explicitly; never invented.
      6. Target-aware output: pub fn structure_format_for(bundle_id: &str, setting: &str) ->
         StructureFormat { Markdown, PlainBullets, Off }.
           Markdown ("- ", "1. ", "## "): editors, notes, Slack, Notion-like — com.tinyspeck.slackmacgap,
             notion.id, md.obsidian, com.apple.Notes, com.microsoft.Word; a browser only when the user
             setting says so (browser = ambiguous, the Y4-F conservative rule).
           PlainBullets ("• " or "- " with real newlines, never "#" or "**"): com.apple.mail,
             com.apple.MobileSMS, com.microsoft.Outlook, and every unknown text field (the
             conservative default).
           Off: code editors and terminals (com.microsoft.VSCode, com.apple.Terminal,
             com.googlecode.iterm2, dev.warp.Warp-Stable, Cursor, com.apple.dt.Xcode) unless a mode is
             set; DictationMode::Code is always Off.
         Chat: Y4-C/Y4-F say chat gets no paragraph breaks because a typed Return sends. Paste is
         clipboard + Cmd-V (paste.rs), which does not send, so a list in Slack pastes as a list. Keep
         the no-blank-line rule for chat, allow single-newline list items, and pin it with a test.
      7. Never change his words beyond what polish already does: no new content, no summaries, no
         reordering, no dropped words except the cue words consumed as markers ("first", "second",
         "next", "number one", "bullet", "heading", "title"), kept in ONE named const list.
      8. Settings: a global "Structure" switch (default ON) and a per-app override list in
         desktop/src/views/settings/Dictation.tsx, persisted in the settings struct in lib.rs
         (structure_enabled: bool, structure_app_overrides keyed by bundle id), read in run_cleanup.
         Plain words on the control ("Turn spoken lists into bullets"), no engineer words (Y4-H).
      9. Latency: structure inference adds <= 150 ms p95 on a 400-word take on Apple Silicon (the lane
         machine is Wilson's M-series Mac). Measure 50 runs over the 400-word fixture, print p95.
     10. Deterministic: same transcript (+ pauses + target) -> byte-identical output.

    FIXTURES  desktop/yap-polish/fixtures/structure/NN-name/{input.txt, target.txt, expected.md,
      pauses.json (optional)} — at least 8 folders. target.txt holds a bundle id. Required:
      01-wilson-review-2026-10-16 (below); 02 numbered steps "first ... then ... finally";
      03 "make sure X / make sure Y" parallel openers; 04 topic-then-verdict alone; 05 explicit
      "heading"; 06 a 400-word take (the latency fixture); and two NEGATIVE controls that must come out
      as unchanged prose: 07 a narrative "and then we drove, and then we ate" (a story, not a list),
      08 fixture 03's text with target com.microsoft.VSCode (Off). A single-item "list" must not
      become a list (Y4-D single_item rule). Invented, bland text for 02-08; never real dictation.

      FIXTURE 01 — Wilson's review, ABRIDGED exactly as quoted from Wispr's output on 2026-10-16. The
      intro and middle prose paragraphs were not available to the item writer and are left out rather
      than invented. input.txt (flat, as Yap's ASR hands it to the rules stage):
        the security, the data pulling, the telemetry, the health, constant-time health checks.
        First run permissions, make sure … Mic test, all of those. Account and licence, perfect.
      target.txt: notion.id
      expected.md:
        - the security
        - the data pulling
        - the telemetry
        - the health
        - constant-time health checks

        - First run permissions: make sure …
        - Mic test, all of those.
        - Account and licence: perfect.

    TESTS (all in structure.rs, all named structure_*)
      structure_fixture_01_two_lists_and_topic_verdict_bullets — at least 2 lists, the "Topic: verdict"
        lines present
      structure_golden_fixtures_match — every folder, input + target -> expected.md, byte-equal
      structure_is_deterministic_three_runs — every fixture 3x, outputs identical
      structure_latency_p95_under_150ms — prints "structure p95_ms=<n>", fails above 150
      structure_never_invents_content_words — for every fixture: the lowercase alphanumeric words of
        the output minus list markers are a sub-multiset of the input words, AND every non-cue input
        word appears in the output (nothing dropped). The review's adversarial check; also run it over
        seeded (fixed seed) shuffles of the fixture sentences.
      structure_format_for_bundle_id — the rule-6 table, incl. unknown -> PlainBullets, code -> Off
      structure_chat_paste_has_no_blank_lines
      structure_single_item_is_not_a_list, structure_narrative_and_then_is_not_a_list
      Settings: extend tests/formatting_settings_persist.rs with structure_settings_persist (global
      switch + one per-app override round-trip). UI: a vitest for Dictation.tsx asserting the switch
      renders and calls the setter if the repo has a view test harness; otherwise one line in
      docs/loop/MANUAL-WALK.md (create if absent): "Settings > Dictation > Turn spoken lists into
      bullets: off -> dictate fixture 02 into Notes -> flat prose; on -> numbered list".

    WHAT NOT TO DO
      - Do NOT route structure through the LLM or make it depend on polish_model.
      - Do NOT relax any existing formatting_fixtures / app_aware_formatting / paragraphing expectation
        to make room; a conflict is a finding to report, not a row to edit.
      - Do NOT add words, summaries, or headings he did not say.
      - Do NOT launch the GUI app or touch Wilson's installed /Applications/Yap.app.
  `,
  acceptance: `
    test 8 -le "$(ls -d desktop/yap-polish/fixtures/structure/*/ | wc -l)"
    grep -q 'Account and licence' desktop/yap-polish/fixtures/structure/01-wilson-review-2026-10-16/input.txt
    grep -q 'Account and licence: perfect' desktop/yap-polish/fixtures/structure/01-wilson-review-2026-10-16/expected.md
    grep -q 'src-tauri/src/structure.rs' desktop/yap-polish/src/main.rs
    for t in structure_fixture_01_two_lists_and_topic_verdict_bullets structure_golden_fixtures_match structure_is_deterministic_three_runs structure_latency_p95_under_150ms structure_never_invents_content_words structure_format_for_bundle_id structure_single_item_is_not_a_list structure_narrative_and_then_is_not_a_list; do grep -q "fn $t" desktop/src-tauri/src/structure.rs || exit 1; done
    grep -q 'structure_settings_persist' desktop/src-tauri/tests/formatting_settings_persist.rs
    grep -qi 'structure' desktop/src/views/settings/Dictation.tsx
    cd ${APP} && npm ci
    test 8 -le "$(cargo test -p yap-polish --release structure_ -- --list 2>/dev/null | grep -c ': test$')"
    cargo test -p yap-polish --release structure_ -- --nocapture ; test $? -eq 0
    cd src-tauri
    cargo test --features custom-protocol structure_ ; test $? -eq 0
    cargo test --features custom-protocol --test formatting_fixtures ; test $? -eq 0
    cargo test --features custom-protocol --test app_aware_formatting ; test $? -eq 0
    cargo test --features custom-protocol --test formatting_settings_persist ; test $? -eq 0
  `,
})
