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
  title: 'Meeting audio is never deleted before it has been transcribed, and stranded "transcribing" rows are reconciled on launch',
  preflight: `
    test -f desktop/src-tauri/tests/meeting_retention_keeps_untranscribed_audio.rs
    grep -q "fn reconcile_stranded_meetings" desktop/src-tauri/src/meetings.rs desktop/src-tauri/src/lib.rs desktop/src-tauri/src/meeting_control.rs
  `,
  spec: `
    Panel: pending
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
      1. Retention becomes STATE-AWARE: audio is eligible for purge only when the meeting is
         complete AND it has at least one meeting_segments row AND the retention window has
         passed. Any other state keeps its audio indefinitely, and the Meetings view (yap24-NT6)
         says so. Both tracks age out together (the YV106 promise stays).
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
  `,
  acceptance: `
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_retention_keeps_untranscribed_audio
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_audio_retention
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib meeting
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-NT2', prompt: 'yap24-NT', branch: 'loop/yap24-nt2-wire-the-meeting-transcription-pipeline', gated: 'panel',
  title: 'The meeting transcript exists: stop hands the audio to the shipped MeetingAsr job, which writes segments and completes the row',
  preflight: `
    test -f desktop/src-tauri/tests/meeting_pipeline_wired.rs
    ! grep -n "allow(dead_code)" desktop/src-tauri/src/meeting_asr.rs
    grep -rln "MeetingAsr {" desktop/src-tauri/src | grep -v "meeting_asr.rs" | grep -q .
  `,
  spec: `
    Panel: pending
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
      - BLOCKING — segments must NOT land per-chunk. append_meeting_segments (db.rs:1652) is a
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
      6. A headless CLI entry beside --transcribe-file (cli.rs): --transcribe-meeting <mic.wav>
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
  `,
  acceptance: `
    ! grep -n "allow(dead_code)" desktop/src-tauri/src/meeting_asr.rs
    grep -rln "MeetingAsr {" desktop/src-tauri/src | grep -v "meeting_asr.rs" | grep -q .
    grep -q 'meeting_pipeline::spawn' desktop/src-tauri/src/lib.rs
    grep -q 'YAP_EVAL_REQUIRE' desktop/src-tauri/tests/meeting_eval.rs
    grep -q 'YAP_MODEL_DIR' desktop/src-tauri/tests/meeting_eval.rs
    cd desktop && npm ci && cd src-tauri
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
  title: 'Notes appear on their own: a completed transcript is summarized locally, action items are stored as rows, and "no model" is a state, not an error',
  preflight: `
    grep -q "CREATE TABLE IF NOT EXISTS meeting_actions" desktop/src-tauri/src/db.rs
    test -f desktop/src-tauri/tests/meeting_notes_auto_summary.rs
  `,
  spec: `
    Panel: pending
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
  `,
  acceptance: `
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_notes_auto_summary
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_delete_cascade
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_markdown_export
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib summarize
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-NT5', prompt: 'yap24-NT', branch: 'loop/yap24-nt5-diarization-joins-the-pipeline', gated: 'panel',
  title: 'Who said what: in-person meetings run the shipped diarization sidecar after transcription, with the honest accuracy framing',
  preflight: `
    grep -rln "diarize::pool()" desktop/src-tauri/src | grep -v "diarize.rs\|meeting_matrix.rs" | grep -q .
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
    test -f desktop/src/meetings/viewState.ts
    test -f desktop/src/meetings/viewState.test.ts
  `,
  spec: `
    Panel: pending
    DEPENDS: yap24-NT2, yap24-NT4 (states and fields it renders)

    EVIDENCE
      - views/Meetings.tsx:339-347: a transcribing meeting with no segments renders "Yap is still
        working through the audio." — forever, because nothing was working through it.
      - No progress, no failed/partial rendering with a way out, no Retry, no summary-model state,
        no action items (the summary panel at :332-337 is one paragraph).
      - notetaker_status (lib.rs, the 14.4 gate sentence) reaches only the Settings setup step
        (meeting_matrix.rs row 12b).

    DO
      1. A pure module desktop/src/meetings/viewState.ts: meetingViewState(meeting, segmentsCount)
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

    Tests: viewState.test.ts — one case per state, the progress arithmetic, and "no state renders
    an empty headline".
  `,
  acceptance: `
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
  `,
  spec: `
    Panel: pending
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
  `,
  acceptance: `
    cd desktop && npm ci
    npx tsc --noEmit
    npx vitest run src/pill
    npm test
    npm run build
  `,
})

ITEMS.push({
  id: 'yap24-NT8', prompt: 'yap24-NT', branch: 'loop/yap24-nt8-calendar-aware-record-prompt', gated: 'panel',
  title: 'Calendar-aware: an upcoming meeting or class raises a "Record?" prompt on the pill — opt-in, EventKit, no OAuth',
  preflight: `
    grep -q "NSCalendarsFullAccessUsageDescription" desktop/src-tauri/Info.plist
    test -f desktop/src-tauri/tests/calendar_prompt_policy.rs
  `,
  spec: `
    Panel: DEFERRED 2026-09-26T17:35:00Z — see docs/loop/DEFERRED.md. Excluded from pass-1 panelApproved (and
    from the yap24-NT chain's intent). It adds a NEW permanent TCC permission (Calendars), a
    5-minute EventKit background poll, and a pill interruption — none of it fixes stop ->
    transcript -> notes, which is the whole mandate of this pass ("the notetaker is not even
    working"). Its own spec already requires the panel AND Wilson to bless it, and it is the
    only pass-1 item whose acceptance can prove nothing beyond a synthetic policy table. 2 of 5
    panel seats named this the single item to kill from pass 1. Reconsider it after yap24-NT9
    has passed on one real meeting on Wilson's own Mac, and only after his explicit Calendars
    yes (ledger: pricing/product identity decisions are his; a new permanent TCC grant belongs
    in the same bucket).
    DEPENDS: yap24-NT6, yap24-NT7

    EVIDENCE
      - Wilson 2026-08-10 (memory project_yap_build_state): "calendar-AWARE (knows when
        meetings/CLASSES are coming — students are a target — and proactively prompts Record?)";
        stack decided then: EventKit via objc2-event-kit, reads iCloud/Google/Outlook through
        Calendar.app, zero OAuth.
      - Wispr ships a meeting reminder pill ("In {{n}} min", Join + Start, snooze) — parity
        teardown Notes/Wispr-Full-Parity-Research-2026-08-09.md line 100.
      - Apple: requestFullAccessToEvents is macOS 14.0+
        (https://developer.apple.com/documentation/eventkit/ekeventstore/requestfullaccesstoevents(completion:));
        purpose string NSCalendarsFullAccessUsageDescription, macOS 14.0+
        (https://developer.apple.com/documentation/bundleresources/information-property-list/nscalendarsfullaccessusagedescription).

    DO
      1. Settings -> Meetings: "Remind me to record meetings and classes" (default OFF). Turning
         it on requests calendar access; below macOS 14 the toggle is disabled with the reason.
      2. A pure policy (calendar_prompt.rs): events with attendees or a video link, or on a
         calendar the user ticked as "classes", starting within 2 min -> one prompt; snooze 2 min;
         never while a dictation or meeting is running; never twice for one event.
      3. The pill shows the prompt (Record / Snooze / Not this one); Record starts the meeting
         with the event title and kind call when the event has a video link, else in_person.
      4. Calendar reads are an EventKit query every 5 minutes at most and on
         EKEventStoreChanged — never a tight poll. Nothing leaves the Mac.

    Tests: tests/calendar_prompt_policy.rs over synthetic events (no EventKit in the test).
  `,
  acceptance: `
    grep -q "NSCalendarsFullAccessUsageDescription" desktop/src-tauri/Info.plist
    cd desktop && npm ci && npx tsc --noEmit && npm test && npm run build
    cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test calendar_prompt_policy
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-NT9', prompt: 'yap24-NT', branch: 'loop/yap24-nt9-notetaker-end-to-end-proof', gated: 'panel',
  title: 'Phase-closing proof: one headless command runs a two-track fixture through capture, transcript, notes and export',
  preflight: `
    test -x scripts/notetaker-e2e.sh
    grep -q "yap24" docs/MEETING-DEMO.md
  `,
  spec: `
    Panel: pending
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
  `,
  acceptance: `
    test -x scripts/notetaker-e2e.sh
    bash -n scripts/notetaker-e2e.sh
    ! grep -q 'SKIP' scripts/notetaker-e2e.sh
    YAP_DATA_DIR="$(mktemp -d)/yap-state" YAP_MODEL_DIR="$HOME/code/wilson-voice-loop/cache/models" YAP_EVAL_CORPUS="$HOME/yap-eval-corpus/meetings" YAP_E2E_REQUIRE=1 bash scripts/notetaker-e2e.sh
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib meeting_matrix
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_pipeline_wired
  `,
})

ITEMS.push({
  id: 'yap24-NT10', prompt: 'yap24-NT', branch: 'loop/yap24-nt10-meeting-survives-sleep-and-lid-close', gated: 'panel',
  title: 'A meeting survives a lid close or sleep: the journal finalizes on WillSleep and the row is marked paused_by_sleep, not left recording forever',
  notes: 'NEW item added by Senior Panel synthesis 2026-09-26T17:35:00Z (User/wildcard seat, BLOCKING — grounded). The audit counted Y1-B (register NSWorkspaceWillSleepNotification / IORegisterForSystemPower) as already-done, but its own pre-flight only matches a COMMENT in power.rs, not a real observer call site (meeting_matrix.rs row 16 still reads PolicyOnly{absent_call_site:"NSWorkspaceWillSleepNotification"}; grep -c for that literal returns 4 on main today). only:[\'yap24-NT\'] never reaches Y1-B, so without this item a lid-close mid-meeting (walking between rooms, closing the laptop) leaves the row stuck in "recording" with no finalize, and yap24-NT1\'s reconcile only handles a crash, not a clean sleep. Corrects docs/ARCHITECTURE-AUDIT-2026-09-26.md §10\'s already-done count from 2 to 1 (Y4-D only).',
  preflight: `
    grep -qE "NSWorkspaceWillSleepNotification|IORegisterForSystemPower" desktop/src-tauri/src/power.rs
    test -f desktop/src-tauri/tests/meeting_survives_sleep.rs
    ! grep -q 'absent_call_site: "NSWorkspaceWillSleepNotification"' desktop/src-tauri/src/meeting_matrix.rs
  `,
  spec: `
    Panel: pending
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
  `,
  acceptance: `
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_survives_sleep
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib meeting_matrix
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo clippy --all-targets --features custom-protocol
  `,
})

ITEMS.push({
  id: 'yap24-NT11', prompt: 'yap24-NT', branch: 'loop/yap24-nt11-speaker-bleed-mic-dedupe-with-route-detection', gated: 'panel',
  title: 'A call on built-in speakers is transcribed once: mic segments that re-hear the system track are dropped, and the dedupe is skipped on headphones',
  notes: 'NEW item added by the Senior Panel round-2 verify 2026-09-26 (THE USER seat, HIGH). NOT in pass 1 (gated panel, left out of the pass-1 panelApproved) — the destination is pass 2, see docs/loop/DEFERRED.md #12. It needs yap24-NT2 (the one-shot two-track merge) and yap24-NT3 (the output-device tracking) merged first. Panel round-3 audit 2026-09-26T23:42:00Z (synthesis of two independent seats — Senior macOS Audio/Rust Engineer and THE USER) revised the spec — see "Panel revisions" below. Still pass-2 only; put back into the pass-2 panelApproved array per Loop-Logs/YAP-RESUME-2026-09-26.md.',
  preflight: `
    test -f desktop/src-tauri/tests/meeting_cross_track_bleed.rs
    grep -q 'fn bleed_score' desktop/src-tauri/src/meeting_asr.rs
  `,
  spec: `
    Panel: audited 2026-09-26 — SOUND-WITH-CHANGES: approved for pass 2, revised (kill the
    output-route on/off switch, add an offline acoustic cross-correlation gate + real track-epoch
    persistence, redefine the dedupe unit as a residue rule, add per-span "Me" attribution) — see
    "Panel revisions" below. Two independent seats converged on the same BLOCKING finding and the
    same kill.
    DEPENDS: yap24-NT2 (the merge stage this dedupe lives in), yap24-NT3 (the whole-meeting output
    device tracking it reuses)

    ### PANEL REVISIONS 2026-09-26T23:42:00Z (Senior Panel synthesis, round 3 — applied,
    BLOCKING + 4x HIGH + 4x MEDIUM, GROUNDED; Senior macOS Audio/Rust Engineer seat and THE USER
    seat converged independently on the BLOCKING clock finding and on killOne):

      - BLOCKING — no shared clock zero exists between the two tracks, so the 500 ms overlap
        window and "same epoch as the track anchors" below have nothing to measure against: each
        track's host_ns is rebased to its OWN first callback (syscapture.rs's TapClock, and the
        mic's cpal base in record.rs), the absolute mach time is never persisted, and
        TrackEpochs::SHARED is, per meeting_asr.rs's own doc comment, "a lie the caller has to
        tell out loud" — no production path ever fills in a real offset (a source grep for
        TrackEpochs:: today finds only tests/two_track_merge_*.rs, nothing under src/). Add an
        NT11 step 0, before the dedupe: at every stream open/reopen, persist the absolute
        first-callback mach time (the tick TapClock already computes as its epoch, and the mic's
        cpal base next to record.rs's mach_absolute_time call) into the per-meeting anchor
        sidecar NT2 already keeps. Build TrackEpochs from those persisted values — never from
        SHARED — inside NT11's own code path (this item, not NT2's). A source-scan test asserts
        TrackEpochs::SHARED has no caller under src/. As a fallback when an epoch is missing
        (e.g. an anchor file written before this landed), estimate it once per meeting from the
        bleed_score correlation peak below, never from a guessed constant.

      - HIGH — the dedupe unit is undefined ("segment" spans word-level or higher depending on
        the ASR model), so text containment on WORD spans reduces to "drop any mic word that also
        appears on the system track nearby" — a real "yeah", "right", or Wilson reading back
        "Tuesday at three" gets deleted. On SEGMENT spans the >= 0.6 ratio cuts both ways: a
        7-word real commitment sitting inside someone else's overlapping 25-word sentence is
        0.78-contained and gets dropped whole (violating this item's own "no real mic word is
        lost" test), while below the ratio the other side's full 25 words survive labelled "Me".
        Replace the ratio rule: for TimedKind::Word, dedupe only a run of 3 or more CONSECUTIVE
        folded mic words matched to a system run inside the measured lag window — an isolated
        single-word match is always kept. For TimedKind::Segment (or higher), text containment
        becomes a residue rule, not a drop/keep binary: remove the matched system run's text from
        the mic span; if the residue has 0 or 1 non-stopword tokens, drop the span; otherwise KEEP
        the residue (word-level spans) or keep the whole span flagged mixed=true (segment-level)
        so NT4 never draws a "Me" action item from it. Text agreement is a NECESSARY condition for
        a drop, never SUFFICIENT on its own — see the acoustic gate below.

      - HIGH — the system WAV is the decisive evidence and this item as first written never uses
        it. Add a pure fn bleed_score(mic: &[f32], sys: &[f32], lag_range) -> (peak_ncc, lag_ms)
        in meeting_asr.rs (16 kHz, band-limited envelopes, windowed FFT cross-correlation — O(n),
        well under 1% of ASR time for a 3-hour meeting) run offline over the two already-finalized
        WAVs (WavWindows::open, never MemoryWindows — 690 MB for a 3-hour meeting). A candidate mic
        run/segment is dropped only when peak_ncc clears a committed threshold AND the text rule
        above agrees. This also satisfies the item's own "measured, not guessed": log the
        per-meeting median measured lag and commit it, next to each device's queried
        kAudioDevicePropertyLatency + safety offset, to docs/BUDGETS.md — "a few hundred ms ...
        through the room" conflated acoustic travel time (~3 ms/m) with this unmeasured clock
        offset; both numbers now get recorded honestly. Still never touches the capture path or
        the WAV files — a re-transcribe still re-derives everything.

      - HIGH — kill the output-route classifier as the dedupe on/off switch (killOne, both seats):
        classifying from the system DEFAULT output device is the wrong device for a call app that
        outputs elsewhere, misses Bluetooth/HDMI/AirPlay speakers under "unknown is treated as
        speakers", and misreads Wilson's own machine today (system_profiler SPAudioDataType shows
        "BH + Headphones: Transport: Unknown" and "BlackHole 2ch: Transport: Virtual"). Gate the
        dedupe instead on MEASURED COUPLING: it runs only where the bleed_score + text evidence
        above actually finds matched runs at a stable lag. On AirPods/headphones no runs match, so
        nothing is dropped, with no device taxonomy required. fn output_route(...) stays in
        syscapture.rs as a DIAGNOSTIC field only, logged next to the suppression count (see
        below) — it never again decides on/off. Do not use the grep for fn output_route as a
        stand-in for "the feature works" (dropped from preflight/acceptance above); the real gate
        is bleed_score plus the matched-run test below.

      - HIGH — per-span attribution: a route change or a tap rebuild leaves a stretch where the
        far side's speech exists ONLY on the mic (dead air across a reopen, or AirPods
        disconnecting back to speakers mid-call) — MicIsMe is decided once per MEETING today
        (diarization_target from whether any system spans exist at all), so that whole stretch is
        labelled "Me" and NT4 attributes their sentence to Wilson. Make the label per SPAN, not
        per meeting: where the route is speakers/unknown and the system track delivered nothing
        for that span (tap hole, reopen gap, or all-zero samples), label the mic speech "Speaker"
        (unattributed) through the existing meetings::speaker_label path, not "Me", and exclude it
        from "Me"-owned action items in NT4. Tests assert labels through speaker_label itself,
        never through hardcoded "You"/"Me"/"Them" string literals in a fixture (the DO text below
        says "You"; the shipped label is "Me").

      - MEDIUM — a route change mid-meeting is exactly when cross-track host time is least
        trustworthy: a reopened stream rebases host_ns to zero, and the segmented timeline cannot
        recover the dead air across the reopen. Record route and reopen boundaries against each
        track's finalized-sample position (the axis meeting.rs::finalized_positions already keeps
        continuous across a reopen), never against raw host_ns. The "speakers -> AirPods" fixture
        below must include a mic reopen at the boundary; assert nothing within +/-1 s of the seam
        is dropped unless the acoustic gate actually fires.

      - MEDIUM — VPIO / capture-time voice-processing echo cancellation is REJECTED as the layer
        for this fix, stated explicitly rather than silently bypassed: it requires both the input
        and output node in voice-processing mode, ducks the call's OWN playback on macOS 14+
        (voiceProcessingOtherAudioDuckingConfiguration), is irreversible (a re-transcribe could
        never recover the raw mic), and would replace the shared cpal mic path dictation also
        uses. The offline system-track reference (bleed_score above) does the same job
        non-destructively, entirely outside the capture path, and re-derives on every
        re-transcribe. Add this sentence to the NOT clause verbatim: "VPIO rejected: it ducks the
        call's own playback, is irreversible, and replaces the shared cpal mic path; the offline
        system-track cross-correlation does the same job non-destructively."

      - MEDIUM — the tests as first specified are text-only ("synthetic ChunkOutcome spans, no
        model"), and meeting_asr.rs's own header warns RNNT/TDT emission times are unstable
        across decodes and must never be used to match one decode's words against another's — yet
        this item matches mic-decode words to system-decode words by time. Add a real paired
        fixture to the NT0 corpus (5 minutes of a call on built-in speakers: mic.wav + sys.wav +
        anchors, with a committed room impulse response at -18 dB / 40 ms delay and a declared
        300 ms epoch offset) and gate on it with the real model via --transcribe-meeting headless:
        remaining duplicated far-side runs <= 5%, mic-only word recall >= 0.98, nothing dropped on
        the headphones fixture, and the "speakers -> AirPods" reopen fixture above all hold. Keep
        the synthetic-span unit tests as Tier A (no model); the paired-WAV run is Tier B, gated
        YAP_EVAL_REQUIRE=1 + YAP_MODEL_DIR like NT0/NT2. Commit both numbers to docs/BUDGETS.md.

      - MEDIUM — suppressed spans must be auditable, not a silent delete-and-count: persist each
        suppressed mic span as its own row flagged suppressed_as_echo=1 (excluded from render,
        FTS, export and NT4 input) instead of the "one diagnostics field" this item first
        specified, so a founder can see and recover a line he remembers saying without a
        re-transcribe. Add this via the NEXT AVAILABLE versioned migration step (never an ad hoc
        ALTER — NT4's revision already claims MIGRATION_6_MEETING_NOTES; this one takes the
        following number when both land). Route spans, now that they are diagnostic-only, live in
        the same per-meeting anchor/journal sidecar NT2 already keeps, cleaned up only through
        NT1's purge/delete path — never independently. yap24-NT6 (its own item, not built here)
        gets a one-line note: "N lines hidden as speaker echo · Show" revealing them inline — that
        UI line is DEFERRED to NT6's own pass.

    EVIDENCE (both seats, converged): syscapture.rs's TapClock rebases each stream's host_ns to
    its own first mHostTime and never persists the absolute tick; meeting_asr.rs's doc comment
    calls TrackEpochs::SHARED "a lie the caller has to tell out loud"; asr_engine.rs's TimedKind
    is None/Segment/Word/Token and meeting_asr.rs's own fixtures already exercise TimedKind::Word;
    meeting.rs::finalized_positions stays continuous across a reopen where host_ns does not;
    system_profiler SPAudioDataType on Wilson's own machine shows "BH + Headphones: Transport:
    Unknown" and "BlackHole 2ch: Transport: Virtual" — both misclassified by a device-taxonomy
    route switch.

    WHY (THE USER seat): a founder's real call is a laptop on a desk with the far side playing out
    of the built-in speakers. The system track (process tap) carries the far side cleanly; the mic
    track ALSO hears it, a few hundred ms later, through the room. Once NT2 transcribes both tracks
    and merges them by host time, every remote sentence appears twice — once as Them (system) and
    once as You (mic) — and NT4's summary then attributes the other side's commitments to Wilson.
    On headphones/AirPods there is no bleed and nothing must be removed.

    DO (original — SUPERSEDED where it conflicts with "Panel revisions" above; kept for context,
    read the revisions first)
      1. SUPERSEDED — output route detection is no longer the dedupe on/off switch (see the HIGH
         kill finding above). Still add fn output_route(...) in syscapture.rs, beside NT3's
         per-meeting device tracking, classifying the default output as speakers | headphones |
         unknown on the same triggers as before, but it now feeds a DIAGNOSTIC log field only —
         never a gate. The real gate is step 2 below.
      2. SUPERSEDED — the dedupe still runs INSIDE NT2's one-shot merge stage (after both tracks'
         ASR finished, before the single transactional write), but the drop decision is now: (a)
         epochs come from the persisted per-track anchors (BLOCKING finding above), never SHARED;
         (b) a candidate window is a text-matched run (3+ consecutive words, or the containment
         residue rule for segment spans — HIGH finding above), gated on peak_ncc from
         fn bleed_score clearing its committed threshold (HIGH finding above) — text agreement
         alone is never sufficient; (c) never trim words out of a kept span/residue.
      3. Audio is never touched — the dedupe and bleed_score both act read-only on the two
         finalized WAVs/segments, so a re-transcribe re-derives everything. SUPERSEDED: suppressed
         mic spans are now their own rows flagged suppressed_as_echo=1 (MEDIUM finding above), not
         a single diagnostics counter; still log how many and on which route per meeting.
      4. Headphones spans skip the dedupe entirely (no matched runs exist there in practice, so
         this now falls out of the measured-coupling gate rather than being special-cased); kind
         in_person meetings (MicOnly per NT3) never run it. NEW: where a speakers/unknown span has
         no system audio to compare against at all (tap hole / reopen gap), label that mic speech
         "Speaker" via meetings::speaker_label, not "Me" (HIGH per-span-attribution finding above).

    NOT: no echo cancellation / DSP on the capture path (VPIO explicitly rejected — see the added
    NOT sentence in the Panel revisions above), no new TCC permission, no change to dictation, no
    second ASR pass. Route classification is a diagnostic field only, never the on/off switch.

    Tests (tests/meeting_cross_track_bleed.rs) — Tier A, synthetic ChunkOutcome spans, no model:
      - speakers route, mic re-hears a system sentence 250 ms late, with a real persisted epoch
        (not SHARED) -> the mic copy is suppressed via a matched run + bleed_score, the system
        copy stays, the merged transcript has the sentence once;
      - the same audio spans on a headphones route (no matched run found) -> nothing suppressed;
      - double-talk: a 7-word mic commitment inside an overlapping 25-word system sentence ->
        the residue (the mic's own 7 words) survives, in both directions of overlap ratio;
      - an isolated single mic word ("yeah") that also appears in a nearby system utterance ->
        kept (never dropped on a single-word match);
      - mic speech with no overlapping system segment -> kept;
      - mic speech on a speakers/unknown route where the system track is silent for that span
        (tap hole) -> labelled "Speaker" via meetings::speaker_label, not "Me";
      - a route change mid-meeting (speakers -> AirPods) with a mic reopen at the boundary ->
        dedupe applies only where bleed_score/text evidence actually matches, and nothing within
        +/-1s of the seam is dropped without that evidence;
      - source scan: no caller of TrackEpochs::SHARED under src/;
      - no real mic word is lost: across the fixture, every word that exists only on the mic track
        survives (mirror meeting_eval's seam_dedupe_never_deletes_real_words posture).

    Tests — Tier B, real model, gated YAP_EVAL_REQUIRE=1 + YAP_MODEL_DIR (MEDIUM finding above):
      - a paired mic.wav/sys.wav fixture (room impulse response, -18 dB, 40 ms delay, declared
        300 ms epoch offset) run through --transcribe-meeting headless: duplicated far-side runs
        <= 5%, mic-only word recall >= 0.98, nothing dropped on the paired headphones fixture.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/meeting_cross_track_bleed.rs
    grep -q 'fn output_route' desktop/src-tauri/src/syscapture.rs
    grep -q 'fn bleed_score' desktop/src-tauri/src/meeting_asr.rs
    ! grep -rn 'TrackEpochs::SHARED' desktop/src-tauri/src
    cd desktop && npm ci && cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_cross_track_bleed
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_pipeline_wired
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib meeting
    YAP_DATA_DIR="$(mktemp -d)/yap-state" YAP_EVAL_REQUIRE=1 YAP_EVAL_CORPUS="$HOME/yap-eval-corpus/meetings" YAP_MODEL_DIR="$HOME/code/wilson-voice-loop/cache/models" cargo test --features custom-protocol --test meeting_cross_track_bleed -- --ignored --nocapture --test-threads=1
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo clippy --all-targets --features custom-protocol
  `,
})
