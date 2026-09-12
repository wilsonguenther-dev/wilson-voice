// Y3 — LONG DICTATIONS. Wilson, 2026-09-12, verbatim: "no handling for long
// dictations."
//
// AUDIT — the dictation path is one-shot from end to end. There is no chunking,
// no streaming, no memory bound, no progress, and a hard wall at 120 seconds of
// DECODE that silently kills the whole take:
//
//   src-tauri/src/transcription.rs:54
//     pub const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(120);
//   its own doc, :51-53: "Way above any real clip (a 60 s take is ~1 s on
//   Metal) — this exists only so a wedged native call surfaces as an error".
//   That reasoning holds for a 60 s take and fails for a 15 minute one.
//
//   src-tauri/src/lib.rs:1350   manager.transcribe(samples, language, bias_prompt)
//     ONE call, the ENTIRE take's samples, no windowing.
//
//   src-tauri/src/record.rs:2368  self.mono.extend_from_slice(interleaved)
//   src-tauri/src/record.rs:2377  self.raw.extend_from_slice(&self.mono)
//     TWO unbounded Vec<f32> for the whole take. At 48 kHz stereo a 15-minute
//     dictation is ~345 MB in `raw` alone before the 16 kHz downsample, and
//     both live until finalize_take (record.rs:2445).
//
//   src-tauri/src/polish.rs:73
//     const MAX_POLISH_WORDS: usize = 400;
//   its own doc, :71-72: "Long-form is rules-only: chunking is a later item,
//   and a single pass over 400+ words cannot hold the deadline". That later
//   item is THIS one. A long dictation gets no formatting at all, which is the
//   other half of Wilson's formatting complaint.
//
// THE MACHINERY ALREADY EXISTS — for MEETINGS, not for dictation:
//   meeting_asr.rs + transcription.rs chunk geometry, MAX_SANCTIONED_CHUNK_DECODE_SECONDS
//   (transcription.rs:101), seam dedupe via max_timestamp_kind (asr_engine.rs:382),
//   rtring.rs (YV91 lock-free SPSC ring, whose doc at rtring.rs:6-10 says in as
//   many words: "A five-second dictation never exposes that. Three continuous
//   hours on a meeting ... does"), the capture journal + recovery dir (YV63),
//   and asr_engine.rs:130 which "cancels a meeting chunk whenever a dictation
//   wants the engine".
//   Y3 is largely a REUSE job, not an invention job. Anything Y3 builds that
//   meeting_asr.rs already does is a defect in Y3.
//
// Wispr's comparable number, for calibration: max session length 20 min, raised
// from 5 (reference_wispr_parity_research §2.1 [OFFICIAL]).
//
// SHARED PREAMBLE + STANDARD GATE: see 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'Y3-A', prompt: 'Y3', branch: 'loop/y3-a-bounded-capture-spill-to-disk', gated: null,
  title: 'Capture stops growing two unbounded Vecs — a long take spills to disk with a measured memory ceiling',
  preflight: `
    test 0 -eq "$(grep -c 'self.raw.extend_from_slice(&self.mono)' desktop/src-tauri/src/record.rs)"
    grep -q 'SpillWriter\\|spill_to_disk' desktop/src-tauri/src/record.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test dictation_capture_memory
  `,
  spec: `
    MEASURED: \`record.rs:2280-2284\` holds \`out\`, \`raw\` and \`mono\` as
    \`Vec<f32>\`; \`:2368\` appends every interleaved callback to \`mono\` and
    \`:2377\` appends \`mono\` to \`raw\`. Nothing bounds either. \`finalize_take\`
    (:2445) then takes the whole thing by value.

    There is already a proven pattern in this repo for exactly this problem and
    it must be reused rather than re-derived: \`tests/meeting_capture_memory.rs\`
    asserts a memory ceiling for the meeting path, and \`rtring.rs\` is the
    real-time-safe hand-off built once "so 22-B reuses it verbatim rather than
    growing a second copy" (rtring.rs:16-18). Dictation is now the third
    consumer.

    Do:
      * The capture consumer writes finalized PCM to the take's WAV incrementally
        (a \`SpillWriter\` over the existing \`recordings\` dir path, which
        \`start_recording\` already passes in at lib.rs:1142) and keeps only a
        bounded working window in RAM.
      * Ceiling: a named constant \`MAX_RESIDENT_CAPTURE_BYTES\`, set so that a
        take of ANY length holds under ~32 MB resident for the audio buffers.
        Pick the window from the DSP's actual lookahead needs (high_pass,
        normalize_rms, denoise all operate blockwise — read record.rs:2483-2600
        and size the window to the largest true dependency, then say the number
        in the doc comment).
      * \`finalize_take\` reads back from the spill instead of taking a giant Vec.
        \`read_wav_16k_mono\` (record.rs:826) already exists for the read side.
      * RNNoise denoise (\`denoise: true\` is the shipped default, lib.rs:419)
        must still run, blockwise, with identical output on a short clip. That is
        the regression that would be easy to miss: assert byte-identical output
        against today's path for the existing fixture
        \`tests/fixtures/quick-brown-fox-16k.wav\`.
      * The capture journal (YV63) and recovery dir keep working unchanged — a
        take that dies mid-spill must still be recoverable, and a spilled WAV is
        strictly easier to recover than a lost Vec.

    Tests \`tests/dictation_capture_memory.rs\`:
      * \`resident_bytes_are_bounded_across_a_synthetic_twenty_minute_take\` —
        feed 20 minutes of generated frames through the consumer and assert peak
        resident buffer bytes < MAX_RESIDENT_CAPTURE_BYTES. Model it on
        meeting_capture_memory.rs so the two agree on how "resident" is counted.
      * \`short_take_output_is_byte_identical_to_the_pre_spill_path\` — against
        the committed fixture WAV.
      * \`spill_survives_a_mid_take_abort_and_lands_in_the_recovery_dir\`.
      * \`the_audio_callback_still_never_allocates\` — reuse the assertion shape
        from tests/meeting_capture_rt_safety.rs; do not write a second one.

    What NOT to do:
      - Do NOT move DSP into the audio callback to avoid buffering. rtring.rs's
        entire premise is that the callback copies and returns.
      - Do NOT change the WAV format or sample rate on disk. Other code reads it.
      - Do NOT bound by TRUNCATING audio. Dropping the end of a long dictation
        is the bug, not the fix.
  `,
  acceptance: `
    test 0 -eq "$(grep -c 'self.raw.extend_from_slice(&self.mono)' desktop/src-tauri/src/record.rs)"   # 0 (was 1)
    grep -q 'MAX_RESIDENT_CAPTURE_BYTES' desktop/src-tauri/src/record.rs
    test -f desktop/src-tauri/tests/dictation_capture_memory.rs
    grep -q 'resident_bytes_are_bounded_across_a_synthetic_twenty_minute_take' desktop/src-tauri/tests/dictation_capture_memory.rs
    grep -q 'short_take_output_is_byte_identical_to_the_pre_spill_path' desktop/src-tauri/tests/dictation_capture_memory.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test dictation_capture_memory ; test $? -eq 0
    cargo test --features custom-protocol --test meeting_capture_memory   ; test $? -eq 0
    cargo test --features custom-protocol --test capture_journal_recovery ; test $? -eq 0
    cargo test --features custom-protocol --test meeting_capture_rt_safety; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y3-B', prompt: 'Y3', branch: 'loop/y3-b-chunked-decode-for-the-dictation-path', gated: null,
  title: 'Long takes decode in windows with seam dedupe, reusing the meeting chunker instead of one 120s-capped call',
  preflight: `
    grep -q 'chunk' desktop/src-tauri/src/lib.rs
    grep -q 'DICTATION_CHUNK' desktop/src-tauri/src/transcription.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test dictation_chunked
  `,
  spec: `
    MEASURED: \`lib.rs:1350\` is \`manager.transcribe(samples, language,
    bias_prompt)\` — the whole take, one call — under
    \`TRANSCRIBE_TIMEOUT = 120s\` (transcription.rs:54). Whisper decode is
    roughly real-time-divided-by-N on Metal; the 120 s wall is generous for a
    minute of speech and fatal somewhere in the multi-minute range, and when it
    trips the user loses the ENTIRE take. There is no partial result to keep.

    Reuse, do not rebuild. Read these first and extract the shared piece:
      transcription.rs:86-110  the chunk-decode budget and
                               MAX_SANCTIONED_CHUNK_DECODE_SECONDS (:101) with
                               its own warning that "the two constants
                               disagreeing is precisely how a legal 37 s chunk
                               could ..." — read that comment in full.
      asr_engine.rs:382-394    max_timestamp_kind / max_audio_ms — "is there an
                               alignment to dedupe seams with at all?" and "is a
                               35 s window ..." This is the seam machinery.
      asr_engine.rs:221-236    Shift onto another timeline (a chunk's own start).
      meeting_asr.rs           the working chunker.

    Do:
      * Factor the chunk geometry + seam dedupe out of the meeting path into a
        module both callers use. If meeting_asr.rs's chunker is already
        general, call it; if it is entangled with meeting state, extract the
        pure part. Either way the outcome is ONE chunker with one set of tests.
      * Dictation decodes in windows with overlap, deduping at seams by
        timestamp when \`max_timestamp_kind\` supports it and by longest-common-
        suffix/prefix when it does not — and when NEITHER is available, it must
        fail loudly rather than silently double a phrase at every seam.
      * \`TRANSCRIBE_TIMEOUT\` becomes per-CHUNK, not per-take, so a long take has
        no wall at all. Keep a separate, generous whole-take deadline so a truly
        wedged engine still surfaces; derive it from chunk count so it scales.
      * Partial results are kept: if chunk 9 of 12 fails, the take returns the
        first 8 chunks' text, marks the take degraded, and preserves the clip in
        the recovery dir for a retry (the retry path exists — lib.rs:6230, YV66).
        Losing 11 minutes because minute 12 failed is the behaviour being deleted.
      * Short takes must take a byte-identical path to today. The single-window
        case has to produce the same bytes, or every formatting fixture and the
        YV66 hallucination-gate corpus shift under us for no reason.

    Tests \`tests/dictation_chunked.rs\`:
      * \`single_window_take_is_byte_identical_to_the_unchunked_path\` (fixture WAV).
      * \`seams_do_not_duplicate_or_drop_words\` — synthesize a known sentence
        stream across a seam and assert exact text.
      * \`a_failed_middle_chunk_keeps_the_earlier_chunks\`
      * \`per_chunk_deadline_not_per_take\` — assert a 12-chunk take is not
        subject to a 120 s total.
      * \`chunker_has_exactly_one_implementation\` — a call-site sweep asserting
        meeting and dictation resolve to the same function.
      * Keep \`tests/matrix_new_asr_chunk_timeout.rs\` green; it already covers
        the meeting side of a chunk timeout and must not regress.

    Depends on Y3-A (a spilled WAV is what the chunker reads).

    What NOT to do:
      - Do NOT fix this by raising TRANSCRIBE_TIMEOUT. A bigger wall is still a
        wall, and a 15-minute single-window decode also blows the memory ceiling
        Y3-A just established.
      - Do NOT let dictation chunks contend with a meeting for the engine.
        asr_engine.rs:130 already makes dictation PREEMPT meetings; preserve that
        priority and keep tests/meeting_dictation_preempts_transcription.rs green.
    PANEL 2026-09-12 — the cliff that justifies this item is not where the audit
    put it, so the scope narrows. MEASURED: a 601-second WAV (241x the
    quick-brown-fox fixture, 16 kHz mono) through the shipped headless path —
    \`./target/debug/wilson-voice --transcribe-file <601s.wav>\` — returned exit
    0 in 30.5 s with ~1,900 words. That is ~20x real time INCLUDING model load,
    consistent with the repo's own note (transcription.rs:51-54, "a 60 s take is
    ~1 s on Metal"), which puts the 120 s TRANSCRIBE_TIMEOUT near 45 minutes of
    audio, not four. What actually broke at ten minutes is what the plan ranked
    lower: 1,900 words arriving as one unbroken block (no paragraph rule, no
    polish past 400 words), with no progress and no cancel.
      * KEEP the narrow, high-value half: TRANSCRIBE_TIMEOUT becomes PER-CHUNK,
        and a trip keeps the chunks already decoded instead of losing the take.
      * Put the measurement in the pre-flight: a timed long-WAV decode row with
        the observed real-time factor, written to docs/BUDGETS.md, so Y3-F's
        declared maximum session length is derived from a number somebody took.
      * Refactoring the WORKING meeting chunker into a shared module is the one
        change in this lane that can regress a shipped subsystem. Do it only as
        far as the per-chunk timeout and the seam dedupe require, and say in the
        PR body what the meeting path's behaviour was before and after.
      * Y3-A (the 345 MB ceiling), Y4-C (paragraphing, rules-only) and
        Y3-C/Y3-D (progress and cancel) are what a ten-minute take needs; this
        item must not block them.

  `,
  acceptance: `
    grep -q 'DICTATION_CHUNK' desktop/src-tauri/src/transcription.rs
    test -f desktop/src-tauri/tests/dictation_chunked.rs
    grep -q 'single_window_take_is_byte_identical_to_the_unchunked_path' desktop/src-tauri/tests/dictation_chunked.rs
    grep -q 'seams_do_not_duplicate_or_drop_words' desktop/src-tauri/tests/dictation_chunked.rs
    grep -q 'a_failed_middle_chunk_keeps_the_earlier_chunks' desktop/src-tauri/tests/dictation_chunked.rs
    grep -q 'chunker_has_exactly_one_implementation' desktop/src-tauri/tests/dictation_chunked.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test dictation_chunked                          ; test $? -eq 0
    cargo test --features custom-protocol --test matrix_new_asr_chunk_timeout               ; test $? -eq 0
    cargo test --features custom-protocol --test meeting_dictation_preempts_transcription   ; test $? -eq 0
    cargo test --features custom-protocol --test formatting_fixtures                        ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y3-C', prompt: 'Y3', branch: 'loop/y3-c-long-take-progress-in-the-pill', gated: null,
  title: 'The pill reports real progress on a long take instead of an honest-looking lie',
  preflight: `
    grep -q '"transcribing"' desktop/src/pill/live.ts
    grep -q 'transcribe_progress' desktop/src-tauri/src/lib.rs
    cd ${APP} && npm ci && npm test -- pill/live
  `,
  spec: `
    Two separate honesty problems, both already named by Wilson.

    (1) THE ESCALATION IS A TIMER. project_yap_open_bugs BUG 2, resolved as
    YV17 but only partly: "the pill's live escalation was TIMER-based, not real
    word/sentence tracking ... it 'just moves along' regardless of what's
    actually said. Wilson wants it to react to ACTUAL words/sentences." The
    machinery still estimates: \`live.ts:44\`
    \`wordsFromVoiced = (voicedSec) => voicedSec * WORDS_PER_SEC\`, and
    \`liveTierForWords\` (:46) drives quick|notes|desk|essay|saga off that
    estimate. On a 10-minute take the estimate's error is enormous and the
    persona it promises is unearned. Y3-B's chunker gives the first real fix
    available: each completed chunk yields REAL text and a REAL word count
    mid-take. Feed it.

    (2) THERE IS NO PROGRESS AT ALL AFTER THE HOLD ENDS. ClassicPill.tsx tracks
    \`{recording, busy, message}\`; \`busy\` is a boolean. A 15-minute take
    decoding across 12 chunks shows the same undifferentiated busy state for
    minutes. project_yap_pill_vision names this exactly: "fill the dead time
    after talking stops and before text appears (transcribe/think gap)".

    Do:
      * Rust emits \`transcribe_progress { chunk, of, words_so_far }\` per
        completed chunk from Y3-B's loop. No timer, no interpolation — only real
        completions. Emit nothing for a single-window take (a short take has no
        progress to report and a flicker of 1/1 is worse than silence).
      * \`LivePhase\` gains \`"transcribing"\` as a first-class phase distinct from
        \`thinking\` (which is the polish/LLM gap), and carries
        \`{ chunk, of, wordsSoFar }\`.
      * Both pills render it: a determinate fill, the numeral in Departure Mono,
        and — this is the character beat — Yappy's live commentary escalates on
        \`wordsSoFar\` from real chunks instead of \`wordsFromVoiced\`. Keep
        \`wordsFromVoiced\` for the LISTENING phase only, where nothing better
        exists, and mark it in its doc comment as an estimate that the
        transcribing phase must not use.
      * Cap the claim: for a take still recording, the pill must never state a
        word count as fact. "Listening" states may be playful; numeric states
        may not be wrong.

    Tests in \`desktop/src/pill/live.test.ts\`:
      * \`transcribing_uses_real_chunk_words_not_the_voiced_estimate\`
      * \`single_window_take_emits_no_progress_phase\`
      * \`progress_is_monotonic_and_never_exceeds_of\`
      * \`listening_tier_still_uses_the_estimate\` (the deliberate exception).
    Rust: \`tests/transcribe_progress_events.rs\` — one event per completed
    chunk, zero for a single window, and none after the take ends.

    Depends on Y3-B.

    PR body owes a capture of the transcribing phase at 3/12 in both pill styles
    and at all three dock positions.

    What NOT to do:
      - Do NOT show an indeterminate spinner for a chunked take. The whole point
        is that the count is now knowable.
      - Do NOT interpolate progress between chunk completions to make the bar
        smooth. A smooth bar that is lying is the defect being removed.
  `,
  acceptance: `
    grep -q '"transcribing"' desktop/src/pill/live.ts
    grep -q 'transcribe_progress' desktop/src-tauri/src/lib.rs
    grep -q 'transcribing_uses_real_chunk_words_not_the_voiced_estimate' desktop/src/pill/live.test.ts
    grep -q 'single_window_take_emits_no_progress_phase' desktop/src/pill/live.test.ts
    grep -q 'progress_is_monotonic_and_never_exceeds_of' desktop/src/pill/live.test.ts
    test -f desktop/src-tauri/tests/transcribe_progress_events.rs
    cd ${APP} && npm ci
    npm test -- pill ; test $? -eq 0
    cd src-tauri && cargo test --features custom-protocol --test transcribe_progress_events ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y3-D', prompt: 'Y3', branch: 'loop/y3-d-cancel-a-long-take-at-any-stage', gated: null,
  title: 'Cancel works mid-decode, not just mid-recording — and a cancel never loses the audio',
  preflight: `
    grep -q 'CANCEL' desktop/src-tauri/src/shortcuts.rs
    grep -q 'cancel_transcription' desktop/src-tauri/src/lib.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test cancel_long_take
  `,
  spec: `
    \`cancel_recording\` exists (referenced at lib.rs:478: "cleared by
    \`cancel_recording\`") and stops a RECORDING. Once the hold ends and decode
    starts, there is nothing to press: a 15-minute take that the user
    immediately regrets occupies the engine to completion. And there is no
    cancel HOTKEY at all — \`shortcuts.rs:143 ALL\` is
    PASTE_LAST, UNDO_AI_EDIT, DICTATION_TOGGLE_LEGACY, MEETING_TOGGLE.
    Wispr ships a dedicated rebindable cancel key
    (reference_wispr_parity_research §2.1 [BUNDLE]
    \`settings_hotkey_dialog_cancel_*\`, listed as ❌ for Yap).

    Do:
      * \`shortcuts::CANCEL\` — a new Binding in the existing table (the table
        exists so bindings are declared once; shortcuts.rs:3-8 explains why).
        Default: Escape while a take is active, rebindable through the YV15
        capture control. It must not register a global Escape that eats Escape
        from every app: bind it only while a take is in flight, or scope it to
        the pill's own window plus a take-active global. State which and why.
      * \`cancel_transcription\`: cooperative cancellation checked at every chunk
        boundary in Y3-B's loop, plus the existing engine-cancel path
        (transcription.rs already has one — \`PREEMPTED_FOR_DICTATION\` :68 and
        \`ABANDONED_FOR_EXIT\` :84 prove chunk decode is already cancellable;
        reuse that mechanism, add a third reason \`CANCELLED_BY_USER\`).
      * A cancel NEVER deletes the clip. It parks it in the recovery dir with
        the same 7-day purge lifecycle (YV52/YV63) so "actually, transcribe that"
        is possible from History. A cancel that destroys 15 minutes of audio is
        a worse bug than the one being fixed.
      * A cancel pastes NOTHING and writes no transcript row. Assert both.
      * The pill shows a cancelled state that settles to idle — not an error.

    Tests \`tests/cancel_long_take.rs\`:
      * \`cancel_mid_recording_writes_no_row_and_pastes_nothing\`
      * \`cancel_mid_decode_stops_at_the_next_chunk_boundary\`
      * \`cancelled_clip_lands_in_the_recovery_dir\`
      * \`cancel_reason_is_distinguishable_from_preempted_and_abandoned\` — three
        distinct constants; a string compared against a literal in another
        module is the bug transcription.rs:74-77 already warns about.
      * \`escape_is_not_a_global_shortcut_while_idle\`

    Depends on Y3-B.

    What NOT to do:
      - Do NOT implement cancel by killing the engine process. The engine is
        in-process and shared with meetings; killing it takes a meeting down.
      - Do NOT reuse PREEMPTED_FOR_DICTATION as the cancel reason.
  `,
  acceptance: `
    grep -q 'pub const CANCEL' desktop/src-tauri/src/shortcuts.rs
    grep -q 'CANCELLED_BY_USER' desktop/src-tauri/src/transcription.rs
    grep -q 'fn cancel_transcription' desktop/src-tauri/src/lib.rs
    test -f desktop/src-tauri/tests/cancel_long_take.rs
    grep -q 'cancelled_clip_lands_in_the_recovery_dir' desktop/src-tauri/tests/cancel_long_take.rs
    grep -q 'cancel_reason_is_distinguishable_from_preempted_and_abandoned' desktop/src-tauri/tests/cancel_long_take.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test cancel_long_take        ; test $? -eq 0
    cargo test --features custom-protocol --test tray_hotkey_no_collision; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'DB-B', prompt: 'Y3', branch: 'loop/db-b-crash-recovery-for-a-long-take', gated: null,
  title: 'A crash or quit mid-long-take loses nothing — the take resumes or is offered back on next launch',
  preflight: `
    grep -q 'resume_take\\|recover_dictation' desktop/src-tauri/src/lib.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test long_take_recovery
  `,
  spec: `
    The pieces exist and are not connected for the dictation path:
      * YV63 capture journal + recovery dir (\`recovery_dir()\`, lib.rs:1145),
        with \`tests/capture_journal_recovery.rs\` green.
      * The meeting path re-decodes abandoned chunks on next launch
        (transcription.rs:70-84: "the relaunch resumed AFTER a chunk that had
        never been decoded: a permanent [gap]" — a bug they already fixed there).
      * \`tests/matrix_new_quit_mid_processing.rs\` covers quit-mid-processing for
        meetings.
    For a long dictation there is no equivalent: the spilled WAV (Y3-A) and any
    completed chunk text (Y3-B) are on disk, and nothing reads them back.

    Do:
      * Persist per-take chunk results as they complete (a small sidecar JSON
        next to the spilled WAV in the recovery dir, or a \`take_chunks\` table —
        prefer the DB, it is already WAL and already the durable store, and
        db.rs:1191 already reasons about which tables hold user text).
      * On launch, \`recover_dictation()\` finds takes with audio + partial text
        and NO transcript row, and offers them in History as recoverable with
        one action: "Finish transcribing". It must not auto-paste anything —
        pasting into whatever app happens to be focused minutes or days later is
        the wrong behaviour and would violate YV21's paste-target guard.
      * Honour the existing 7-day purge so the recovery dir cannot grow forever.
      * A recovered take runs the SAME cleanup and hallucination gates as a live
        take. lib.rs:6230 already records this rule for the retry path ("the
        retry path runs the SAME gate as the live path") — extend it, do not
        fork it.

    Tests \`tests/long_take_recovery.rs\`:
      * \`kill_mid_decode_leaves_audio_plus_partial_text_and_no_transcript_row\`
      * \`next_launch_offers_the_take_and_never_pastes_it\`
      * \`finishing_a_recovered_take_runs_the_same_gates_as_a_live_take\`
      * \`recovery_dir_purge_still_bounds_growth\`
      * \`a_completed_take_leaves_nothing_recoverable\` (no phantom offers).

    Depends on Y3-A, Y3-B.

    What NOT to do:
      - Do NOT auto-resume decode at launch without the user asking. A cold
        launch that pins the GPU for four minutes is its own bug.
      - Do NOT store partial TEXT anywhere outside the app's data dir. Transcript
        text must never reach a log or a support bundle unredacted — there are
        already tests for that (tests/support_bundle_redaction.rs); keep them green.
  `,
  acceptance: `
    grep -q 'fn recover_dictation' desktop/src-tauri/src/lib.rs
    test -f desktop/src-tauri/tests/long_take_recovery.rs
    grep -q 'next_launch_offers_the_take_and_never_pastes_it' desktop/src-tauri/tests/long_take_recovery.rs
    grep -q 'finishing_a_recovered_take_runs_the_same_gates_as_a_live_take' desktop/src-tauri/tests/long_take_recovery.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test long_take_recovery        ; test $? -eq 0
    cargo test --features custom-protocol --test capture_journal_recovery  ; test $? -eq 0
    cargo test --features custom-protocol --test support_bundle_redaction  ; test $? -eq 0
    cargo test --features custom-protocol --test matrix_new_quit_mid_processing ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y3-F', prompt: 'Y3', branch: 'loop/y3-f-max-session-length-and-the-honest-ceiling', gated: null,
  title: 'A declared maximum session length with a warning before it, instead of an undeclared cliff',
  preflight: `
    grep -q 'MAX_SESSION_SECONDS' desktop/src-tauri/src/record.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test max_session
  `,
  spec: `
    There is no maximum take length in the code — \`git grep "MAX_TAKE\\|
    max_take\\|MAX_RECORD\\|session_len" desktop\` returns nothing functional.
    That is not generosity; it means the failure mode at some unknown length is
    a timeout or an OOM rather than a message. Wispr declares 20 minutes,
    raised from 5 (reference_wispr_parity_research §2.1 [OFFICIAL]).

    Do:
      * \`record::MAX_SESSION_SECONDS\` — a declared ceiling. Choose it from what
        Y3-A/B actually hold and SAY the measurement in the doc comment; do not
        copy Wispr's 20 minutes without evidence. If the measured answer is
        larger than 20 minutes, declare the larger number — the point is that it
        is declared and tested, not that it matches a competitor.
      * At 80% of the ceiling the pill says so, once, calmly (reuse the
        transcribing/listening phase copy path from Y3-C — no new toast system).
      * At the ceiling the take STOPS cleanly and transcribes what it has. It
        does not error, does not truncate mid-word if a VAD boundary is within
        a couple of seconds (vad.rs:803 lines of machinery already exists — use
        it to pick the cut), and does not discard anything.
      * A meeting is NOT subject to this ceiling. \`tests/matrix_row17_meeting_cap.rs\`
        already covers the meeting cap; assert the two ceilings are separate
        constants so tightening one never silently tightens the other.

    Tests \`tests/max_session.rs\`:
      * \`take_stops_at_the_ceiling_and_transcribes_what_it_has\`
      * \`warning_fires_once_at_eighty_percent\`
      * \`cut_prefers_a_vad_boundary_within_the_grace_window\`
      * \`meeting_cap_and_dictation_cap_are_separate_constants\`

    Depends on Y3-A, Y3-C.

    What NOT to do:
      - Do NOT pick 5 minutes to be safe. Wilson dictates long; a low ceiling is
        the same complaint in a new costume.
      - Do NOT stop the take without transcribing it.
  `,
  acceptance: `
    grep -q 'MAX_SESSION_SECONDS' desktop/src-tauri/src/record.rs
    test -f desktop/src-tauri/tests/max_session.rs
    grep -q 'take_stops_at_the_ceiling_and_transcribes_what_it_has' desktop/src-tauri/tests/max_session.rs
    grep -q 'meeting_cap_and_dictation_cap_are_separate_constants' desktop/src-tauri/tests/max_session.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test max_session              ; test $? -eq 0
    cargo test --features custom-protocol --test matrix_row17_meeting_cap ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y3-G', prompt: 'Y3', branch: 'loop/y3-g-long-take-latency-and-energy-budget', gated: null,
  title: 'A measured latency and energy budget for long takes, published as a test that fails on regression',
  preflight: `
    test -f desktop/src-tauri/tests/long_take_budget.rs
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test long_take_budget
  `,
  spec: `
    \`latency.rs\` (146 lines) already instruments the press->capture_start span
    (YV35, anchored on physical key-down — lib.rs:1138). YV81 was an explicit
    energy pass ("no busy timers, idle animations park, polish sidecar unloads
    when unused"). Y3 adds chunked decode, a spill writer and a progress event
    stream — three new opportunities to undo all of that.

    Also note the marketing stake, which is real and already written down:
    reference_wispr_parity_research §2.11 records Wispr at ~800 MB RAM / ~8% CPU
    idle and says of Yap "Measure it and publish the number — it is free
    marketing." A regression test is how that number stays true.

    Create \`tests/long_take_budget.rs\` with named budgets as constants and a
    comment giving the machine and the date each was measured on:
      * \`press_to_capture_start\` unchanged by the spill writer (compare against
        latency.rs's existing measurement path, not a new stopwatch).
      * \`chunk_decode_wall_per_audio_second\` — a ceiling, measured on the
        committed fixture, so a chunker regression that halves throughput fails.
      * \`progress_events_per_minute_of_audio\` — a CEILING. An event storm from
        the progress stream is an energy regression that nothing else would catch.
      * \`resident_bytes_after_a_long_take_return_to_baseline\` — the spill
        buffers and the chunk text must be dropped, not leaked, when the take
        finalizes.
      * \`no_new_polling_timer_was_introduced\` — sweep for
        \`Duration::from_millis\`/\`from_secs\` under a threshold in the modules Y3
        touched (record.rs, transcription.rs, lib.rs's take path), asserting
        against an explicit allowlist of the ones that already exist. This is
        the standing guard for YV81.

    Then write the numbers into docs/loop/PLAN.md's audit section as measured
    facts with their date, so the next loop inherits evidence instead of folklore.

    Depends on Y3-A..F.

    What NOT to do:
      - Do NOT assert a wall-clock latency budget that depends on the runner's
        GPU. Express the decode budget as a RATIO against the same fixture
        decoded single-window on the same machine in the same run.
      - Do NOT use a timing test as the only proof of a correctness fix.
    PANEL 2026-09-12 — the budget list is missing the app itself and the write
    target does not exist in a lane worktree.
      * Budgets go to docs/BUDGETS.md (TRACKED). docs/loop/PLAN.md is UNTRACKED
        in git — \`git status --porcelain --untracked=all\` lists it, and
        \`git ls-files docs/loop\` returns HARNESS.md only — so a lane worktree,
        which is a checkout of origin/main, does not contain it and the old
        \`grep -q 'measured' docs/loop/PLAN.md\` could never pass.
      * ADD A FOOTPRINT CEILING: peak resident bytes with the ASR engine AND
        the polish child both loaded, asserted as a ceiling, on the declared
        floor machine (Y6-E names it). SEC-C turns on a 1.12 GB resident GGUF
        and Y4-E runs a multi-chunk pass over it while ASR is loaded; Y3-A caps
        only the audio buffers (~32 MB). "Budgets on the weakest supported Mac"
        cannot be enforced until the weakest Mac is named.

  `,
  acceptance: `
    test -f desktop/src-tauri/tests/long_take_budget.rs
    grep -q 'progress_events_per_minute_of_audio' desktop/src-tauri/tests/long_take_budget.rs
    grep -q 'resident_bytes_after_a_long_take_return_to_baseline' desktop/src-tauri/tests/long_take_budget.rs
    grep -q 'no_new_polling_timer_was_introduced' desktop/src-tauri/tests/long_take_budget.rs
    # PANEL: docs/loop/PLAN.md is UNTRACKED in git, so it does not exist in any
    # lane worktree and this grep could never pass. Budgets go in a tracked doc.
    test -f docs/BUDGETS.md
    grep -qE '[0-9]+ ?ms' docs/BUDGETS.md
    grep -q 'peak_resident_bytes_with_asr_and_polish_loaded' desktop/src-tauri/tests/long_take_budget.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test long_take_budget ; test $? -eq 0
  `,
})
