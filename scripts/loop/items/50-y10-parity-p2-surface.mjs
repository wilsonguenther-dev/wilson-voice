// Y10 — THE P2 PARITY QUEUE: "surface & polish". Source of record:
// ~/Obsidian/Wilson-Brain/Notes/Wispr-Full-Parity-Research-2026-08-09.md §3 P2
// items 15-23, quoted per item. Two of the nine already left this file: #17
// earcons and #20 coaching nudges went into 40-y8 because they are the cheapest
// answers to "the app looks broken". #18 rich-text snippets is here. #21 Focus
// Mode shipped its plumbing in Y9-D.
//
// Separate file from Y9 so the two run on opposite lanes.
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'Y10-A', prompt: 'Y10', branch: 'loop/y10-a-multi-language-and-the-in-bar-picker', gated: null,
  title: 'Multi-language: expose what the engine can already do, with the picker in the bar',
  preflight: `
    grep -q 'language_set' desktop/src-tauri/src/lib.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test languages
  `,
  spec: `
    P2 #15, VERBATIM: "Multi-language + in-bar language picker. (M · L) Whisper
    is already multilingual; expose a language set + auto-detect and put the
    picker in the bar."

    MEASURED: \`language: "en"\` is the shipped default (lib.rs:400) and the
    teardown's §2.1 row scores Yap "❌ English-only path" against Wispr's
    "99-language auto-detect, or pick a language subset; language picker lives
    in the Flow Bar". The capability exists in the engine —
    \`asr_engine.rs:394\` carries \`supports_language_detect\` and
    \`supports_streaming\` in the probed capabilities, and
    \`tests/asr_capabilities_probe.rs\` already reads them.

    Do:
      * A language SET, not a single language: the user picks the languages they
        actually speak, and auto-detect chooses among that set. A 99-language
        auto-detect is worse than a 2-language one — it mis-detects.
      * Drive it off the probed capability, not a hardcoded list: if
        \`supports_language_detect\` is false for the installed model, the UI must
        say so and fall back to the single selected language. A picker that
        silently does nothing is the defect pattern this whole plan is about.
      * The picker is a Y8-B slot in the bar, and also a Settings control.
      * The formatting pipeline is English-shaped in places (spoken punctuation
        names, \`format_email_shape\`). Do NOT pretend otherwise: state per
        cleanup stage whether it is language-agnostic, and for a non-English
        take skip the stages that are not, rather than applying English rules to
        German. Assert that in a test.
      * The meeting path is English-only by design and has a test for it
        (\`tests/meeting_english_only_gate.rs\`). Keep it green; this item is
        dictation only.

    Tests \`tests/languages.rs\`: a model without detect support falls back and
    says so; auto-detect only chooses within the set; English-shaped cleanup
    stages are skipped for a non-English take; the default remains English for
    an existing install.

    What NOT to do:
      - Do NOT expose 99 languages.
      - Do NOT run the English spoken-punctuation table over a non-English take.
      - Do NOT touch the meeting English-only gate.
  `,
  acceptance: `
    grep -q 'language_set' desktop/src-tauri/src/lib.rs
    test -f desktop/src-tauri/tests/languages.rs
    grep -q 'a_model_without_detect_support_falls_back_and_says_so' desktop/src-tauri/tests/languages.rs
    grep -q 'english_shaped_stages_are_skipped_for_a_non_english_take' desktop/src-tauri/tests/languages.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test languages                 ; test $? -eq 0
    cargo test --features custom-protocol --test meeting_english_only_gate ; test $? -eq 0
    cargo test --features custom-protocol --test asr_capabilities_probe    ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'PERM-H', prompt: 'Y10', branch: 'loop/perm-h-microphone-ranking-and-device-intelligence', gated: null,
  title: 'A ranked microphone preference list, forget-device, and the AirPods and clamshell warnings',
  preflight: `
    grep -q 'mic_ranking' desktop/src-tauri/src/lib.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test mic_devices
  `,
  spec: `
    P2 #16, VERBATIM: "Mic ranking + device intelligence. (S · L) Ordered
    preference list, 'forget device', AirPods/clamshell/lid warnings. Yap
    already has cpal device enumeration (YV35)."
    The teardown's §2.1 row details what Wispr ships [BUNDLE]: "Ranked
    preference list, drag to reorder, 'forget device', AirPods warning,
    clamshell/lid-closed detection, Jabra wear-detection auto-switch, separate
    Notetaker mic", Yap "🟡 single device pick".

    Do:
      * An ordered preference list persisted in settings; the highest-ranked
        PRESENT device wins at take start. Drag to reorder. "Forget device"
        removes a row so an old headset stops winning.
      * AirPods warning: Bluetooth input is low-bandwidth and noticeably worse
        for ASR. Say so once, when an AirPods-class device is selected, and
        offer the built-in mic. This is the single most common cause of a bad
        transcript that looks like a model problem.
      * Clamshell / lid-closed: the built-in mic is unavailable or muffled.
        Detect and warn. The teardown names Wispr's
        \`settings_microphone_airpods_warning\` and
        \`NoClamshellBuiltInMic\` notification as the [BUNDLE]/[LOCAL] evidence
        that both cases are real enough to ship copy for.
      * A DEVICE CHANGE MID-TAKE must not lose the take.
        \`tests/matrix_row14_output_device_change.rs\` covers OUTPUT; input is
        untested. Assert the take either continues on the new device or is
        parked in recovery (Y3/DB-B), never silently truncated.
      * Interaction with PERM-A: a device being present is a HARDWARE question
        (\`input_device_present\`) and authorization is a TCC question
        (\`authorization_status\`). This item must not re-merge them; PERM-A split
        them deliberately.
      * No Jabra wear-detection. Vendor-specific and out of scope; say so.

    Tests \`tests/mic_devices.rs\`: the highest-ranked present device wins;
    forget removes it from selection; the AirPods warning fires once per
    selection; a mid-take input change does not silently truncate; ranking
    persists across a restart (in Y4-H's round-trip).

    Depends on PERM-A, DB-B, Y4-H.

    What NOT to do:
      - Do NOT conflate device presence with permission.
      - Do NOT switch devices mid-take without telling the user.
      - Do NOT add vendor-specific wear detection.
  `,
  acceptance: `
    grep -q 'mic_ranking' desktop/src-tauri/src/lib.rs
    test -f desktop/src-tauri/tests/mic_devices.rs
    grep -q 'the_highest_ranked_present_device_wins' desktop/src-tauri/tests/mic_devices.rs
    grep -q 'the_airpods_warning_fires_once_per_selection' desktop/src-tauri/tests/mic_devices.rs
    grep -q 'a_mid_take_input_change_does_not_silently_truncate' desktop/src-tauri/tests/mic_devices.rs
    grep -q 'presence_and_authorization_stay_separate' desktop/src-tauri/tests/mic_devices.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test mic_devices     ; test $? -eq 0
    cargo test --features custom-protocol --test mic_auth_status ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y10-B', prompt: 'Y10', branch: 'loop/y10-b-rich-text-snippets-on-the-pasteboard', gated: null,
  title: 'Rich-text snippets: RTF and HTML flavours on the pasteboard without racing the receipt-sequenced paste',
  preflight: `
    grep -q 'rtf\\|public.rtf' desktop/src-tauri/src/paste.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test rich_snippets
  `,
  spec: `
    P2 #18, VERBATIM: "Rich-text snippets. (M · L) RTF/HTML flavours on
    \`NSPasteboard\`; interacts with the YV39 receipt-sequenced paste — needs its
    own slice."
    The teardown's §2.5 rows: Wispr's snippets are "rich text
    (bold/italic/links/lists)" against Yap's "✅ plain-text (YV48)", and
    "Replacement rules incl. HTML replacement" against Yap's "🟡 plain text".

    The hazard named in the spec is the whole item. \`paste_tx.rs\` (641 lines,
    YV39) sequences pastes by receipt precisely so two takes cannot interleave;
    writing MULTIPLE pasteboard flavours is several writes where there was one,
    and a half-written pasteboard is a paste of the wrong thing.

    Do:
      * Write all flavours for one paste as a single atomic pasteboard
        declaration — declare the types, then set each — inside the existing
        receipt transaction. Never a second transaction, never a write outside it.
      * Flavours: \`public.utf8-plain-text\` always (so every target works), plus
        \`public.rtf\` and/or \`public.html\` when the snippet carries markup. A
        target that cannot take rich text must still get the plain text.
      * Snippet storage gains a content-type. A plain snippet stays byte-for-byte
        what it is today; assert that, because YV48's existing snippet tests are
        the regression floor.
      * The signature block is copied BYTE FOR BYTE after polish
        (\`snippets::append_signature\`, lib.rs:283) — if a signature can now be
        rich, it must stay byte-identical in the plain flavour and the rich
        flavour must be derived, never re-authored by a model. Assert it.
      * Y6-C's paste_target_e2e must be extended, not duplicated: two takes in
        quick succession with rich flavours cannot interleave.

    Tests \`tests/rich_snippets.rs\`: all flavours land in one transaction; a
    plain snippet is byte-identical to today; a rich snippet degrades to plain
    on a plain-only target; a signature stays byte-identical; no interleaving.

    Depends on Y6-C.

    What NOT to do:
      - Do NOT write the pasteboard outside the receipt transaction.
      - Do NOT drop the plain-text flavour.
      - Do NOT let a model author the rich version of a signature.
  `,
  acceptance: `
    grep -qE 'public.rtf|public.html' desktop/src-tauri/src/paste.rs
    test -f desktop/src-tauri/tests/rich_snippets.rs
    grep -q 'all_flavours_land_in_one_receipt_transaction' desktop/src-tauri/tests/rich_snippets.rs
    grep -q 'a_plain_snippet_is_byte_identical_to_today' desktop/src-tauri/tests/rich_snippets.rs
    grep -q 'a_signature_stays_byte_identical' desktop/src-tauri/tests/rich_snippets.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test rich_snippets    ; test $? -eq 0
    cargo test --features custom-protocol --test paste_target_e2e ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y10-D', prompt: 'Y10', branch: 'loop/y10-d-mouse-button-push-to-talk', gated: null,
  title: 'A non-primary mouse button as push-to-talk',
  preflight: `
    grep -q 'mouse_ptt\\|MouseBinding' desktop/src-tauri/src/ptt_macos.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test mouse_ptt
  `,
  spec: `
    P2 #22, VERBATIM: "Mouse-button PTT. (M · L) Non-primary mouse button as
    the hotkey."
    The teardown's §2.1 row on Wispr's version ([BUNDLE]
    \`settings_hotkey_dialog_mx_master_*\`, [OFFICIAL] whats-new 2026-03-31):
    "Bind a non-primary mouse button as PTT; dedicated MX Master setup flow;
    also an Enter rebind so a mouse button sends the message", Yap ❌.

    \`ptt_macos.rs\` (590 lines) already owns the CGEvent tap for the
    modifier-only fn / fn⌃ hold, which is the hard part — a mouse button is
    another event type on the same tap.

    Do:
      * Bind button 3+ (never the primary or secondary button — stealing a
        right-click is unacceptable and must be impossible, not merely
        discouraged). Enforce it in the binding validator (Y8-A).
      * The tap must not swallow the event for other apps. A PTT mouse button
        that also fires in the game or the design tool the user is in is worse
        than no feature; a PTT button that is swallowed everywhere is also wrong.
        Decide, state the decision, and test the pass-through behaviour.
      * Input Monitoring is required for a raw button tap the same way it is for
        the modifier-only hold — PERM-E added Input Monitoring to the permission
        report; this item consumes it. Without the grant, the feature must be
        visibly unavailable rather than silently dead.
      * No MX-Master-specific setup flow. Generic, any mouse.

    Tests \`tests/mouse_ptt.rs\`: primary and secondary buttons are rejected by
    the validator; a bound button starts and stops a take; pass-through is as
    decided; without Input Monitoring the feature reports unavailable.

    Depends on Y8-A, PERM-E.

    What NOT to do:
      - Do NOT allow binding the primary or secondary button.
      - Do NOT ship it silently dead without Input Monitoring.
      - Do NOT add a vendor-specific setup flow.
  `,
  acceptance: `
    grep -qE 'mouse_ptt|MouseBinding' desktop/src-tauri/src/ptt_macos.rs
    test -f desktop/src-tauri/tests/mouse_ptt.rs
    grep -q 'primary_and_secondary_buttons_are_rejected' desktop/src-tauri/tests/mouse_ptt.rs
    grep -q 'without_input_monitoring_the_feature_reports_unavailable' desktop/src-tauri/tests/mouse_ptt.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test mouse_ptt           ; test $? -eq 0
    cargo test --features custom-protocol --test shortcut_validation ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y10-E', prompt: 'Y10', branch: 'loop/y10-e-local-insights-v2-and-the-yappy-profile', gated: null,
  title: 'Insights v2: the numbers Wispr computes in the cloud, computed in SQLite, feeding Yappy\'s dialogue',
  preflight: `
    grep -q 'most_corrected_word' desktop/src-tauri/src/db.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test insights_v2
  `,
  spec: `
    P2 #23, VERBATIM: "Local Insights v2 / Yappy profile. (M · D) 'most
    corrected word', 'peak hour', 'catch phrase' — Wispr computes these in the
    cloud; Yap can do it in SQLite. Feeds Yappy's dialogue."
    The teardown's §2.5 row on Wispr's gamified Voice Profile [LOCAL]/[BUNDLE]:
    \`superpower_title\`, \`catch_phrase\`, \`persona\`, \`most_used_word\`,
    \`most_removed_word\`, \`peak_time_top_app\`, \`word_count_milestone\`, with the
    note "Yappy is the better version of this idea".

    Yap has the raw material and is not using it: the dictionary learns from
    corrections (YV47), \`raw_text\` and the formatted text are both stored
    (YV10/51), \`get_insights\` and a day-series exist (lib.rs:2420-2432), and
    DB-A's usage rollup lands the per-day words and voiced seconds.

    Do:
      * Computed in SQL over the existing tables, no new capture: most-used
        word (stopword-filtered), most-CORRECTED word (from the raw-vs-final
        diff Y4-G's diff module already computes — reuse it, do not write a
        second differ), peak hour, peak app, longest take, current streak,
        word-count milestones.
      * Bounded cost: these run on demand when the Insights view opens, not on
        every take, and DB-C's volume test must still pass with them
        (10,000 takes, no full scans on the take path).
      * FEEDS YAPPY. Y5-H's habitat reacts to real state; these are the richest
        real state Yap has. Wire at least three of them into the habitat's
        reaction table and into the pill's commentary via \`pill/tone.ts\`, so
        the numbers become personality rather than a dashboard
        (feedback_no_generic_ui: reject AI-dashboard aesthetics).
      * Nothing is transmitted and nothing is a leaderboard. The teardown's
        "explicitly not wanted" list includes teams and leaderboards.
      * Insights must render honestly at ZERO takes — Y5-B's empty state for
        this view exists because today the view renders blank (App.tsx:2953
        renders nothing when \`insights\` is falsy).

    Tests \`tests/insights_v2.rs\`: each metric on a seeded corpus with a known
    answer; stopwords excluded from most-used; most-corrected uses the stored
    raw-vs-final pair; zero takes yields a defined empty result, never a panic
    and never a divide-by-zero; no query added to the take path.

    Depends on DB-A, DB-C, Y4-G, Y5-H.

    What NOT to do:
      - Do NOT compute these on every take.
      - Do NOT write a second diff implementation.
      - Do NOT transmit any of it, and do not build a leaderboard.
  `,
  acceptance: `
    grep -q 'most_corrected_word' desktop/src-tauri/src/db.rs
    test -f desktop/src-tauri/tests/insights_v2.rs
    grep -q 'stopwords_are_excluded_from_most_used' desktop/src-tauri/tests/insights_v2.rs
    grep -q 'zero_takes_yields_a_defined_empty_result' desktop/src-tauri/tests/insights_v2.rs
    grep -q 'no_query_was_added_to_the_take_path' desktop/src-tauri/tests/insights_v2.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test insights_v2       ; test $? -eq 0
    cargo test --features custom-protocol --test history_at_volume ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y10-F', prompt: 'Y10', branch: 'loop/y10-f-publish-the-idle-cost-number', gated: null,
  title: 'Measure and publish Yap\'s idle RAM and CPU — the free marketing line the research asked for',
  preflight: `
    test -f desktop/src-tauri/tests/idle_cost.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test idle_cost
  `,
  spec: `
    From the teardown §2.11, verbatim: "Idle cost measured by users at ~800 MB
    RAM / ~8% CPU; 8-10 s cold start ... **Yap's position:** a Rust/Tauri binary
    with a warm GGUF engine and idle-throttled canvases (YV24) should beat that
    by an order of magnitude. **Measure it and publish the number — it is free
    marketing.**"

    Yap's own recorded numbers: "138MB idle" for the build carrying the diarize
    sidecar (project_yap_build_state, 2026-08-16), and an explicit energy pass
    (YV80 lazy model load, YV81 no busy timers / idle animations park / the
    polish sidecar unloads when unused).

    Do:
      * \`tests/idle_cost.rs\`: launch the built binary headless, let it settle,
        and assert resident memory and CPU are under named ceilings — with the
        machine and date in a comment, and the ceilings set with enough headroom
        that they fail on a REGRESSION and not on a different Mac. Express CPU
        as a ceiling over a sampling window, never an instantaneous read.
      * Assert the specific things the energy pass bought, so they cannot erode:
        no ASR model resident before the first take (YV80 — and
        \`tests/meeting_no_model_resident.rs\` is the existing sibling
        assertion, follow its shape), the polish sidecar not running when unused
        (YV81), and no timer firing faster than the documented floor while idle.
      * Cold start: measure and assert a ceiling.
      * Publish the numbers in README.md and on the site copy, next to the
        claim, with the measurement method in one sentence. A published number
        with no method is a number nobody believes.

    Depends on Y3-G (which establishes the long-take budget harness — reuse its
    measurement helpers rather than writing a second sampler).

    What NOT to do:
      - Do NOT publish a number you did not measure on a build from this repo.
      - Do NOT name the competitor's number in Yap's own marketing copy
        (no competitor jabs). Publish Yap's number and let it stand alone.
      - Do NOT set a ceiling so tight it goes red on a different Mac.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/idle_cost.rs
    grep -q 'no_asr_model_is_resident_before_the_first_take' desktop/src-tauri/tests/idle_cost.rs
    grep -q 'the_polish_sidecar_is_not_running_when_unused' desktop/src-tauri/tests/idle_cost.rs
    grep -q 'no_timer_fires_faster_than_the_documented_floor_while_idle' desktop/src-tauri/tests/idle_cost.rs
    grep -qE 'idle' README.md
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test idle_cost                ; test $? -eq 0
    cargo test --features custom-protocol --test meeting_no_model_resident ; test $? -eq 0
  `,
})
