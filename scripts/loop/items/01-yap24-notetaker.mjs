// yap24-NT — THE NOTETAKER CHAIN. Written 2026-09-26 by the architecture audit
// (docs/ARCHITECTURE-AUDIT-2026-09-26.md §2). Wilson, dictated 2026-09-26: "The notetaker is not
// even working and that's one of the most important things — every component of that."
//
// ROOT CAUSE, verified from code AND from Wilson's own install (read-only copy of the SQLite DB):
//   * Recording works: a stop writes the mic WAV and sets the row to `transcribing`
//     (desktop/src-tauri/src/meeting_control.rs:652-656, :710-712).
//   * NOTHING EVER TRANSCRIBES IT. The meeting ASR job (meeting_asr.rs:1718 `MeetingAsr`, :1767
//     `run`, :614 `WarmEngineChunkAsr`) is constructed ONLY in two integration tests
//     (tests/meeting_dictation_preempts_transcription.rs:145, tests/matrix_new_asr_chunk_timeout.rs:108);
//     the module opens with `#![allow(dead_code)]` (meeting_asr.rs:52), which is how the compiler was
//     kept quiet about it. `Database::append_meeting_segments` (db.rs:1652) has no caller in src/.
//     tests/meeting_manual_start_stop.rs:158-162 asserts `transcribing` and stops — "(YV93 finishes
//     it)" — so 105 lib meeting tests + 30 integration tests are green around a dead pipeline.
//   * The summary command then refuses: "that meeting has no transcript to summarize" (lib.rs:4216-4218).
//   * Diarization is never invoked: `diarize::pool()` has no caller (meeting_matrix.rs:143).
//   * Then the retention sweep deletes the audio anyway: `purge_meeting_audio` (db.rs:1936-1972)
//     selects on `started_at` alone, run every hygiene pass (lib.rs:2966-3001), 7 days
//     (meetings.rs:73). Wilson's DB: 4/4 meetings stuck in `transcribing`, processed_through 0.0,
//     audio_kept 0, both WAV paths NULL, meetings/ dir 0 B. The audio is gone.
//   * And the system-audio verdict marks silence as denial: 3/4 of those rows (one of them kind
//     `in_person`) carry LOOKS_DENIED_MESSAGE (syscapture.rs:2324-2327) because nothing played for
//     DENIAL_GRACE = 3 s (syscapture.rs:2243, :2334-2341).
//
// ORDER IS THE DEPENDENCY CHAIN: capture durability -> transcript -> notes -> UI. This file is ONE
// lane, sequential, on purpose. Pass 1 of the yap24 loop runs this file alone:
//   args: {mode:'build', only:['yap24-NT'], panelApproved:[...NT ids the panel approved]}
// Every item is gated:'panel' until the Senior Panel on the 2026-09-26 audit rules.
//
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs and docs/loop/HARNESS.md.
// Never touch the bundle identifier or the data directory name. Never sandbox. Headless only:
// no test in this file opens a window, and no acceptance command needs a microphone or a TCC grant.
//
// ACCEPTANCE HYGIENE (Panel round 2, 2026-09-26): the command runner executes every acceptance line
// ON ITS OWN, so a standalone `export` line sets nothing for the lines after it. Every cargo/bash
// line in this file therefore carries YAP_DATA_DIR="$(mktemp -d)/yap-state" INLINE — without it,
// cargo test on the loop machine writes into Wilson's real ~/Library/Application Support/WilsonVoice.

ITEMS.push({
  id: 'yap24-NT0', prompt: 'yap24-NT', branch: 'loop/yap24-nt0-provision-the-notetaker-eval-model-and-corpus', gated: 'panel',
  title: 'Provisioning, not product: the pinned parakeet model sits sha256-verified in the lane cache and the meeting eval corpus is materialised at its fixed path',
  notes: 'NEW item added by the Senior Panel round-2 verify 2026-09-26. Pure infrastructure (no app code), so it is in the pass-1 panelApproved list. yap24-NT2 (WER/RTF gate) and yap24-NT9 (the e2e proof) DEPEND on it: both used to be unable to run their model-backed gates on a throwaway YAP_DATA_DIR, which is how the e2e proof ended up specified as SKIP-exit-0.',
  preflight: `
    test -x scripts/provision-notetaker-eval.sh
    test "$(shasum -a 256 "$HOME/code/wilson-voice-loop/cache/models/parakeet-unified-en-0.6b-Q8_0.gguf" | cut -c1-64)" = 4b50b6dd862bf6e346929aaf4f5eaacec003bfa3f56462d6c874b41ef2f38795
    (R="$(git rev-parse --show-toplevel)" && cd "$HOME/yap-eval-corpus/meetings" && shasum -a 256 -c "$R/desktop/src-tauri/tests/fixtures/meeting_eval_manifest.sha256")
  `,
  spec: `
    Panel: APPROVED for pass 1 (round-2 verify 2026-09-26) — infrastructure only, no product surface.
    DEPENDS: none. yap24-NT2 and yap24-NT9 depend on THIS.

    WHY: a throwaway YAP_DATA_DIR never has a model (app_paths.rs:71 puts models under
    root.join("models")), so every model-backed gate in this chain either touched Wilson's real
    install or skipped. The fix is to provision the two inputs ONCE, at fixed paths, verified by
    hash, and have each gate point at them explicitly — never at the real install, never skip.

    THE TWO FIXED PATHS (acceptance lines in NT0, NT2 and NT9 name them literally):
      - model:  $HOME/code/wilson-voice-loop/cache/models/parakeet-unified-en-0.6b-Q8_0.gguf
                (the lane cache; Drain removes only the target-* dirs and the worktrees, never
                cache/). This directory is what YAP_MODEL_DIR means everywhere in this file.
      - corpus: $HOME/yap-eval-corpus/meetings (the durable location meeting_eval.rs already
                defaults to, CORPUS_HOME_RELATIVE). This is what YAP_EVAL_CORPUS means.

    DO
      1. scripts/provision-notetaker-eval.sh (bash, set -euo pipefail, executable, idempotent):
         a. Read the pin from desktop/src-tauri/src/catalog.json — the models[] entry with
            id handy-computer/parakeet-unified-en-0.6b-gguf, its revision, and the files[] row with
            quant Q8_0 (filename, size_bytes, sha256). The catalog is the one source of truth; do
            not hardcode a second copy of the hash in the script.
         b. If the cached file exists and its sha256 matches, print "model ok" and move on.
            Otherwise: if Wilson's installed copy exists at
            ~/Library/Application Support/WilsonVoice/models/<filename> AND its sha256 matches,
            clone it READ-ONLY into the cache (cp -c, APFS clone — never move, never modify the
            installed copy). Else download to <file>.partial from
            https://huggingface.co/<id>/resolve/<revision>/<filename>, falling back to each
            catalog mirror at <mirror>/<id>/<revision>/<filename> (the same order models.rs
            download_urls uses); verify sha256 BEFORE the atomic rename; a mismatch deletes the
            partial and exits non-zero.
         c. Corpus: if $HOME/yap-eval-corpus/meetings verifies with
            shasum -a 256 -c desktop/src-tauri/tests/fixtures/meeting_eval_manifest.sha256 (run from
            the corpus dir), print "corpus ok". Otherwise grow it with the SHIPPED generator
            (cargo test --features custom-protocol --test meeting_eval meeting_eval_generate_corpus
            -- --ignored --nocapture, with YAP_DATA_DIR="$(mktemp -d)/yap-state" inline and
            YAP_EVAL_CORPUS pointing at the fixed path) and verify again. A regrown corpus whose
            bytes disagree with the committed manifest (a different macOS say voice build) is a
            hard failure with that sentence — NEVER rewrite meeting_eval_manifest.* to match.
         d. Print the two paths and exit 0 only when both verified.
      2. docs/loop/HARNESS.md: a short "Notetaker eval inputs" note naming the two paths, the
         script, and YAP_MODEL_DIR / YAP_EVAL_CORPUS.

    NOT: no Rust or TypeScript change; no write into Wilson's installed data dir (read + clone only);
    no audio committed; no edit to catalog.json or the eval manifest.
  `,
  acceptance: `
    test -x scripts/provision-notetaker-eval.sh
    bash -n scripts/provision-notetaker-eval.sh
    YAP_DATA_DIR="$(mktemp -d)/yap-state" bash scripts/provision-notetaker-eval.sh
    grep -q 4b50b6dd862bf6e346929aaf4f5eaacec003bfa3f56462d6c874b41ef2f38795 desktop/src-tauri/src/catalog.json
    test -f "$HOME/code/wilson-voice-loop/cache/models/parakeet-unified-en-0.6b-Q8_0.gguf"
    test "$(shasum -a 256 "$HOME/code/wilson-voice-loop/cache/models/parakeet-unified-en-0.6b-Q8_0.gguf" | cut -c1-64)" = 4b50b6dd862bf6e346929aaf4f5eaacec003bfa3f56462d6c874b41ef2f38795
    test -f "$HOME/yap-eval-corpus/meetings/lecture-15min/audio.wav"
    (R="$(git rev-parse --show-toplevel)" && cd "$HOME/yap-eval-corpus/meetings" && shasum -a 256 -c "$R/desktop/src-tauri/tests/fixtures/meeting_eval_manifest.sha256")
  `,
})

ITEMS.push({
  id: 'yap24-NT1', prompt: 'yap24-NT', branch: 'loop/yap24-nt1-never-purge-untranscribed-meeting-audio', gated: 'panel',
  title: 'RESIDUAL: launch recovers a crashed meeting (recover_orphaned_meetings gets its first production caller) and non-complete audio has a 30-day window and a 5 GB ceiling',
  preflight: `
    grep -v '^[[:space:]]*//' desktop/src-tauri/src/lib.rs | grep -q 'recover_orphaned_meetings('
    grep -q 'FAILED_AUDIO_RETENTION_DAYS' desktop/src-tauri/src/meetings.rs
    grep -q 'FAILED_AUDIO_CEILING_BYTES' desktop/src-tauri/src/meetings.rs
    test -f desktop/src-tauri/tests/meeting_failed_audio_ceiling.rs
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26 as a RESIDUAL item: #218 (7399ec5) already shipped the state-aware purge test and reconcile_stranded_meetings; only what the last paragraph lists remains.
    DEPENDS: none (first item of the chain — it stops the data loss before anything else lands)

    Panel revisions 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, HIGH, GROUNDED):
      - Retention keys off a NEW pipeline_done_at column (set only once transcription, summary
        and diarization all reach a terminal state), never off started_at — db.rs:1945-1969's
        cutoff on started_at alone lets audio purge between "transcription complete" and
        NT5's diarize read or a Retry re-decode. Add MIGRATION_6_MEETING_NOTES in meetings.rs
        (SCHEMA_VERSION = 6) for this column (shared with NT4's meeting_items table below).
      - Failed/partial-with-audio meetings get a LONGER retention window (>=30 days), not
        indefinite: at ~230 MB/hour of two-track audio a backlog of failed meetings otherwise
        grows without bound. Add a disk ceiling (default 5 GB retained meeting audio); past it
        the oldest non-complete audio purges first, one sentence about it in Settings.
      - At launch, BEFORE reconcile, call meeting::recover_orphaned_meetings(meetings_dir) —
        it already exists (meeting.rs:1599) but has NO production caller today (only a unit
        test and meeting_matrix.rs reference it), so a crash mid-meeting leaves the row stuck
        in "recording" forever. Wire its FinalizedMeeting results into finish_meeting /
        set_meeting_sys_wav_path via wav_for_track (never a positional index), set state from
        FinalizedMeeting.state, then hand it to yap24-NT2's queue.
      - "Complete with zero segments" is a documented VALID outcome (a genuinely silent
        meeting) — do not special-case it out of purge eligibility; use pipeline_done_at, not
        segment count, as the purge criterion.
      - Delete the per-track ASR progress ledger (<id>.tN.asr-progress.json, see NT2) and the
        kept per-track host-time anchors (<id>.tN.index.jsonl, see NT2) together with the WAV
        whenever audio is purged or the meeting is deleted — today neither is referenced by
        purge_meeting_audio or the delete cascade.
      - Every cargo/bash acceptance command below carries YAP_DATA_DIR="$(mktemp -d)/yap-state"
        INLINE (round-2 verify: a standalone export line does not persist — the runner executes
        each line on its own). Without it, cargo test on this machine has already written into
        ~/Library/Application Support/WilsonVoice (a stray probe file from an earlier run is
        proof) — never touch Wilson's real install.

    EVIDENCE
      - db.rs:1936-1972  purge_meeting_audio selects every meeting with a WAV path and
        started_at < cutoff. It does not look at state, so a meeting still in transcribing, partial
        or failed loses the only copy of its audio after AUDIO_RETENTION_DAYS (meetings.rs:73 = 7).
      - lib.rs:2960-2965 says why the purge is time-based: "there is no summarize stage yet (YV97)".
        The stage exists now (summarize.rs), but no transcript ever reaches it (see yap24-NT2).
      - Wilson's install, read-only DB copy 2026-09-26: 4 meetings, all state=transcribing,
        processed_through_seconds 0.0, audio_kept 0, mic_wav_path NULL, sys_wav_path NULL;
        ~/Library/Application Support/WilsonVoice/meetings is empty. yap.log shows each of them
        "stopped ... -> transcribing" and nothing afterwards.

    DO
      1. [SHIPPED by #218; the "at least one segment" clause is now the rule for the 7-day class only, see the last paragraph] Retention is STATE-AWARE: the 7-day purge applies only to a complete meeting that has at least one meeting_segments row. [SUPERSEDED 2026-10-26: any other state keeps its audio 30 days, bounded by the 5 GiB ceiling (R2 in the last paragraph), and the Meetings view (yap24-NT6) says so.] Both tracks age out together (the YV106 promise stays).
      2. reconcile_stranded_meetings(), run once at launch after the DB opens and BEFORE the first
         hygiene sweep:
           - state recording with no live capture  -> partial (crash mid-meeting; the capture
             journal already recovers what it can — reuse it, do not duplicate it);
           - state transcribing/summarizing whose WAV exists -> left as is and handed to the
             pipeline queue yap24-NT2 creates (until NT2 lands: left as is, logged once);
           - state transcribing/summarizing whose WAV is gone -> failed, error = one honest
             sentence ("The audio for this meeting was removed before Yap transcribed it."),
             audio_kept 0. Never silently complete.
      3. One log line per reconciled row with id, old state, new state.

    NOT
      - Do not change AUDIO_RETENTION_DAYS. Do not delete rows. Do not touch dictation retention.
      - Do not rename the data directory or any column.

    Tests (new file tests/meeting_retention_keeps_untranscribed_audio.rs):
      - a transcribing meeting older than the window keeps both WAV paths after purge;
      - a complete meeting WITH segments older than the window loses both;
      - a complete meeting with ZERO segments keeps its audio;
      - reconcile marks a transcribing row whose WAV is missing as failed with the sentence,
        and leaves a transcribing row whose WAV exists untouched.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - WHY A RESIDUAL: this item's old preflight passed on main (the #218 test file and reconcile_stranded_meetings exist), so the harness
        would have marked it already-done and every revision above that #218 did not ship would have vanished. The preflight now keys on
        the residual only.
      - RESIDUAL DO (names are contract; the acceptance greps them):
          R1. meeting::recover_orphaned_meetings (meeting.rs:1599) has only test callers today. Call it ONCE at launch, after the DB opens and
              BEFORE reconcile_stranded_meetings; wire each FinalizedMeeting into finish_meeting / set_meeting_sys_wav_path via wav_for_track
              (never a positional index) and set the row state from FinalizedMeeting.state, then hand the row to the NT2 queue.
          R2. purge_meeting_audio (db.rs:2052+) selects only state='complete' AND EXISTS segments, so every failed, partial or
              complete-with-zero-segments row keeps its audio forever (~230 MB/hour of two tracks). Add FAILED_AUDIO_RETENTION_DAYS = 30
              and FAILED_AUDIO_CEILING_BYTES = 5 GiB in meetings.rs: a row retained for any reason other than (complete AND at least one
              segment) is purged after 30 days, and past the 5 GiB ceiling the OLDEST such rows purge first until under it. One plain
              sentence in Settings says so. Test file tests/meeting_failed_audio_ceiling.rs (ordering, ceiling, the 7-day class untouched).
          R3. Purge and delete also remove the per-track ledger files and the kept anchors (<id>.*.asr-progress.json, <id>.t*.index.jsonl) together
              with the WAVs. Tested in the same file.
          R4. (folded in from the retired NT3) kind in_person must plan TrackBPlan::MicOnly with no badge and no error. meeting_kind_branch
              must assert it; add the assertion if it is absent.
      - VOID: the pipeline_done_at column and MIGRATION_6_MEETING_NOTES for it (and the "use pipeline_done_at, not segment count" bullet). Its
        two readers are gone: diarization is deferred (NT5) and the notes job (#250) reads transcript rows, not audio. The acceptance fails
        if pipeline_done_at appears. No schema change in this item.
      - Conflict that was resolved: DO step 1's "at least one segment" text contradicted the revision's pipeline_done_at criterion. The shipped
        rule (complete AND at least one segment, 7 days) stays; everything else falls under R2.
    Audit 2026-10-26: confirmed on the INSTALLED 0.9.3-dev (a740614) that recover_orphaned_meetings (meeting.rs:1599) is still called only from its test (meeting.rs:4026). Storage is 16-bit WAV at 1.92 MB/min per track (60 MB per 31-min track, 320 MB kept); consider FLAC/Opus under the 5 GB ceiling. See docs/audits/meeting-notetaker-audit-2026-10-26.md issues #11 and #17.
  `,
  acceptance: `
    grep -v '^[[:space:]]*//' desktop/src-tauri/src/lib.rs | grep -q 'recover_orphaned_meetings('
    grep -q 'FAILED_AUDIO_RETENTION_DAYS' desktop/src-tauri/src/meetings.rs
    grep -q 'FAILED_AUDIO_CEILING_BYTES' desktop/src-tauri/src/meetings.rs
    ! grep -rn 'pipeline_done_at' desktop/src-tauri/src
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_failed_audio_ceiling
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_retention_keeps_untranscribed_audio
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_audio_retention
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_kind_branch
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib meeting
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-NT2', prompt: 'yap24-NT', branch: 'loop/yap24-nt2-wire-the-meeting-transcription-pipeline', gated: 'panel',
  title: 'RESIDUAL: the shipped meeting pipeline gets its guards: live recording never counts toward max_yield, a crash-loop guard, waiting_for_model, asr_model on the row, and a REQUIRED WER/RTF run',
  preflight: `
    ( if ! ( test -f "$HOME/code/wilson-voice-loop/cache/models/parakeet-unified-en-0.6b-Q8_0.gguf" ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap24-NT0 has not provisioned the lane model cache; build NT0 first"; exit 0; fi; grep -q 'YAP_EVAL_REQUIRE' desktop/src-tauri/tests/meeting_eval.rs && grep -q 'YAP_MODEL_DIR' desktop/src-tauri/tests/meeting_eval.rs && grep -q 'asr_model' desktop/src-tauri/src/meeting_pipeline.rs && grep -q 'waiting_for_model' desktop/src-tauri/src/meeting_pipeline.rs && grep -q 'MAX_UNPROGRESSED_ATTEMPTS' desktop/src-tauri/src/meeting_pipeline.rs && grep -q 'catch_unwind' desktop/src-tauri/src/meeting_pipeline.rs && test -f desktop/src-tauri/tests/meeting_pipeline_residual.rs )
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26 as a RESIDUAL item: #218 (7399ec5) shipped meeting_pipeline.rs and the wiring test; only the guards in the last paragraph remain.
    DEPENDS: yap24-NT0 (the pinned model + eval corpus its WER/RTF gate runs against),
    yap24-NT1 (audio must not be purged out from under a queued job)

    Panel round-2 verify 2026-09-26 (applied — acceptance now enforces what the revision below says):
      - The WER/RTF run is IN the acceptance, not prose: meeting_eval runs with
        YAP_EVAL_REQUIRE=1, YAP_EVAL_CORPUS=$HOME/yap-eval-corpus/meetings and
        YAP_MODEL_DIR=$HOME/code/wilson-voice-loop/cache/models (both provisioned and
        hash-verified by yap24-NT0). Implement in tests/meeting_eval.rs: (a) YAP_EVAL_REQUIRE=1
        turns the CORPUS_ABSENT skip into a panic, and a missing model into a panic — never a
        green skip; (b) when YAP_MODEL_DIR is set, the Decoder links (symlink or cp -c clone) the
        pinned parakeet Q8_0 file from it into <YAP_DATA_DIR>/models/ before the first decode, and
        refuses to run if YAP_DATA_DIR is unset (it must never seed or read the default root);
        (c) print WER, real-time factor and lecture-15min stop-to-notes wall clock, and commit
        the measured numbers to docs/BUDGETS.md.
      - The launch call site is pinned: lib.rs's start path calls meeting_pipeline::spawn (that
        exact path — the acceptance greps for it; "or the equivalent" is gone).

    Panel revisions 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, 3x BLOCKING + HIGH, GROUNDED):
      - BLOCKING — per-track ledger collision: JsonProgressStore keys ONLY on meeting_id
        (meeting_asr.rs:1597-1599), so the system track's run() resumes from the mic track's
        processed_through_seconds and dedups chunk.index against the mic track's chunks. Key
        the ledger per track, e.g. run(&format!("{id}.t{track}")), and add a
        meeting_pipeline_wired test with two tracks of different lengths asserting each track's
        own processed_through and no chunk-index collision.
      - [VOID 2026-10-26: #218 shipped per-chunk append plus delete-past-ledger (meeting_pipeline.rs:17-21, :517), which already makes a resume idempotent; no one-shot merge transaction is required here, the host-time merge is NT11-0.] BLOCKING — segments must NOT land per-chunk. append_meeting_segments (db.rs:1652) is a
        non-idempotent INSERT with a fresh row id every call: writing "as each chunk completes"
        (as this spec said) double-writes every seam and duplicates rows on any resume-after-quit.
        Instead: run BOTH tracks' MeetingAsr to completion (interrupted == false), call the
        shipped merge_two_tracks_by_host_time(ledger_a.chunks, ledger_b.chunks, anchors, epochs,
        kind) (meeting_asr.rs:1123), then in ONE transaction DELETE FROM meeting_segments WHERE
        meeting_id=? and INSERT the merged spans, set state complete. Drive progress EVENTS from
        the ledger's processed_through_seconds, not from segment-row writes.
      - BLOCKING — the two-track merge needs the per-track host-time anchors
        (<id>.tN.index.jsonl), which finalize_meeting_marker deletes today "for a consumer that
        does not exist" (meeting.rs:1794-1798, :1842-1845). NT2 IS that consumer: keep the
        anchor files (or persist records+epochs to a new meeting_anchors table) through
        finalize, and clean them up only via NT1's purge/delete path, never independently.
      - HIGH — max_yield: Duration::from_secs(120) (meeting_asr.rs:1675) breaks "dictation
        always wins": a hands-free take over ~2 minutes (routine in Wilson's logs) makes meeting
        ASR take the engine back mid-take. A live-recording demand must never count toward
        max_yield; the 120 s cap applies only to a stuck "busy, not recording" flag. Add a test:
        a 180 s simulated dictation never observes a meeting chunk take the engine.
      - HIGH — no crash-loop guard: the launch-time requeue of every stranded "transcribing" row
        runs in-process, on the SAME warm engine dictation uses (unlike polish/diarize, which are
        sidecars). Two unexplained SIGSEGVs already exist in the audit (0.8.0, worker threads
        39/40). Add an attempt counter per meeting; two crash-without-progress launches ->
        failed with an honest sentence, never a silent retry loop. Wrap the worker body in
        catch_unwind so one panic marks that row failed and keeps the FIFO worker alive. Start
        the queue ~30 s after launch, after the dictation engine's first warm-up.
      - HIGH — enqueue every row that has audio and zero segments in state {transcribing,
        partial}, not "transcribing" only — stop() and finalize both downgrade to partial on a
        capture hole or spliced silence, and those meetings currently never reach the pipeline
        and never free their disk under NT1's new state-aware retention.
      - "The compiler is the proof the pipeline is wired" is FALSE — meeting_asr.rs's items are
        pub in a lib crate (Cargo.toml crate-type includes rlib), so rustc's dead_code lint never
        fires on them regardless of wiring. Drop that claim from the acceptance's rationale; the
        real proof is behavioral: meeting_pipeline_wired must drive MeetingController::stop (not
        the worker directly) and observe segments > 0, plus a source-scan grep that
        MeetingAsr is constructed outside meeting_asr.rs AND that lib.rs's start path spawns the
        pipeline (grep -q 'meeting_pipeline::spawn' src/lib.rs or the equivalent call site).
      - Reuse the SHIPPED EngineDemand impl: TranscriptionManager already implements it
        (meeting_asr.rs:600-604, exercised by tests/meeting_dictation_preempts_transcription.rs)
        — do not invent a second AppState-based demand. Build the audio source with
        WavWindows::open (meeting_asr.rs:507), never MemoryWindows (690 MB for a 3-hour meeting).
      - Model gating: meeting_availability_for's NoModel/UnknownModel checks only that a model id
        is configured, never that its files are on disk (app_paths.rs:71 puts models under
        root.join("models"), which a fresh/offline install lacks). A refusal on a missing-but-
        configured model must produce a NEW waiting_for_model state that KEEPS the audio and
        auto-requeues on the model-download-complete event, not a permanent "failed" row.
      - Pin meeting ASR to the catalog's recommended model (parakeet, Q8_0), not "whatever
        engine happens to be loaded" — PKG2's interim whisper-tiny path must never become the
        model a meeting is permanently transcribed by; record asr_model on the row, and NT1 only
        purges audio transcribed by the recommended model.
      - Measurability: run the existing WER harness (meeting_eval.rs, WER_GATE 0.02) against
        ~/yap-eval-corpus/meetings inside this item's acceptance when the corpus is present, and
        FAIL (not silently pass) when it is absent from a machine expected to have it (gate via
        YAP_EVAL_REQUIRE=1). Print real-time factor and stop-to-notes wall clock for
        lecture-15min into docs/BUDGETS.md — nothing today measures either number.
      - Every cargo/bash acceptance command below carries YAP_DATA_DIR="$(mktemp -d)/yap-state"
        INLINE — see the NT1 revision for why.

    EVIDENCE — the whole reason "the notetaker is not even working":
      - meeting_control.rs:652-656 sets transcribing with the comment "YV93's transcription
        pipeline moves it on to complete". There is no such pipeline in src/.
      - meeting_asr.rs:52 #![allow(dead_code)] over the module. MeetingAsr (:1718), its run()
        (:1767) and WarmEngineChunkAsr (:614) are constructed only in
        tests/meeting_dictation_preempts_transcription.rs:145 and tests/matrix_new_asr_chunk_timeout.rs:108.
      - db.rs:1652 append_meeting_segments: no caller outside db.rs.
      - tests/meeting_manual_start_stop.rs:158-162 asserts transcribing and ends — no test drives
        stop -> segments -> complete.

    DO
      1. A new module meeting_pipeline.rs owning ONE background worker thread and a FIFO of meeting
         ids. Enqueue on: a successful MeetingController::stop whose state is transcribing; and at
         launch, every row reconcile_stranded_meetings (yap24-NT1) left in transcribing with audio.
      2. The worker builds the shipped job — do not write a second chunker:
           MeetingAsr { store: JsonProgressStore under <meetings dir>/<id>.progress.json,
                        asr: WarmEngineChunkAsr over the ONE warm engine (TranscriptionManager),
                        demand: an EngineDemand that answers true while a dictation is recording
                                or busy (AppState recording/busy) — dictation always wins,
                        quit: the exit-teardown flag, config: MeetingAsrConfig::default() }
         and runs it per track (mic track, then the system track when sys_wav_path is set, tagged
         MIC_TRACK / SYSTEM_TRACK exactly as meeting.rs defines them).
      3. Segments land through Database::append_meeting_segments as each chunk completes (the FTS
         index follows through its existing triggers), processed_through_seconds advances through
         the existing ledger, and the row moves transcribing -> complete (or partial with the
         error sentence when a track failed). The English-only gate (meeting_availability_for) is
         consulted first and a refusal is a failed row with its sentence, not a silent skip.
      4. Emit the existing meeting status event after every chunk and at the end, carrying
         {id, state, processedThroughSeconds, durationSeconds}, so yap24-NT6/NT7 can draw progress.
      5. Remove #![allow(dead_code)] from meeting_asr.rs. Anything still dead after the wiring is
         deleted or given a caller — the compiler is the proof that the pipeline is wired.
      6. [MOVED TO yap24-NT9, which now owns the flag in its own DO and preflight] A headless CLI entry beside --transcribe-file (cli.rs): --transcribe-meeting <mic.wav>
         [--sys <sys.wav>] that runs the same worker against a scratch state root
         (YAP_DATA_DIR required, like --smoke) and prints the segment count and state.
      7. Update meeting_matrix.rs rows whose PolicyOnly cell existed only because the pipeline had
         no caller — a row flips to Test only with a test that proves the call site.

    NOT
      - No second ASR engine, no new model load: meeting ASR shares the warm engine and yields.
      - Never block the main thread or the hotkey path (spawn a named thread, never the Tauri
        async runtime for the decode).
      - Do not touch diarization here (yap24-NT5).

    Tests (tests/meeting_pipeline_wired.rs), stub ChunkAsr, no model, no microphone:
      - controller start -> stop -> pipeline drain => segments > 0, state complete, both tracks
        present for a two-track meeting, processed_through_seconds == duration;
      - a dictation demand mid-meeting preempts at a chunk boundary and the job resumes;
      - a quit mid-job leaves the ledger; relaunch resumes and the transcript equals an
        uninterrupted run (reuse the existing resume-seam assertions);
      - source scan: MeetingAsr is constructed in src/ outside meeting_asr.rs.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - WHY A RESIDUAL: the old preflight (test -f meeting_pipeline_wired.rs, no allow(dead_code), MeetingAsr constructed outside
        meeting_asr.rs) passed on main after #218, so this item was silently already-done and its guards were never going to be built.
      - RESIDUAL DO (shipped and NOT to be redone: the worker module, FIFO, per-track ledger, WavWindows source, progress event, dead_code removal):
          R1. max_yield. lib.rs (~:6790) passes MeetingAsrConfig::default() (max_yield 120 s, meeting_asr.rs:1674, :1935), so a hands-free
              take over two minutes hands the engine back to meeting ASR mid-take. A LIVE-RECORDING demand never counts toward max_yield; the
              120 s cap applies only to a stuck "busy, not recording" flag. Test in tests/meeting_pipeline_residual.rs: a simulated 180 s
              dictation never observes a meeting chunk take the engine.
          R2. Crash-loop guard. Add MAX_UNPROGRESSED_ATTEMPTS (2) in meeting_pipeline.rs with a per-meeting attempt counter (persist it beside the
              progress ledger): two launches that crash without progress mark the row failed with one honest sentence, never a silent retry
              loop. Wrap the worker body in catch_unwind so one panic fails that row and keeps the FIFO alive. Start the queue about 30 s
              after launch, after the dictation engine's first warm-up. Enqueue every row that has audio and zero segments in
              {transcribing, partial}, not transcribing only (verify; add if absent).
          R3. waiting_for_model. meeting_availability_for checks only that a model id is configured. A refusal on a configured-but-missing
              model becomes a NEW state waiting_for_model that KEEPS the audio and auto-requeues on the model-download-complete event;
              never a permanent failed row.
          R4. asr_model. Record the model that transcribed the row in an asr_model column, via the next free versioned migration step (take
              the next number at build time; rebase after #250). Meeting ASR is pinned to the catalog's recommended model (parakeet Q8_0);
              no other engine may transcribe a meeting.
          R5. Measurability. tests/meeting_eval.rs: YAP_EVAL_REQUIRE=1 turns a missing corpus or model into a panic, never a green skip;
              YAP_MODEL_DIR links the pinned model into <YAP_DATA_DIR>/models/ and the run refuses an unset YAP_DATA_DIR. Print WER, real-time
              factor and lecture-15min stop-to-notes wall clock and commit the numbers to docs/BUDGETS.md. Record peak RSS during transcription.
      - The CLI flag --transcribe-meeting (old DO step 6) is no longer here: yap24-NT9 owns it.
    Audit 2026-10-26: a 51.8 s dictation spoken right after a meeting stop waited on the meeting decode: asr_decode_ms=5166 after 'cancelling the in-flight meeting chunk decode' (16:31:38Z), vs 338 ms for a 22.8 s take that morning. Measure take latency while a meeting job runs. ASR RTF on real meetings: 0.011-0.028 idle, 0.13 under loop CI load. Per-chunk timing is NEW yap-audit-meeting-timings. See docs/audits/meeting-notetaker-audit-2026-10-26.md issues #8 and #10.
  `,
  acceptance: `
    grep -q 'YAP_EVAL_REQUIRE' desktop/src-tauri/tests/meeting_eval.rs
    grep -q 'YAP_MODEL_DIR' desktop/src-tauri/tests/meeting_eval.rs
    grep -q 'asr_model' desktop/src-tauri/src/meeting_pipeline.rs
    grep -q 'waiting_for_model' desktop/src-tauri/src/meeting_pipeline.rs
    grep -q 'MAX_UNPROGRESSED_ATTEMPTS' desktop/src-tauri/src/meeting_pipeline.rs
    grep -q 'catch_unwind' desktop/src-tauri/src/meeting_pipeline.rs
    grep -q 'meeting_pipeline::spawn' desktop/src-tauri/src/lib.rs
    ! grep -n "allow(dead_code)" desktop/src-tauri/src/meeting_asr.rs
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_pipeline_residual
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_pipeline_wired
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_manual_start_stop
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_dictation_preempts_transcription
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib meeting
    YAP_DATA_DIR="$(mktemp -d)/yap-state" YAP_EVAL_REQUIRE=1 YAP_EVAL_CORPUS="$HOME/yap-eval-corpus/meetings" YAP_MODEL_DIR="$HOME/code/wilson-voice-loop/cache/models" cargo test --features custom-protocol --test meeting_eval -- --nocapture --test-threads=1
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-NT3', prompt: 'yap24-NT', branch: 'loop/yap24-nt3-silence-is-not-a-denied-permission', gated: 'panel',
  title: 'Silence is not a denial: an in-person meeting never attaches the system tap, and a quiet call is never told macOS refused it',
  preflight: `
    grep -q "output_was_running" desktop/src-tauri/src/syscapture.rs
    test -f desktop/src-tauri/tests/system_audio_verdict_silence_is_not_denial.rs
  `,
  spec: `
    Panel: pending
    DEPENDS: yap24-NT2 (the verdict is written on the same stop path the pipeline now owns)

    Panel revisions 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, HIGH, GROUNDED):
      - The code already has the right per-process probe field (TapEnvironment.
        system_output_active, syscapture.rs:680-683) and the right verdict function
        (TapLiveness::verdict, :501-518), but observe_environment (:2617-2619) has NO caller
        anywhere in src/, so env stays Default and the stop path falls back to the naive,
        device-level permission_verdict (:2334, :2628) this item was meant to replace. Implement
        the probe: kAudioHardwarePropertyProcessObjectList, then per process (skipping getpid())
        kAudioProcessPropertyPID and kAudioProcessPropertyIsRunningOutput; sample it on the pump
        tick and call observe_environment. Route BOTH the stop-path and pre-warm verdicts through
        TapLiveness::verdict and delete the LooksDenied arm of the old permission_verdict.
      - A muted call app (Zoom/Meet) or a mid-meeting default-output-device change (AirPods
        connect) still makes kAudioDevicePropertyDeviceIsRunningSomewhere true with no audible
        output. Evaluate over the WHOLE meeting, track every device that was default at any
        point during it, and require the tap to have delivered nothing for the entire meeting
        AND output to have run >=60s before writing LooksDenied; anything short of that is
        Unknown, and Unknown is never stored as an error. Never flip the setup row to LooksDenied
        from a single meeting — require two consecutive ones.
      - Fix the pre-flight: grep -q "output_was_running" is satisfied by a comment. Gate on the
        test file's existence and passing, not the string.

    EVIDENCE
      - syscapture.rs:2243 DENIAL_GRACE = 3 s; :2334-2341 permission_verdict returns LooksDenied
        whenever the tap delivered no non-zero sample for 3 s. Nothing playing looks identical.
      - syscapture.rs:2324-2327 LOOKS_DENIED_MESSAGE ("macOS has not granted System Audio
        Recording to Yap, and it will not ask again") is stored as the meeting error.
      - Wilson's DB: 3 of 4 meetings carry that sentence, including cc9f6153 whose kind is
        in_person — a room recording that never needed system audio at all.
      - settings_kv meeting_system_audio_setup_ack_v1 = "ran" (2026-08-20), so track_b_plan
        (syscapture.rs:2462-2485) attaches the tap on every meeting.
      - Apple: process taps are AudioHardwareCreateProcessTap, macOS 14.2+
        (https://developer.apple.com/documentation/coreaudio/audiohardwarecreateprocesstap(_:_:));
        the purpose string is NSAudioCaptureUsageDescription, macOS 14.2+
        (https://developer.apple.com/documentation/bundleresources/information-property-list/nsaudiocaptureusagedescription).
        There is no public API to read that grant back, so the verdict must stay a heuristic —
        make it an honest one.

    DO
      1. kind in_person => TrackBPlan::MicOnly with NO badge and NO error. The tap is for the
         can't-join-the-call case (Wilson 2026-08-10: IRL first).
      2. The verdict gains an input: was the default output device running during the window
         (kAudioDevicePropertyDeviceIsRunningSomewhere on the default output device,
         https://developer.apple.com/documentation/coreaudio/kaudiodevicepropertydeviceisrunningsomewhere).
         No delivery + output idle => Unknown ("No system audio played during this meeting."),
         never LooksDenied. LooksDenied only when output was running and the tap stayed silent.
      3. The meeting row stores the verdict sentence only when it is LooksDenied or Failed.
      4. The Settings pre-warm verdict follows the same rule, so an idle Mac cannot write a
         sticky LooksDenied into the setup row.

    Tests: pure verdict table (delivery x output_was_running x ran_for) in
    tests/system_audio_verdict_silence_is_not_denial.rs; an in_person kind yields MicOnly with
    no badge; existing meeting_track_b_wiring stays green.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - RETIRED. Not in panelApproved. Its preflight (output_was_running in syscapture.rs plus the verdict test file) passes on main
        (7399ec5), so the harness would report already-done anyway. The one residual line (kind in_person plans MicOnly, no badge, no error;
        unverified) is folded into yap24-NT1 as R4 with its test in meeting_kind_branch. Nothing else here remains to build.
    Audit 2026-10-26: on a real virtual call (445654ee) the tap went all_zero_buffers at 12:00, rebuild 2/3 timed out, and the call track ends at 690 s of 883 s. On a kind-unknown meeting (d8761579) no_system_audio_observed after 3 rebuilds, so the meeting was mic-only for 9 min. Retarget the tap on an output-route change instead of rebuilding the same tap. See docs/audits/meeting-notetaker-audit-2026-10-26.md issue #5.
  `,
  acceptance: `
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test system_audio_verdict_silence_is_not_denial
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_track_b_wiring
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_kind_branch
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib syscapture
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-NT4', prompt: 'yap24-NT', branch: 'loop/yap24-nt4-notes-summary-and-action-items-automatically', gated: 'panel',
  title: 'RESCOPED after #250: the default notes are MEASURED (action-item recall and precision at least 0.8 on a planted-commitment fixture) and installing the notes model leaves polish_model byte-unchanged',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/summary_jobs.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 250 (default meeting notes) has not landed; land it, then build NT4"; exit 0; fi; test -f desktop/src-tauri/tests/meeting_notes_recall_eval.rs && test -f desktop/src-tauri/tests/notes_model_install_keeps_polish_model.rs && test -f desktop/src-tauri/tests/notes_owner_calendar_name.rs && grep -q 'summary recall' docs/BUDGETS.md )
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26 and RESCOPED after PR #250 (notes as Markdown sections in meetings.summary, summary_jobs.rs, polish_model kept as the user override). The meeting_items table, the summary_model split and MIGRATION_6 in the text below are VOID.
    DEPENDS: yap24-NT2 (segments must exist), SEC-C (merged #184 — the polish model install path)

    Panel revisions 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, HIGH, GROUNDED):
      - Installing the summary model must NOT silently turn on LLM polish for dictation:
        summarize_meeting_blocking and the dictation-polish path both read state.settings.
        polish_model today (lib.rs:4225-4227, :320-325), and SEC-C's one-click install writes
        that same setting (lib.rs:4823-4826). Give summaries their OWN setting, summary_model,
        independent of dictation's polish_model. Add a test asserting polish_model is
        byte-unchanged after a summary-model install.
      - Schema: rename the meeting_actions table to meeting_items(kind CHECK IN ('action',
        'decision','question'), text, speaker_label, segment_id, start_seconds). Drop the
        'owner' column — summarize.rs:1026-1040 is explicit that the speaker is not a claim
        about who OWES the action. Map the positional seg_NNNN label to a real segment_id inside
        summarize before persisting (the mapping never otherwise leaves that process). Write the
        rows and meetings.summary in ONE transaction so the two stores cannot drift on a
        re-summarize. Add this table via the SAME MIGRATION_6 as NT1's pipeline_done_at, not an
        unversioned ALTER TABLE/CREATE TABLE IF NOT EXISTS — db.rs's own comment says shipped
        migration steps are immutable and its ad hoc let _ = conn.execute("ALTER TABLE ...")
        discards errors (db.rs:476-522, finding #26 cited at :480).
      - Give the summary job the SAME EngineDemand suspend-between-map-chunks behavior as
        MeetingAsr: on an 8 GB Mac, the ASR engine (warm 15 min), the dictation polish sidecar
        (warm for the process lifetime) and a second Qwen-1.5B summary sidecar together approach
        ~3 GB resident. Unload the ASR engine before summarizing when no dictation is pending, or
        route the summary through the already-warm polish sidecar on a low-priority queue.
        Record peak RSS (app + sidecars) during a summary into docs/BUDGETS.md with a ceiling
        (e.g. 2.5 GB on 8 GB machines) that falls back to summarizing on the next idle tick.
      - Add ONE measured eval: extend the synthetic corpus generator to plant K explicit
        commitments in a 15-minute fixture, run the REAL 1.5B sidecar (#[ignore] — the loop runs
        it only when the model is installed) and gate action-item recall >= 0.8 and precision
        >= 0.8, numbers committed to docs/BUDGETS.md. Shape/groundedness validators alone do not
        tell Wilson whether the notes are useful.

    EVIDENCE
      - lib.rs:4201-4260 summarize_meeting is a MANUAL command and refuses when there are no
        segments (:4216-4218) — which, before yap24-NT2, is every meeting.
      - The notetaker plan (Obsidian Notes/Yap-Notetaker-Epic-Plan-2026-08-10.md, schema
        meetings / meeting_segments / speaker_profiles / meeting_actions) is only partly real:
        Wilson's DB has no meeting_actions table (sqlite .tables, 2026-09-26).
      - The summary is a free-text column (meetings.summary). Action items with evidence links
        were the Read-AI-style promise ("transcript, summary, NEXT ACTIONS" — Wilson 2026-08-10).

    DO
      1. When the pipeline completes a meeting, enqueue a summarize job on the same worker (after
         transcription, never concurrently with it). It spawns its OWN sidecar session exactly as
         summarize_meeting_blocking does (lib.rs:4221-4223) so dictation is never starved.
      2. No local summary model installed => state stays complete, a new field
         summary_status = 'needs_model' is set, and the UI (yap24-NT6) offers the one-click
         install SEC-C already ships. Never an error toast.
      3. Migration: meeting_actions(id INTEGER PRIMARY KEY, meeting_id TEXT NOT NULL REFERENCES
         meetings(id) ON DELETE CASCADE, idx INTEGER NOT NULL, text TEXT NOT NULL, owner TEXT,
         evidence_segment_id INTEGER). The summarizer's structured output (summarize.rs, GBNF
         JSON) writes the action items there; each cites the segment it came from.
      4. Markdown export (the existing export path) gains Summary and Action items sections above
         the transcript.
      5. The manual Summarize command keeps working and re-runs the same job (idempotent: the
         empty-summary refusal at lib.rs:4250-4262 stays).

    Tests (tests/meeting_notes_auto_summary.rs, stub summary client): completion enqueues a
    summary; actions land as rows with evidence ids; no model => needs_model and no error;
    deleting a meeting cascades its actions (extend meeting_delete_cascade); export contains
    both sections.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - WHY: PR #250 (open, landing today) already owns the product half: default 1.5B notes model, notes stored as Markdown sections in
        meetings.summary, summary_jobs.rs, polish_model kept as the user's override. This item's old schema (meeting_items, a summary_model
        setting, MIGRATION_6) contradicts a binding ledger decision and duplicates #250, so it is VOID. Action items stay Markdown, not rows;
        rows are added only if a later item (X1, X3) is approved and needs them.
      - RESIDUAL DO, after #250 is on main (the preflight holds this item until summary_jobs.rs exists):
          R1. Provisioning: extend scripts/provision-notetaker-eval.sh to also verify (and APFS-clone read-only from the installed copy, or
              download per the polish catalog pin) the qwen2.5-1.5b Q4_K_M notes model into the lane cache next to the parakeet file. Same
              rules as NT0: hash from the catalog, never a second copy of the hash in the script.
          R2. Measured eval: extend the synthetic corpus generator to plant K explicit commitments in the 15-minute fixture and add
              tests/meeting_notes_recall_eval.rs, #[ignore], running the REAL 1.5B sidecar through summary_jobs. Gate action-item recall >= 0.8 and
              precision >= 0.8; under YAP_EVAL_REQUIRE=1 a missing model is a panic, not a skip. Commit both numbers to docs/BUDGETS.md
              under a heading containing "summary recall".
          R3. tests/notes_model_install_keeps_polish_model.rs: installing or selecting the notes model leaves the dictation polish_model setting
              byte-unchanged (read it before and after).
          R4. Memory: record peak RSS (app plus sidecars) during a summary in docs/BUDGETS.md against the 8 GB floor machine, and make the summary
              job run serialized with ASR, never concurrently with a live decode, falling back to the next idle tick above a stated ceiling.
      - Seat evidence: ASR warm (15 min) plus the 1.5B notes sidecar is about 2.6-3.0 GB resident, about 4 GB if the polish and notes
        sidecars do not share mmap'd weights; next to a live Zoom on 8 GB that swaps, which is why R4 is a gate and not advice.
    Audit 2026-10-26: all 3 meetings PR 250 summarized on Wilson's Mac grade 1/5. The Summary is 0.91-0.97 verbatim (4-gram containment), key points are raw lines (backfill_key_points), small talk is filed as actions, owners are wrong under echo, and there are up to 8 cross-section duplicates. Rejections per run: 17-38 v3_ungrounded + 3-17 v2_runaway. Add to this eval: Summary verbatim ratio <= 0.5 and owner accuracy. The first post-upgrade meeting auto-downloads 1.1 GB (allows_auto); make that visible before the run. Mechanical fixes are NEW yap-audit-notes-guards; strategy is NEW yap-audit-notes-strategy. See docs/audits/meeting-notetaker-audit-2026-10-26.md issues #2 and #18.

    Panel revisions 2026-10-26 (Senior Panel 3, two seats + synthesis; AI/models and PM seat; applied as gates):
      - R2 EVAL, RESCOPED. The planted fixture is a 15-minute synthetic (macOS say) meeting containing 6 explicit commitments ("I will send the deck by Friday"),
        4 implicit commitments ("I can get you the specs"), 2 decisions, 6 small-talk distractors that use "going to" or "'ll" (generic_commitment lets them through),
        and at least 2 commitments inside the last 20% of the meeting. It runs through summary_jobs alongside call-bleed-2voice (yap-audit-eval-call-bleed). Gates, all
        asserted by tests/meeting_notes_recall_eval.rs and committed to docs/BUDGETS.md under the heading containing "summary recall":
          action recall >= 0.8, action precision >= 0.8, decision recall >= 0.5, Summary 4-gram verbatim ratio <= 0.5, cross-section duplicates = 0,
          items per cited segment <= 2, owner accuracy >= 0.8 OR the owner left blank ("Someone here" counts as blank: it makes no claim).
        Explicit-only plants reach 0.9 recall while real meetings with implicit commitments and echo still fail; that is why the implicit, distractor and
        end-of-meeting plants are in the gate.
      - HUMAN HALF, numbers only: run scripts/eval/compare-reference.py --notes on a COPY of the 31-minute real meeting's DB and paste the verbatim ratio, duplicate
        count, max items per segment and owner counts in the PR body. Never paste text.
      - R5 (the v1 remote-name path, which also settles the deferred yap-audit-remote-speaker-names): a pure fn other_party_name(attendees, self) -> Option<String> that returns the
        display name when exactly one non-self calendar attendee exists and None for zero or two-plus. A Them-track item's owner in the NOTES uses that name when it is Some,
        otherwise reads "Them". Transcript labels stay Me / Them. It reads the attendees yap-notetaker-detect stores from the calendar match (opt-in); if that item stores
        none when this builds, ship the pure fn and its test and leave the wiring to that item (say so in the PR body). No new permission, no Accessibility read.
        tests/notes_owner_calendar_name.rs: one attendee -> name; none -> None; three -> None; the self address is excluded.
      - The owner-claim rules (mic-track "Someone here", echo owner) live in yap-audit-notes-guards; arms E and C are measured by yap-audit-notes-strategy, which also owns the honest
        fallback copy. This item owns the eval and its numbers only.
  `,
  acceptance: `
    test -f desktop/src-tauri/src/summary_jobs.rs
    test -f desktop/src-tauri/tests/notes_owner_calendar_name.rs
    ! grep -rn 'meeting_items' desktop/src-tauri/src
    ! grep -rn 'summary_model' desktop/src-tauri/src
    grep -q 'summary recall' docs/BUDGETS.md
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test notes_model_install_keeps_polish_model
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test notes_owner_calendar_name
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_notes_recall_eval
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test summary_default_notes
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib summarize
    YAP_DATA_DIR="$(mktemp -d)/yap-state" YAP_EVAL_REQUIRE=1 YAP_MODEL_DIR="$HOME/code/wilson-voice-loop/cache/models" cargo test --features custom-protocol --test meeting_notes_recall_eval -- --ignored --nocapture --test-threads=1
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-NT5', prompt: 'yap24-NT', branch: 'loop/yap24-nt5-diarization-joins-the-pipeline', gated: 'panel',
  title: 'Who said what: in-person meetings run the shipped diarization sidecar after transcription, with the honest accuracy framing',
  preflight: `
    grep -rln "diarize::pool()" desktop/src-tauri/src | grep -vE "diarize\\.rs|meeting_matrix\\.rs" | grep -q .
    test -f desktop/src-tauri/tests/meeting_pipeline_diarizes_in_person.rs
  `,
  spec: `
    Panel: DEFERRED 2026-09-26T17:35:00Z — see docs/loop/DEFERRED.md. Excluded from pass-1 panelApproved.
    Re-approve only after Y11-A..Y11-E are merged AND Y11-F's real-voice DER/FAR is measured
    against a stated threshold. DEPENDS is prose the harness does NOT enforce (no 'DEPENDS'
    handling exists in template.mjs/build.mjs — grep confirms it), and pass 1 runs
    only:['yap24-NT'], which hard-skips every Y11-* id, so an approved NT5 would build speaker
    enrollment on a path independently measured at FAR 1.000 (Y11-C) with DER 0.34-0.45 on clean
    synthetic voices (Y11 audit). Meeting notes that confidently attribute words to the wrong
    person are worse than notes with no speaker labels — 3 of 5 panel seats named this the single
    item to kill from pass 1. yap24-NT6/NT7 already render labels only when present, so leaving
    NT5 out costs nothing else in the chain.
    DEPENDS: yap24-NT2, Y11-A..Y11-E (the six parked diarization defects — FAR 1.000 on the shipped
    enrollment path, split_partition seeding, the pinned-digest skip — must be fixed first or this
    item ships known-wrong labels)

    EVIDENCE
      - meeting_matrix.rs:141-145: "diarize::pool() has no caller, no meeting is ever handed to a
        sidecar".
      - yap23 shipped the sidecar (yap-diarize, sherpa-onnx CAM++ 192-dim) and clustering (#141),
        and parked six items (#142-#146, #149, issues #150-#155).
      - Eval numbers are floors on a synthetic say-voice corpus (DER 0.34-0.45) — Y11-F gates the
        real-voice corpus.

    DO
      1. For kind in_person (and unknown), after transcription completes, hand the mic track to
         diarize::pool() and attribute clusters to segments with the shipped
         attribute_clusters / rank_and_floor path. kind call keeps track labels (You / Them).
      2. New or unknown voice => the "who is this?" prompt the plan specified (one question per
         cluster, never per segment), stored in speaker_profiles.
      3. UI copy states the accuracy honestly: "Speaker labels are a best guess — tap to fix."
      4. A diarize failure never fails the meeting: transcript and notes stand, labels are absent,
         one sentence says why.

    Tests: tests/meeting_pipeline_diarizes_in_person.rs with a stub DiarizeClient; a failing
    sidecar leaves the meeting complete with no labels.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - DEFER: after the first 100 users (not in panelApproved). Y11-A..E are not on main and speaker labels measured at FAR 1.000.
      - Regex bug fixed even though deferred: the preflight used grep -v with a BRE alternation, where | is a literal, so the comment at
        meeting_matrix.rs:143 satisfied line 1. It is grep -vE now, with escaped dots.
      - Single owner: the ungated yap-notetaker-notes item no longer wires diarize::pool() (its step was struck today), so this item is the
        ONLY owner of diarization in the meeting pipeline.
    Audit 2026-10-26: on a 25-min in-person meeting (ba4dac81) every one of 47 rows reads 'Speaker' and cluster_index is NULL on every segment in the DB; yap-diarize is bundled in /Applications/Yap.app but never called. On the 31-min call a second local voice (75% of its words in the mic track) is labelled 'Me'. See docs/audits/meeting-notetaker-audit-2026-10-26.md issues #6 and section 10b.
  `,
  acceptance: `
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_pipeline_diarizes_in_person
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_cluster_attribution
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib diarize
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test -p yap-diarize --release
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-NT6', prompt: 'yap24-NT', branch: 'loop/yap24-nt6-meetings-view-shows-every-state', gated: 'panel',
  title: 'The Meetings view tells the truth about every meeting: progress, notes, failure with Retry, audio kept or expired',
  preflight: `
    ( if ! ( test -f desktop/src/meetings/NotesView.tsx ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 250 (NotesView) has not landed; land it, then build NT6"; exit 0; fi; grep -qE 'canRetry\\([^)]*audioKept' desktop/src/meetings/progress.ts && ! grep -q 'canRetry(m.state)' desktop/src/views/Meetings.tsx && ! test -e desktop/src/meetings/viewState.ts )
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26.
    DEPENDS: yap24-NT2, yap24-NT4 (states and fields it renders)

    EVIDENCE
      - views/Meetings.tsx:339-347: a transcribing meeting with no segments renders "Yap is still
        working through the audio." — forever, because nothing was working through it.
      - No progress, no failed/partial rendering with a way out, no Retry, no summary-model state,
        no action items (the summary panel at :332-337 is one paragraph).
      - notetaker_status (lib.rs, the 14.4 gate sentence) reaches only the Settings setup step
        (meeting_matrix.rs row 12b).

    DO
      1. [VOID 2026-10-26: no viewState.ts; extend desktop/src/meetings/progress.ts, see the last paragraph] A pure module desktop/src/meetings/viewState.ts: meetingViewState(meeting, segmentsCount)
         -> { badge, headline, detail, progress: 0..1 | null, actions: ('retry'|'summarize'|
         'install-model'|'export'|'delete')[] } covering recording, transcribing (with
         processedThroughSeconds / durationSeconds), summarizing, complete, complete +
         needs_model, partial, failed, audio expired. Every state has exactly one headline.
      2. Meetings.tsx renders it: a progress bar that moves with the pipeline event, the notes
         (summary + action items, each item linking to its transcript segment), Retry for
         failed/partial (re-enqueues through a new retry_meeting command), Export Markdown.
         Panel revision 2026-09-26T17:35:00Z: Retry is offered ONLY when a WAV path exists and
         audio_kept = 1 — NT1 marks old rows failed with their audio already gone, and a Retry
         button that can never work is worse than none. meetingViewState needs a test for the
         no-audio failed case (actions: delete/export only).
      3. The empty state offers "Start a meeting" with the kind picker (in person / call) and
         carries notetaker_status when system audio is unavailable.
      4. Follow the token layer (Y5-A) and the empty/loading/error pattern (Y5-B) if merged;
         otherwise the existing App.css tokens — no new colours.

    Tests: [progress.test.ts, not viewState.test.ts] — one case per state, the progress arithmetic, and "no state renders
    an empty headline".

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - REAL DEFECT, LIVE ON MAIN: Meetings.tsx:289 calls canRetry(m.state) (progress.ts:58), which offers Retry on failed and partial rows even
        when audio_kept = 0 and no WAV exists, which is exactly what this item's Retry rule forbids.
      - CHANGE: extend progress.ts to canRetry(state, audioKept) and have Meetings.tsx (and #250's NotesView) pass the row's audio_kept; Retry is
        offered ONLY when a WAV path exists and audio_kept = 1. Do NOT create viewState.ts: it would duplicate #218's progress.ts and #246/#250's
        state mapping. progress.test.ts gains the no-audio failed case (actions: delete and export only) and a two-argument canRetry table.
      - The "N lines hidden as speaker echo - Show" line stays DEFERRED to the NT11 pass. Rebase on #246 and #250 before starting (the
        preflight holds this item until NotesView.tsx exists).
    Audit 2026-10-26 (installed 0a87249): the detail view prints the raw state 'complete' (Meetings.tsx:428), cards count 'segments' that are 30 s windows (:320-321), the detail page 'does not refresh itself' (:452), and Write/Retry notes exist only on the card (:339-350). Notes are render-only (NotesView.tsx). Rewrite is NEW yap-audit-rewrite-notes. See docs/audits/meeting-notetaker-audit-2026-10-26.md issue #12.
  `,
  acceptance: `
    grep -qE 'canRetry\\([^)]*audioKept' desktop/src/meetings/progress.ts
    ! grep -q 'canRetry(m.state)' desktop/src/views/Meetings.tsx
    ! test -e desktop/src/meetings/viewState.ts
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/meetings
    npm test
    npm run build
  `,
})

ITEMS.push({
  id: 'yap24-NT7', prompt: 'yap24-NT', branch: 'loop/yap24-nt7-pill-and-tray-follow-the-meeting-after-stop', gated: 'panel',
  title: 'The pill and the menu bar follow the meeting after stop: transcribing 42%, notes ready, or what went wrong',
  preflight: `
    grep -q "notes-ready" desktop/src/pill/meeting.ts
    grep -q "notes-ready" desktop/src/pill/meeting.test.ts
    grep -q "MEETING_PROGRESS_EVENT" desktop/src/pill/meeting.ts
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26.
    DEPENDS: yap24-NT2, yap24-NT6

    EVIDENCE
      - pill/MeetingBadge.tsx:2 "the pill's persistent recording state (YV95)" — the badge knows
        recording and nothing after it; the moment a meeting stops the pill goes quiet while the
        work (now real, yap24-NT2) happens.
      - The tray meeting item toggles start/stop only (meeting_control.rs:733-757 toggle).

    DO
      1. pill/meeting.ts gains post-stop phases: processing (with percent), notes-ready (holds
         until clicked or 10 s), failed (sentence). Driven by the pipeline event from NT2 — no
         polling.
      2. Both characters render them through the shell (Y5-K owner decision: the shell owns
         phases, a character only owns art).
      3. The tray shows "Meeting notes ready — Open" and a macOS notification (the notification
         plugin is already initialised) when a meeting completes while Yap is in the background.
      4. Clicking notes-ready opens that meeting in the Meetings view (navigate event).

    Tests: pill/meeting.test.ts — event sequence recording -> processing 0..100 -> notes-ready;
    failure path; a dictation during processing still shows the dictation phases (dictation
    outranks the badge).

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - This is the notetaker wow moment: "Meeting notes ready - Open" arrives while the user is doing something else. Today the pill goes quiet
        after stop (MeetingBadge knows recording and nothing after it; pill/meeting.ts has no post-stop phase).
      - CHANGE: consume the EXISTING meeting:progress event (MEETING_PROGRESS_EVENT, emitted by meeting_pipeline.rs:52 and typed in
        meetings/progress.ts:15). No new event, no polling; the acceptance fails on a setInterval in pill/meeting.ts. The notes-ready phase
        reads its state from the same payload (state complete) and #250's summary-ready state.
      - Pill matrix: once scripts/pill-matrix.mjs exists (yap24-UI3), run node scripts/pill-matrix.mjs --only processing and --only notes-ready
        and attach the PNGs to the PR (never commit them).
  `,
  acceptance: `
    grep -q "MEETING_PROGRESS_EVENT" desktop/src/pill/meeting.ts
    ! grep -n "setInterval" desktop/src/pill/meeting.ts
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/pill
    npm test
    npm run build
  `,
})

// yap24-NT8 — CUT 2026-10-26 by the Senior Panel plan-audit (object removed; the harness has no retired/status field, removal is the mechanism).
// Why: a duplicate of the ungated yap-notetaker-detect (same calendar_prompt_policy.rs, same EventKit prompt; NT8 default OFF, detect default ON)
// that also adds a permanent Calendars TCC permission and fixes nothing in stop -> transcript -> notes. One owner now: yap-notetaker-detect,
// whose calendar signal is opt-in and whose app signal needs no permission. The original spec is in git history (commit before 2026-10-26).

ITEMS.push({
  id: 'yap24-NT9', prompt: 'yap24-NT', branch: 'loop/yap24-nt9-notetaker-end-to-end-proof', gated: 'panel',
  title: 'Phase-closing proof: one headless command runs a two-track fixture through capture, transcript, notes and export',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/tests/meeting_pipeline_residual.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap24-NT2 residual (guards) has not landed; run the review pass, then build NT9"; exit 0; fi; grep -q 'transcribe-meeting' desktop/src-tauri/src/cli.rs && test -x scripts/notetaker-e2e.sh && grep -q "yap24" docs/MEETING-DEMO.md )
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26.
    DEPENDS: yap24-NT0 (the provisioned model + corpus the proof runs against), yap24-NT1-NT4,
    yap24-NT6, yap24-NT7 (NT5 and NT8 are DEFERRED — see their own Panel revisions
    above; NT9 does not wait on either)

    Panel revisions 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, BLOCKING, GROUNDED — 4 of 5 seats):
      - The phase-closing proof must actually run. Acceptance today is test -x + bash -n +
        two cargo tests — notetaker-e2e.sh itself is NEVER executed, and even when it is, it
        SKIPs (exit 0) on ANY throwaway YAP_DATA_DIR because a fresh scratch root never has a
        model (app_paths.rs:71 puts models under root.join("models")). "Reuse the repo's meeting
        fixtures, do not add audio" cannot be honored either: tests/fixtures has one 2.5 s mono
        WAV — no two-track meeting audio exists in the repo. This repeats the exact
        "verification that verifies nothing" failure this whole chain exists to end.
      - FIX: acceptance runs YAP_E2E_REQUIRE=1 bash scripts/notetaker-e2e.sh for real (added
        below). The script takes YAP_MODEL_DIR / YAP_EVAL_CORPUS overrides pointing at a
        lane-cached, sha256-verified parakeet model and a two-track fixture; when
        YAP_E2E_REQUIRE=1 is set, SKIP becomes a hard failure (non-zero exit) instead of exit 0.
        (Round-2 verify: SKIP-as-pass is removed entirely — see DO step 1.)
      - Commit ONE small generated two-track fixture under
        tests/fixtures/meeting-two-track/ (macOS say -> afconvert to 16 kHz mono, a few
        seconds per track — the same recipe the repo already used for quick-brown-fox-16k.wav),
        so the wired-path assertions (segments > 0 on BOTH tracks) have something to run against
        with no network and no owner-gated corpus.

    WHY: the 2026-08 loops closed the notetaker phases with 135 green meeting tests while the
    pipeline had no caller (see the header of this file). A phase is closed by the behaviour, not
    by the mechanisms.

    DO (step 1 rewritten by the Panel round-2 verify 2026-09-26 — the gate EXECUTES, it never skips)
      1. scripts/notetaker-e2e.sh (set -euo pipefail): REQUIRES YAP_DATA_DIR (a throwaway root —
         refuses the default root, like --smoke), YAP_MODEL_DIR and YAP_EVAL_CORPUS; a missing or
         unset one is a non-zero exit with the sentence naming it and the command that provisions
         it (scripts/provision-notetaker-eval.sh, yap24-NT0). It links the pinned parakeet Q8_0
         file from YAP_MODEL_DIR into <YAP_DATA_DIR>/models/ (symlink or cp -c clone, never a
         write into YAP_MODEL_DIR), then runs the release binary with --transcribe-meeting on the
         committed two-track fixture tests/fixtures/meeting-two-track/ (generated per the Panel
         revision above: say -> afconvert, 16 kHz mono, a few seconds per track), and also on
         YAP_EVAL_CORPUS/two-track-ordering. It asserts: row state complete, segments > 0 on BOTH
         tracks, summary present when the summary model is installed or summary_status
         needs_model when it is not, Markdown export contains Summary / Action items /
         transcript. Any miss is a non-zero exit. There is NO skip path (the script never prints
         SKIP — the acceptance greps for it) and no exit-0-on-SKIP branch: under YAP_E2E_REQUIRE=1 (the loop always sets it) a missing input is a failure,
         and without it the script still fails — it only adds the provisioning hint.
      2. docs/MEETING-DEMO.md gains a "yap24 — what a meeting does now" section with the
         command and a pasted run.
      3. meeting_matrix.rs: every row whose call site now exists is Test, with its test named.

    NOT: no microphone, no TCC, no window. This is the headless half; the human half (one real
    meeting on Wilson's Mac) is listed in the PR body as the remaining manual check.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - THIS ITEM NOW OWNS THE FLAG: the proof runs the release binary with --transcribe-meeting, which is absent from cli.rs (grep finds 0), and
        its former owner (NT2 DO step 6) is skipped as already-done. Step 0 of the DO: add --transcribe-meeting <mic.wav> [--sys <sys.wav>]
        beside --transcribe-file in cli.rs, running the same pipeline worker against a scratch state root (YAP_DATA_DIR required, like
        --smoke) and printing the segment count and the final state. The preflight and acceptance both grep it.
      - The summary assertion follows #250's states: with the notes model linked from YAP_MODEL_DIR the Markdown export contains the notes
        sections; without it, whatever pending state #250 defines, never an error and never the retired needs_model string.
      - Depends on NT0, NT1, NT2 (residual), #250 (the preflight holds on the NT2 residual test file).
    Audit 2026-10-26: the audio box is offline-first. The CLI should run NEW yap-audit-eval-call-bleed's call-bleed-2voice fixture and print its metrics (echo_dup_rate, mid-sentence ends, median segment s, notes owner accuracy, Summary verbatim ratio). See docs/audits/meeting-notetaker-audit-2026-10-26.md section 9.
  `,
  acceptance: `
    grep -q 'transcribe-meeting' desktop/src-tauri/src/cli.rs
    test -x scripts/notetaker-e2e.sh
    bash -n scripts/notetaker-e2e.sh
    ! grep -q 'SKIP' scripts/notetaker-e2e.sh
    YAP_DATA_DIR="$(mktemp -d)/yap-state" YAP_MODEL_DIR="$HOME/code/wilson-voice-loop/cache/models" YAP_EVAL_CORPUS="$HOME/yap-eval-corpus/meetings" YAP_E2E_REQUIRE=1 bash scripts/notetaker-e2e.sh
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib meeting_matrix
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_pipeline_wired
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_pipeline_residual
  `,
})

ITEMS.push({
  id: 'yap24-NT10', prompt: 'yap24-NT', branch: 'loop/yap24-nt10-meeting-survives-sleep-and-lid-close', gated: 'panel',
  title: 'A meeting survives a lid close or sleep: IOKit holds the sleep, the journal finalizes, and the row ends partial with one plain sentence instead of staying recording forever',
  notes: 'NEW item added by Senior Panel synthesis 2026-09-26T17:35:00Z (User/wildcard seat, BLOCKING — grounded). The audit counted Y1-B (register NSWorkspaceWillSleepNotification / IORegisterForSystemPower) as already-done, but its own pre-flight only matches a COMMENT in power.rs, not a real observer call site (meeting_matrix.rs row 16 still reads PolicyOnly{absent_call_site:"NSWorkspaceWillSleepNotification"}; grep -c for that literal returns 4 on main today). only:[\'yap24-NT\'] never reaches Y1-B, so without this item a lid-close mid-meeting (walking between rooms, closing the laptop) leaves the row stuck in "recording" with no finalize, and yap24-NT1\'s reconcile only handles a crash, not a clean sleep. Corrects docs/ARCHITECTURE-AUDIT-2026-09-26.md §10\'s already-done count from 2 to 1 (Y4-D only).',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/tests/matrix_row16_sleep_wake.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 239 (sleep/wake observer) has not landed; land it, then build NT10"; exit 0; fi; grep -v '^[[:space:]]*//' desktop/src-tauri/src/power.rs | grep -q 'IORegisterForSystemPower' && test -f desktop/src-tauri/tests/meeting_survives_sleep.rs )
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26.
    DEPENDS: yap24-NT1 (reuses its capture-journal recovery path; this item is the sleep-triggered
    twin of NT1's crash-triggered reconcile)

    EVIDENCE
      - power.rs mentions NSWorkspaceWillSleepNotification only in a comment (no registration
        call site exists in src/); a grep for WillSleep across power.rs and permissions.rs
        matches comments only.
      - meeting_matrix.rs row 16: PolicyOnly { absent_call_site: "NSWorkspaceWillSleepNotification" }.
      - meeting::recover_orphaned_meetings (meeting.rs:1599) already recovers a crash-abandoned
        journal — this item registers the real observer and calls the SAME recovery path on a
        clean sleep, rather than duplicating it.

    DO
      1. Register NSWorkspaceWillSleepNotification (and the IORegisterForSystemPower fallback
         already scoped in Y1-B) in power.rs. On fire, if a meeting is recording: finalize the
         capture journal exactly as recover_orphaned_meetings does for a crash, and mark the row
         paused_by_sleep (a new, honest state distinct from partial — the recording was stopped
         cleanly by the OS, not lost).
      2. On wake (NSWorkspaceDidWakeNotification), a paused_by_sleep meeting is left for the user
         to resume (Start again, producing a second meeting) or stop-and-finalize; it is NEVER
         silently resumed into the old row.
      3. yap24-NT6 renders paused_by_sleep with a one-sentence explanation and the same Retry/
         Export affordances as a normal complete meeting once transcribed.
      4. Update meeting_matrix.rs row 16 to Test once the call site and its test exist.

    NOT: no change to dictation's own sleep handling (already shipped/separate). No new TCC
    permission — NSWorkspace notifications need none.

    Tests (tests/meeting_survives_sleep.rs): simulate WillSleep mid-recording -> row finalizes to
    paused_by_sleep with both WAV paths intact; WillSleep with no active meeting is a no-op.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - API CHOICE: NSWorkspaceWillSleepNotification does NOT delay sleep; IORegisterForSystemPower with kIOMessageSystemWillSleep and an
        IOAllowPowerChange acknowledgement does. The IOKit path is now PRIMARY (the old DO had it as a fallback). Acknowledge only after the
        journal is finalized. Per Apple's IOKit power-management docs; not re-fetched this run, so the builder reads the current docs first.
      - KEEP PR #239's NSWorkspace observer registration (and its SleepWakeHub); this item subscribes to the same hub and adds the IOKit hold. The
        preflight holds this item until #239's tests/matrix_row16_sleep_wake.rs exists. Dep: #239 landed.
      - NO NEW STATE: the paused_by_sleep state in DO 1, 2, 3 is VOID. Finalize the row to partial with the sentence "Stopped when the Mac went to
        sleep." and let the existing pipeline enqueue partial rows (NT2 R2 enqueues {transcribing, partial}). Wake never silently resumes the old row.
        The acceptance fails if the string paused_by_sleep appears anywhere.
      - Test (tests/meeting_survives_sleep.rs): inject kIOMessageSystemWillSleep mid-recording and assert the row ends partial, both WAV paths intact,
        the pipeline enqueued it, and the acknowledgement happened after finalize; with no active meeting it is a no-op that still acknowledges.
  `,
  acceptance: `
    grep -v '^[[:space:]]*//' desktop/src-tauri/src/power.rs | grep -q 'IORegisterForSystemPower'
    grep -q 'IOAllowPowerChange' desktop/src-tauri/src/power.rs
    ! grep -rn 'paused_by_sleep' desktop/src desktop/src-tauri/src
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_survives_sleep
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test matrix_row16_sleep_wake
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib meeting_matrix
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo clippy --all-targets --features custom-protocol
  `,
})

// yap24-NT11-0 REMOVED by Senior Panel 3 2026-10-26 (both seats said fold or kill). Its question ("is the bleed real, at a stable lag?") was answered by data on
// disk: text containment 0.71-0.89 on 3 calls, and acoustically a constant lag (155 of 157 confident windows on c6187806, 144 within 30 ms; GCC-PHAT lag 7.1 / 0.6 / -1.7 ms
// on the three real calls, no drift). Its PASS gate (envelope peak_ncc >= 0.5) was an unvalidated threshold on an unbuilt metric, its human half put the top CRITICAL
// behind a calendar dependency, and the mach-epoch persistence is not needed (the WAV sample index is the working shared clock, measured). Its one keep-line (finalize
// keeps <id>.tN.index.jsonl) now lives in yap24-NT11 DO 5. The original spec is in git history. Do not add it back to panelApproved.

ITEMS.push({
  id: 'yap24-NT11', prompt: 'yap24-NT', branch: 'loop/yap24-nt11-aec3-prepass-on-the-mic-track', gated: 'panel',
  title: 'A call on built-in speakers is transcribed once: an AEC3 pre-pass cancels the far side out of the mic track (derived file, raw WAV untouched), with a per-span Speaker label and a 3-word residue rule as the fallback',
  notes: 'REWRITTEN by Senior Panel 3 2026-10-26 (seat A built aec3 0.4.0 in scratch and ran it on the real stored WAVs; seat B the PM/User/AI seat). Earlier revisions (text-dedupe primary, bleed_score gate, persisted mach epochs, the output-route grep) are GONE, not superseded in place. yap24-NT11-0 is folded here and removed. The v1 blocker for the first-session walk: every remote line appears twice (Me and Them) until this lands. Build early.',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/tests/meeting_failed_audio_ceiling.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap24-NT1 residual (the purge test file meeting_failed_audio_ceiling.rs, whose purge must also remove the derived .aec.wav) has not landed; run the review pass, then build NT11"; exit 0; fi; test -f desktop/src-tauri/tests/meeting_cross_track_bleed.rs && grep -q 'fn aec_prepass' desktop/src-tauri/src/meeting_aec.rs && grep -q 'suppressed_as_echo' desktop/src-tauri/src/db.rs )
  `,
  spec: `
    Panel 3 2026-10-26: REWRITTEN, APPROVED. Class: STANDARD (not SEC). Why: it reads and writes audio files in the meetings directory only; it adds no permission, no network, no capture-path
    change and no IPC surface. The one supply-chain change (a new crate) is covered by the existing supply_chain.rs pin test, which this item extends. Re-classify as SEC only if the build touches
    permissions.rs, entitlements, or the capture code in syscapture.rs / record.rs.
    DEPENDS: yap24-NT1 (purge and delete own the derived file), yap24-NT2 (the pipeline whose track-0 windows read the cleaned WAV).

    WHY (audit issue #1, CRITICAL). On built-in speakers the far side is transcribed twice: 3 of 3 real virtual calls show 0.71-0.89 of the call track's word trigrams inside the mic track's
    same window; on c6187806 echo_dup_rate is 0.88 and the rendered both-track WER against a cloud reference is 0.838. The notes then give the other side's commitments to Me.

    DECISION (duel settled with evidence): the primary fix is an AEC3 pre-pass, NOT a text dedupe. Seat A MEASURED the pure-Rust aec3 crate 0.4.0 offline over the stored 16 kHz mono WAVs
    of c6187806 (mic = t0, system tap = t1), NS and AGC off: call-playing windows attenuated 25.5 dB at the median (p25 7.4 dB); local-only windows lose 0.17 dB (p90 0.20); the two stored
    WAVs are already aligned (lag 7.1 / 0.6 / -1.7 ms on the three real calls, constant across the file, no drift); 12.1 s for 31 minutes on an M4 Pro (about 20-25 s expected on M1 8 GB),
    streaming state tens of MB; it builds on arm64; MIT OR BSD-3-Clause; its dependencies (rustfft, num-complex, crossbeam) are already in desktop/Cargo.lock, so it adds ONE crate.
    A text dedupe alone cannot fix ASR of mixed double-talk and would delete real words; energy ducking kills double-talk; the webrtc-audio-processing crate is a second native C library and is REJECTED.
    VPIO (capture-time voice processing) stays rejected: it ducks the call's own playback, is irreversible, and replaces the shared cpal mic path; the offline system-track reference does the
    same job non-destructively.

    DO
      0. PRESERVE THE EVIDENCE FIRST. The audio of c6187806 is purged about 7 days after 2026-10-26 (complete meeting with segments). Before anything else copy its two WAVs (read-only cp,
         never move) into the private eval corpus ~/yap-eval-corpus/private/call-c6187806/ (outside the repo, never committed, never uploaded) and record only that path in the PR body.
      1. Cargo.toml: aec3 = "=0.4.0" (default features), exact pin like llama-cpp-2. NOTICE: add the aec3 entry (MIT OR BSD-3-Clause). tests/supply_chain.rs: add aec3 to the reachable-from-app
         expected set and assert the exact x.y.z pin with the existing helper. No other new crate may appear in Cargo.lock (the test diffs the lockfile package set against the committed one).
      2. New module src/meeting_aec.rs, everything behind two fns: fn aec_coupling(mic, sys) -> Option<lag_ms> (GCC-PHAT over the call-playing windows; Some only when a confident lag is found
         in at least 30% of the windows where the system track is playing) and fn aec_prepass(mic_wav, sys_wav, out_wav) -> Result (streaming, bounded memory, never a whole-meeting f32 buffer).
         The reference is shifted 20 ms earlier so a lag of -1.7 ms stays causal. It writes <audio>.t0.aec.wav ONCE (idempotent: a second run is a no-op; written to a temp name then renamed);
         the raw WAV is never modified and a re-transcribe re-derives it.
      3. MeetingAsr reads <audio>.t0.aec.wav for track 0 (instead of the raw mic) only when kind is virtual AND a t1 track exists AND aec_coupling is Some. Headphones (no coupling) and in-person
         meetings read the raw mic unchanged. The derived file is purged and deleted WITH the raw WAVs by yap24-NT1's purge and the meeting-delete cascade (extend both; test it); it is never kept alone.
      4. FALLBACK, after AEC, for what it cannot fix: (a) where the system track is all-zero or missing for a span (a tap hole, a reopen gap; on the real calls: 4456 after 690 s and 61d7, whose far
         side is mostly NOT in the tap, so no AEC or dedupe can fix it), label the mic speech "Speaker" (unattributed) through meetings::speaker_label, never "Me", and exclude it from Me-owned
         notes items; (b) the residue rule: a run of 3 or more CONSECUTIVE folded mic words that matches a system run within +-1.5 s is echo residue; an isolated single-word match is always kept;
         for segment spans, remove the matched run from the mic span and drop the span only if the residue has 0 or 1 non-stopword tokens, else keep the residue flagged mixed. Text agreement is a
         drop condition only where AEC ran or the span has no system audio; it never deletes on a single word. Each suppressed mic span is persisted as its own row flagged
         suppressed_as_echo=1 (excluded from render, FTS, export and notes input) via the NEXT free versioned migration step, never an ad hoc ALTER; the Transcript shows one line "N lines hidden as
         speaker echo - Show" revealing them inline (a one-line TS change in the transcript list). Record route/reopen boundaries against each track's finalized-sample position, never raw host_ns.
         Tests assert labels through speaker_label itself, never through hardcoded "Me"/"You" literals.
      5. Finalize keeps <id>.tN.index.jsonl (it no longer deletes the per-track anchors); cleanup only through NT1's purge and the delete cascade. (Carried from the removed NT11-0.)
      6. DEPENDENT (only when the Rewrite notes button from yap-audit-rewrite-notes is on main at build time; otherwise that item carries this step when it lands): when audio_kept=1 offer
         "Re-transcribe and rewrite", so an old echo transcript can be re-derived through the AEC pre-pass and its notes rewritten.
      7. Create scripts/eval/aec-real-call.sh (executable; described under ACCEPTANCE ON REAL AUDIO below). Its output is numbers only, never transcript text.
      8. NUMBERS to docs/BUDGETS.md under "AEC pre-pass (real call)" and "AEC pre-pass (synthetic)": attenuation p50 and p90, local-only loss p90, lag, wall time and peak RSS for the pre-pass.
    NOT: no capture-path change, no new TCC permission, no change to dictation, no second native C library, no VPIO, no mach-epoch persistence (the WAV sample index is the shared clock, measured), no
    merge of the two tracks' transcripts here (ordering by start time belongs to yap-audit-utterance-segments), no NT5 diarization (NEXT-RELEASE note: NT5 on the call track would give Them A / Them B).
    Known limit, stated honestly: when two people share the laptop mic, AEC correctly keeps both voices on the mic track; their owners are handled by yap-audit-notes-guards ("Someone here"), not here.

    ACCEPTANCE ON REAL AUDIO (numbers only, run from the audio on disk in a mktemp dir, never the data dir): scripts/eval/aec-real-call.sh copies the two WAVs of c6187806 from
    ~/yap-eval-corpus/private/call-c6187806/ into a mktemp dir, runs the AEC pre-pass, runs wilson-voice --transcribe-file <aec.wav> --timed windowed at 30 s (NT9's --transcribe-meeting when it lands),
    and prints ONLY numbers via scripts/eval/compare-reference.py --tracks against the cloud reference (YAP_AEC_REAL_REFERENCE). With YAP_EVAL_REQUIRE=1 a missing WAV or reference is a failure, not a skip.
    Gates:
      (a) echo_dup_rate <= 0.10 (today 0.88);
      (b) owner (reference Speaker B) share in the Me track >= 0.60 (was 0.64) and local Speaker D >= 0.70 (was 0.75);
      (c) remote Speakers A and C share in the Me track <= 0.10 (was 0.66 and 0.75);
      (d) user-visible (both tracks rendered) WER <= 0.25 (today 0.838);
      (e) acoustic guard with no ASR: local-only windows attenuation p90 <= 1.0 dB loss, and call-playing windows p50 >= 15 dB.
    Tier A (no model) also runs on the synthetic call-bleed-2voice fixture and on a headphones control: attenuation p50 >= 15 dB on call windows, nothing removed on the control, plus the unit
    cases of step 4 (isolated "yeah" kept; 7-word mic commitment inside a 25-word overlapping system sentence keeps its residue; tap-hole span labelled via speaker_label; route change with a mic
    reopen drops nothing within +-1 s of the seam without evidence; every word that exists only on the mic track survives).
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/meeting_cross_track_bleed.rs
    grep -q 'fn aec_prepass' desktop/src-tauri/src/meeting_aec.rs
    grep -q 'fn aec_coupling' desktop/src-tauri/src/meeting_aec.rs
    grep -q 'aec3 = "=0.4.0"' desktop/src-tauri/Cargo.toml
    grep -q 'aec3' NOTICE
    test -x scripts/eval/aec-real-call.sh
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test supply_chain
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_cross_track_bleed
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_failed_audio_ceiling
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_pipeline_wired
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib meeting
    YAP_DATA_DIR="$(mktemp -d)/yap-state" YAP_EVAL_REQUIRE=1 bash ../../scripts/eval/aec-real-call.sh
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo clippy --all-targets --features custom-protocol
  `,
})
