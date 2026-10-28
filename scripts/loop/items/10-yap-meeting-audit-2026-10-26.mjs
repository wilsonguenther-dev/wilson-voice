// yap-audit — MEETING NOTETAKER AUDIT 2026-10-26 (stamp 2026-10-26T16:44:50Z).
// Source: docs/audits/meeting-notetaker-audit-2026-10-26.md (read it first; every item cites its issue #).
// Evidence came from Wilson's installed 0.9.3-dev (main a740614 + PR 250 head 0a87249) and a read-only copy
// of his meetings DB: 12 meetings, 4 over 10 min, plus a cloud-notetaker reference transcript of the 31-min one.
// Findings that existing items already own were appended to those items as "Audit 2026-10-26:" lines
// (NT1, NT2, NT3, NT4, NT5, NT6, NT9, NT11-0, NT11, yap-notetaker-notes, yap-lic-00-kill-forge-fetch).
// SENIOR PANEL 3 (2026-10-26T17:00Z, two seats + synthesis): approved utterance-segments, notes-strategy, rewrite-notes; extended notes-guards
// (F2 padded key points, F3 echo owner, F4 no-summary copy, F5 prefix clamp, mic-track owner, repetition penalty); remote-speaker-names DEFERRED
// (next release; calendar 1:1 name lives in yap24-NT4 DO R5); yap24-NT11 rescoped to an AEC3 pre-pass and yap24-NT11-0 folded into it and removed.
// This file holds ONLY the fixes no item owned. PRIVACY: no fixture, test or log may contain text from Wilson's
// real meetings; fixtures are synthetic (macOS say) and planted.
// Every cargo line carries YAP_DATA_DIR="$(mktemp -d)/yap-state" INLINE (never the real data dir).

ITEMS.push({
  id: 'yap-audit-vad-boundaries', prompt: 'yap-audit', branch: 'loop/yap-audit-vad-boundaries-wire-silence-cuts', gated: null, builder: 'sonnet',
  title: 'Meeting chunks cut at pauses, not on a 30 s clock: the shipped pipeline passes the warm Silero VAD to MeetingAsr (it is hardwired to vad: None)',
  preflight: `
    ! grep -n 'vad: None' desktop/src-tauri/src/meeting_pipeline.rs
    test -f desktop/src-tauri/tests/meeting_pipeline_vad_boundaries.rs
  `,
  spec: `
    Audit issue #3 (HIGH). meeting_pipeline.rs:522 (main a740614) builds MeetingAsr with vad: None, so plan_windows
    (meeting_asr.rs:379-440) always takes plan_windows_fixed. On Wilson's install all 414 chunks in the
    *.asr-progress.json files are fixed_clock/edge, zero silence boundaries, and 15-47% of segments end mid-sentence.
    DO
      1. Hand the app's warm Silero session (crate::vad::WarmVad, already implements VoiceActivity at
         meeting_asr.rs:578) to MeetingAsr in the production pipeline. A VAD load failure keeps today's behaviour
         (fixed clock) and logs one warn line; it never fails a meeting.
      2. Keep resume semantics: a resumed run re-plans from ResumePoint exactly as today.
      3. tests/meeting_pipeline_vad_boundaries.rs: drive the PRODUCTION pipeline entry (the function the worker
         calls, not plan_windows directly) over a synthetic WAV with pauses placed between 20 s and 40 s; assert at
         least one boundary kind is the silence kind and every silence cut lands inside a pause; a second case with
         a VAD stub that errors asserts fixed_clock fallback and a complete meeting.
    NOT: no change to the 30 s target / 2 s overlap constants; no UI change.
  `,
  acceptance: `
    ! grep -n 'vad: None' desktop/src-tauri/src/meeting_pipeline.rs
    (cd desktop/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_pipeline_vad_boundaries)
  `,
})

ITEMS.push({
  id: 'yap-audit-utterance-segments', prompt: 'yap-audit', branch: 'loop/yap-audit-utterance-segments-from-token-spans', gated: 'panel',
  title: 'A stored meeting segment is an utterance (token-span pauses merged into 3-12 s same-track turns), not a whole 30 s window, so Me/Them turns can be read',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/summary_jobs.rs && test -f desktop/src-tauri/src/meeting_aec.rs ) || ! grep -q 'call-bleed-2voice' desktop/src-tauri/tests/fixtures/meeting_eval_manifest.json; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 250 (summary_jobs.rs, which this touches via summarize.rs), yap24-NT11 (meeting_aec.rs, the AEC pre-pass this is measured on) or yap-audit-eval-call-bleed (the fixture) has not landed; land them, then build this"; exit 0; fi; test -f desktop/src-tauri/tests/meeting_utterance_segments.rs && grep -q 'fn split_utterances' desktop/src-tauri/src/meeting_asr.rs )
  `,
  spec: `
    Panel 3 2026-10-26: APPROVED-WITH-CHANGES (two seats, SOUND-WITH-CHANGES each). Unit and split rule are decided below.
    Audit issue #4 (HIGH). Average stored segment length is 29.4-30.1 s on every real meeting: a two-person call
    renders as alternating 30 s paragraphs, each with one label. Token spans already exist in 100% of non-empty chunks
    (timestampKind token; c6187806 track 0 had 62 of 62 chunks with spans) and are thrown away at insert.
    WHY THE RULE IS A MERGE: the raw gap rule alone (cut at a span gap >= 0.6 s, or a sentence end followed by a gap >= 0.25 s)
    produced 440 pieces with median 1.8 s on c6187806: too fine to read and too many MAP passes for the 1.5B notes budget.
    DO
      1. meeting_asr.rs: pub fn split_utterances(chunk spans) -> Vec<(start, end, text)>. Step one, raw cut: gap >= 0.6 s, or a sentence end
         followed by a gap >= 0.25 s. Step two, merge: consecutive pieces of the SAME track merge while the gap between them is < 1.2 s and the
         merged piece is <= 20 s; a piece under 3 s merges into its neighbour on the same track (when the result stays <= 20 s) and is kept
         alone only when it is the whole chunk. Chunks with timestampKind none stay whole. Never split inside a word span.
      2. The pipeline inserts the pieces as segments (same track, absolute times); the seam merge still runs first. Existing meetings keep
         their 30 s rows: NO migration, no re-split on open; only a re-transcribe (or the Re-transcribe and rewrite action) produces utterances.
      3. Track 0 reads the AEC-cleaned window from yap24-NT11 when that exists for the meeting, so the pieces are cut on the cleaned decode.
         This item does NOT depend on yap-audit-vad-boundaries.
      4. summarize.rs seg labels keep working (positional). Re-measure MAP chunk planning: every MAP chunk, INCLUDING its seg labels, is <= 1400
         tokens on a 31-minute two-track fixture, and the MAP pass count is <= 1.25x the whole-window-row count for the same meeting (a test
         computes both from the same fixture).
      5. tests/meeting_utterance_segments.rs: synthetic spans -> expected pieces (including the three thresholds above at their edges); at least one
         case reads token spans from a corpus asr-progress JSON of the NT0 synthetic corpus (timings only; never text from a real meeting); a
         two-track synthetic meeting yields interleaved turns ordered by start; meeting_utterance_segments asserts median_segment_seconds is
         between 3 and 12 on call-bleed-2voice after the AEC pre-pass.
    NOT: no change to the 30 s target / 2 s overlap constants; no UI change; no migration; no re-splitting of stored rows.
    Coordinate with yap-notetaker-notes (track turns UI).
  `,
  acceptance: `
    grep -q 'fn split_utterances' desktop/src-tauri/src/meeting_asr.rs
    (cd desktop/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_utterance_segments)
    (cd desktop/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test summary_default_notes)
  `,
})

ITEMS.push({
  id: 'yap-audit-notes-guards', prompt: 'yap-audit', branch: 'loop/yap-audit-notes-guards-no-retranscript', gated: null,
  title: 'Notes never ship a re-transcript or a wrong owner: refuse a verbatim Summary, strip Me:/Them: prefixes, one section per item, at most two items per cited segment, no padded Key points, no echoed or mic-track owner claims, a repetition penalty on notes decoding, a Summary input that covers the whole meeting, and a title that is not a transcript fragment',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/summary_jobs.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 250 (default meeting notes, summary_jobs.rs) has not landed; land it, then build this"; exit 0; fi; test -f desktop/src-tauri/tests/summary_audit_guards.rs && grep -q 'VERBATIM_NARRATIVE_MAX' desktop/src-tauri/src/summarize.rs && grep -q 'No summary this time: the notes model could only repeat the transcript.' desktop/src-tauri/src/summarize.rs && grep -q 'fn word_trigram_containment' desktop/src-tauri/src/meeting_echo.rs )
  `,
  spec: `
    Panel 3 2026-10-26: APPROVED (ungated; extended with F2, F3, F4, F5, the owner rule and the repetition penalty). PR 250 merged as-is ships grade-1 notes:
    guards 1-6 plus F2 and F3 must land before PR 250 reaches a release (posted on PR 250).
    Audit issue #2 (CRITICAL, mechanical half). On the three meetings PR 250 summarized on Wilson's Mac the Summary
    paragraph is 0.91-0.97 verbatim (4-gram containment in the transcript), one starts with a leaked "Me:" prefix,
    one segment produced 10 near-identical actions, the same line appears in up to 3 sections (8 duplicates on the
    31-min meeting), first-person narration ("It's really dope.") is filed as an action, and a failed headline falls
    back to the first 8 transcript words as the title (fallback_title, summarize.rs).
    Three of the failures are NOT the model: backfill_key_points fills Key points with raw transcript sentences by design; headline_source
    reads the MAP narratives clamped to a 750-word PREFIX so the end of a long meeting never reaches the Summary (the 31-min meeting's
    3 real commitments were at 27:30-29:10); and notes decoding is greedy with no repetition penalty (the runaway and the 10 actions from one segment).
    DO (summarize.rs and the polish sidecar, deterministic, no model change)
      1. VERBATIM_NARRATIVE_MAX = 0.6: a narrative whose word-4-gram containment in the source exceeds it is rejected
         (reason v8_verbatim). If no narrative survives, render NO Summary paragraph (never the MAP fallback text) and the Summary section reads
         exactly: "No summary this time: the notes model could only repeat the transcript."
      2. Strip a leading speaker tag (Me:, Them:, Speaker, Speaker N:) from narrative, key points and items.
      3. One item lives in exactly one section: actions over decisions over open questions (same_item match).
      4. At most 2 items may cite the same seg label; keep the two with the strongest commitment score.
      5. An action needs a commitment (owner_commitment / imperative addressed to a person / "I'll" + verb);
         first-person feelings and descriptions ("I'm excited", "It's really dope") are dropped (reason v9_not_action).
         A "decision" needs a decision marker; a statement without "?" is not an open question.
      6. fallback_title: never raw transcript words; use "Meeting, <weekday> <HH:MM>" from started_at when the
         headline title fails.
      7. tests/summary_audit_guards.rs: synthetic transcripts reproducing each failure above (no real meeting
         text); assert every guard, and that a clean planted fixture still yields its planted actions.
      8. (F2) backfill_key_points never pads with transcript sentences: fewer than MIN_KEY_POINTS validated points render as they are; zero
         validated points renders NO Key points section.
      9. (F3) Echo-owner rule: an item citing a Me-track segment whose word trigrams are >= 30% contained in a Them-track segment within +-1.5 s is
         attributed to Them; if neither track is clear the owner is left blank. Use the SAME pure fn as echo_dup_rate:
         meeting_echo::word_trigram_containment (src/meeting_echo.rs, created by yap-audit-eval-call-bleed; if it is not on main when this builds,
         create it there with that name and let yap-audit-eval-call-bleed import it). The AEC pre-pass (yap24-NT11) will reduce how often this
         fires; the guard stays, because AEC cannot fix tap holes or a far side that is mostly absent from the tap.
     10. Owner on the mic track: an action whose cited segment is on the mic track is owned "Me" only when the meeting is known to have a single
         voice on that mic; otherwise the owner reads "Someone here". v1 has no voice-count source (diarization, NT5, is deferred until after
         100 users), so the pure fn mic_is_single_voice(meeting) returns false until NT5 supplies it: every mic-track action reads
         "Someone here" unless the item text itself names a person. This is a deliberate v1 loss, taken over a wrong "Me": on c6187806 the
         3 real commitments came from a second local voice that AEC correctly keeps, and they were filed under Me. A Them-track action reads "Them".
     11. Repetition penalty on NOTES decoding only (yap-polish main.rs sampler plan, near line 548): add a llama penalties sampler (repeat 1.1 over the
         last 64 tokens) to the chain AHEAD of greedy for notes requests. Dictation polish stays byte-for-byte greedy (its golden fixtures must
         not move); a test asserts the dictation plan is unchanged.
     12. (F5) headline_source: the Summary input covers EVERY MAP chunk (never a prefix clamp of 750 words). When over budget, shrink each
         chunk's contribution evenly, never truncate from the end. A test plants a commitment in the LAST chunk of a long synthetic transcript and
         finds it in the Summary input.
    NOT: no change to GROUNDEDNESS_FLOOR or the model (that is yap-audit-notes-strategy, gated). No diarization.
  `,
  acceptance: `
    grep -q 'VERBATIM_NARRATIVE_MAX' desktop/src-tauri/src/summarize.rs
    grep -q 'v8_verbatim' desktop/src-tauri/src/summarize.rs
    grep -q 'No summary this time: the notes model could only repeat the transcript.' desktop/src-tauri/src/summarize.rs
    grep -q 'Someone here' desktop/src-tauri/src/summarize.rs
    grep -q 'fn word_trigram_containment' desktop/src-tauri/src/meeting_echo.rs
    grep -q 'fn mic_is_single_voice' desktop/src-tauri/src/summarize.rs
    (cd desktop/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test summary_audit_guards)
    (cd desktop/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test summary_default_notes)
    (cd desktop && cargo test -p yap-polish)
  `,
})

ITEMS.push({
  id: 'yap-audit-notes-strategy', prompt: 'yap-audit', branch: 'loop/yap-audit-notes-strategy-measured-decision', gated: 'panel',
  title: 'DECISION and SHIPPED, measured: Yap writes notes items-first and composes the Summary last from the surviving items; the arms are measured on the NT4 eval over AEC-cleaned input',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/summary_jobs.rs && test -f desktop/src-tauri/tests/meeting_notes_recall_eval.rs && test -f desktop/src-tauri/src/meeting_aec.rs ) || ! grep -q 'call-bleed-2voice' desktop/src-tauri/tests/fixtures/meeting_eval_manifest.json || ! grep -q 'VERBATIM_NARRATIVE_MAX' desktop/src-tauri/src/summarize.rs; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 250 (summary_jobs.rs), yap24-NT4 (the recall eval test), yap24-NT11 (AEC-cleaned input), yap-audit-eval-call-bleed (fixture) or yap-audit-notes-guards (VERBATIM_NARRATIVE_MAX) has not landed; land them, then build this"; exit 0; fi; test -f docs/decisions/notes-strategy-2026-10.md && grep -q '^DECISION:' docs/decisions/notes-strategy-2026-10.md && grep -q 'summary recall' docs/BUDGETS.md )
  `,
  spec: `
    Panel 3 2026-10-26: APPROVED-WITH-CHANGES. The strategy is chosen as advisory below; the numbers in this item confirm or overturn it, and the chosen arm SHIPS here.
    Audit issue #2 (CRITICAL, strategy half). summarize.rs GROUNDEDNESS_FLOOR = 0.80 over content words (common_gates, polish.rs content_words: raw >=4-char
    tokens, no stemming) rejects paraphrase ("discussed", "agreed", "planning" never appear in the transcript; 38 / 15 / 22 v3_ungrounded rejections per
    real meeting) and keeps verbatim copies. A 1.5B model classifies short spans reliably under a grammar; it cannot reliably write abstractive prose
    over 1,400 noisy tokens. The mechanical non-model failures are fixed first in yap-audit-notes-guards (padded Key points, prefix clamp, no repetition
    penalty), so this item measures the model honestly.
    ARMS (each runs on the AEC-cleaned fixture input, never on echo-polluted text, which would measure echo and not strategy):
      A. today (1.5B + 0.80 floor) + the guards: the baseline.
      B. 1.5B with a paraphrase-tolerant grounding check alone (digits, names and contacts stay hard-gated; lemma-level match for the rest). Necessary, not sufficient.
      E. ITEMS FIRST, SUMMARY LAST (recommended): keep the MAP grammar items (actions, decisions, questions, each tied to a segment id); drop
         narrative from MAP and drop REDUCE; add a 3-8 word topic label per MAP chunk (timestamped); write the Summary in ONE pass whose input is only the
         surviving items plus the topic labels (about 300-600 tokens, covering the whole meeting); replace the 0.80 floor with hard digit/name/contact
         gates, item grounding against the CITED segment +-1, and a Summary 4-gram verbatim ceiling <= 0.5.
      C. Qwen2.5-3B q4, MEASURED ONLY (about a 2 GB download and about 2x wall time, UNVERIFIED): reports peak RSS with the ASR engine unloaded; must stay <= 2.2 GB.
      The BYOM arm is REMOVED: the yap-byom-* items own BYOM and it is not a notes strategy.
    Each arm reports: action recall and precision, decision recall, owner accuracy (or blank), Summary 4-gram verbatim ratio, cross-section duplicates,
    items per segment, wall time and peak RSS, on call-bleed-2voice and the planted 15-minute fixture of yap24-NT4 (R2). The eval gates are the table in NT4:
    recall >= 0.8, precision >= 0.8, decision recall >= 0.5, verbatim ratio <= 0.5, cross-section dups 0, items per segment <= 2, owner accuracy >= 0.8 or blank.
    HONEST FALLBACK COPY (ships with arm E): no Summary passes -> no paragraph, and the section shows "Topics" (the timestamped labels); items fail the
    gates -> a section "Lines that sounded like commitments" (quoted, with time and track), never filed under "Action items"; every notes card ends with
    the footer "Written on this Mac by a small model. Check owners against the transcript."
    DO
      1. Implement arm E behind a const NOTES_STRATEGY in summarize.rs (E default) so A, B and E are runnable by the eval; arm C is a model-path env override used only by the eval.
      2. Extend tests/meeting_notes_recall_eval.rs (NT4 owns the file) to run each arm and print one numeric table row per arm.
      3. docs/decisions/notes-strategy-2026-10.md: the table (rows labelled A, B, C, E, numbers with a decimal point), a line starting "DECISION:" naming the
         shipped arm, and the follow-up item ids. Commit the same numbers to docs/BUDGETS.md under a heading containing "summary recall".
      4. The chosen arm is the default in the shipped code and the fallback copy above is in the UI path (a test asserts each of the three fallback renderings).
      5. The Summary input covers every chunk (the guards' last-chunk test stays green).
    NOT: no second local model by default (C is measurement only); no hosted model; no change to the polish_model setting.
    Human half (numbers only, never text): run the notes metrics against a COPY of the 31-minute meeting through scripts/eval/compare-reference.py --notes and paste the
    verbatim ratio, duplicate count, max items per segment and owner counts in the PR body.
    DEPENDS: yap24-NT4 eval, yap-audit-eval-call-bleed fixture, yap-audit-notes-guards, yap24-NT11 (AEC), PR 250.
  `,
  acceptance: `
    test -f docs/decisions/notes-strategy-2026-10.md
    grep -q '^DECISION:' docs/decisions/notes-strategy-2026-10.md
    grep -q 'summary recall' docs/BUDGETS.md
    [ "$(grep -cE '^\| [ABCE] \|.*[0-9]\.[0-9]' docs/decisions/notes-strategy-2026-10.md)" -ge 3 ]
    grep -q 'NOTES_STRATEGY' desktop/src-tauri/src/summarize.rs
    (cd desktop/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" YAP_EVAL_REQUIRE=1 YAP_MODEL_DIR="$HOME/code/wilson-voice-loop/cache/models" cargo test --features custom-protocol --test meeting_notes_recall_eval -- --ignored --nocapture --test-threads=1)
    (cd desktop/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test summary_default_notes)
    (cd desktop/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test summary_audit_guards)
  `,
})

ITEMS.push({
  id: 'yap-audit-rewrite-notes', prompt: 'yap-audit', branch: 'loop/yap-audit-rewrite-notes-any-meeting', gated: 'panel',
  title: 'Rewrite notes on any finished meeting, from the card and the detail view, through the retry command PR 250 already ships; the old notes stay until the new run succeeds',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/summary_jobs.rs ) || [ "$(gh pr view 246 --json state -q .state 2>/dev/null)" = OPEN ] || ! grep -q 'VERBATIM_NARRATIVE_MAX' desktop/src-tauri/src/summarize.rs; then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 250 (summary_jobs.rs) has not landed, PR 246 (Meetings.tsx, progress.ts) is still open, or yap-audit-notes-guards (VERBATIM_NARRATIVE_MAX) has not landed (a rewrite before the guards returns the same grade-1 notes); land them, then build this"; exit 0; fi; grep -q 'Rewrite notes' desktop/src/views/Meetings.tsx && test -f desktop/src/meetings/rewriteNotes.test.tsx )
  `,
  spec: `
    Panel 3 2026-10-26: APPROVED-WITH-CHANGES (two seats). The backend already exists; this is a small UI-and-gate item.
    Audit issue #7 (HIGH) and #12. Two 10+ min meetings carry pre-PR-250 notes that are a 5x repetition loop cut
    mid-sentence and a 56-word excerpt; the backlog banner only offers meetings WITHOUT notes and the card shows
    "Write notes" only when there is no summary. The detail view has no notes action.
    EVIDENCE: PR 250 already ships summarize_meeting (lib.rs) and retry_meeting_summary, which submit whether or not a summary exists, and
    summarize_meeting_blocking refuses an empty overwrite ("the existing summary was kept"), so the old notes survive a failed run. The ONLY gate is
    canRetryNotes(..., hasSummary) in desktop/src/meetings/progress.ts (about lines 104-112). A new rewrite_notes command would duplicate all of that.
    DO
      1. Change canRetryNotes to allow a complete or partial meeting that already has a summary. No new Rust command: the button invokes retry_meeting_summary.
      2. A "Rewrite notes" button on the card and a notes action row (Copy, Rewrite notes) in the detail view.
      3. While it runs the notes area reads exactly: "Rewriting. Your current notes stay until the new ones are ready." No "older version" hint is shown.
      4. If the notes model is not installed, show the one-line download size (about 1.1 GB) and the existing consent BEFORE the run; the rewrite never starts a silent
         fetch (run_one -> fetch_model must not run without the existing consent).
      5. desktop/src/meetings/rewriteNotes.test.tsx covers both placements, the keep-old-until-success rule, the copy above, and the model-missing line.
      6. DEPENDENT (only when src/meeting_aec.rs from yap24-NT11 is on main at build time; otherwise yap24-NT11 carries this step when it lands): when the meeting has
         audio_kept=1, offer "Re-transcribe and rewrite", because rewriting over an echo transcript re-files the same wrong owners. It re-runs the transcription
         (so the AEC pre-pass applies) and then the notes job.
    NOT: no fn rewrite_notes, no new Tauri command, no notes history UI.
  `,
  acceptance: `
    grep -q 'Rewrite notes' desktop/src/views/Meetings.tsx
    grep -q 'retry_meeting_summary' desktop/src/views/Meetings.tsx
    grep -q 'Your current notes stay until the new ones are ready.' desktop/src/views/Meetings.tsx
    ! grep -q 'fn rewrite_notes' desktop/src-tauri/src/lib.rs
    (cd desktop && npx vitest run src/meetings/rewriteNotes.test.tsx)
  `,
})

ITEMS.push({
  id: 'yap-audit-asr-unload-before-notes', prompt: 'yap-audit', branch: 'loop/yap-audit-asr-unload-before-notes', gated: null, builder: 'sonnet',
  title: 'The ASR engine is unloaded before the notes sidecar starts (unless a dictation ran in the last 60 s), so the post-meeting peak is one model, not two',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/summary_jobs.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 250 (summary_jobs.rs) has not landed; land it, then build this"; exit 0; fi; test -f desktop/src-tauri/tests/notes_unloads_asr_first.rs )
  `,
  spec: `
    Audit issue #9 (MEDIUM). On Wilson's Mac the ASR model (in-process, ~1.1 GB) stays loaded ~20 min after a meeting
    (unloads logged 21:17:43Z after a 20:57:48Z transcript; 16:49:05Z after 16:16:19Z) while the notes sidecar runs
    at 1.37 GB RSS: about 2.5 GB peak against the 8 GB M1 floor.
    DO: in the notes backend's start path, if no dictation take ended in the last 60 s, unload the ASR engine (the
    same call the idle unload uses) before spawning yap-polish; the next dictation reloads lazily (YV80 lazy default).
    A dictation that starts during notes reloads ASR as today; notes never block a take.
    tests/notes_unloads_asr_first.rs: with a stub engine and a stub sidecar launcher, assert the unload happens before
    spawn when idle, and does not happen when a take ended 10 s ago.
  `,
  acceptance: `
    (cd desktop/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test notes_unloads_asr_first)
  `,
})

ITEMS.push({
  id: 'yap-audit-meeting-timings', prompt: 'yap-audit', branch: 'loop/yap-audit-meeting-timings-in-the-log', gated: null,
  title: 'The log explains a slow meeting: decode ms per chunk, one RTF line per track, notes phase timings and rejection counts by reason, and a true retune line',
  preflight: `
    grep -q 'meeting rtf' desktop/src-tauri/src/meeting_pipeline.rs && test -f desktop/src-tauri/tests/meeting_timing_lines.rs
  `,
  spec: `
    Audit issues #10 and #16 (MEDIUM/LOW). The 31-min meeting took 4:01 to transcribe (RTF 0.13) vs 0:28 for 24 min
    three days earlier (RTF 0.019); the log cannot say why (no per-chunk timing; loop CI load and thermal fair were
    both present). Notes runs log one line per rejection but no phase timings. A mid-meeting input retune logged
    "0 samples captured so far" 30 minutes into a capture (the dictation fallback counter, not the meeting's).
    DO
      1. meeting_asr / pipeline: debug line per chunk (track, index, audio s, decode ms, boundary kind); one info
         line per track at completion: "meeting rtf track=N audio_s=.. wall_s=.. rtf=.. chunks=.. preempted=..".
      2. summarize: one info line per run with model load ms, MAP pass count and ms, headline ms, reduce ms, and
         rejection counts by reason (v2_runaway=.. v3_ungrounded=..) instead of one line per rejection (keep the
         per-rejection lines at debug).
      3. record.rs retune line: for a meeting capture report the meeting's captured seconds; add the retune (time,
         old/new format) to the meeting diagnostics JSON.
      4. tests/meeting_timing_lines.rs: format functions are pure and tested; no log scraping of real data.
  `,
  acceptance: `
    grep -q 'meeting rtf' desktop/src-tauri/src/meeting_pipeline.rs
    (cd desktop/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_timing_lines)
  `,
})

ITEMS.push({
  id: 'yap-audit-eval-call-bleed', prompt: 'yap-audit', branch: 'loop/yap-audit-eval-call-bleed-fixture-and-metrics', gated: null, builder: 'sonnet',
  title: 'The audio box, offline: a synthetic two-voice call-bleed fixture with planted commitments, echo-dup and notes-owner metrics in meeting_eval, and a numbers-only reference comparison script',
  preflight: `
    grep -q 'call-bleed-2voice' desktop/src-tauri/tests/fixtures/meeting_eval_manifest.json && test -x scripts/eval/compare-reference.py && grep -q 'fn word_trigram_containment' desktop/src-tauri/src/meeting_echo.rs
  `,
  spec: `
    Audit issue #13 (MEDIUM) and section 9. The NT0 corpus has no fixture where the far side leaks into the mic,
    no conversation between two voices and no planted owners, so issues #1-#4 and #2 cannot be regression-tested.
    DO
      1. Extend meeting_eval_generate_corpus (tests/meeting_eval.rs) with fixture call-bleed-2voice (~4 min, macOS
         say, two voices): track1.wav = far side; track0.wav = near side plus far side at -12 dB, delayed 60 ms,
         short reverb tail; turns of 2-8 s; a second near-side voice for 30 s; 6 planted commitments with owners and
         2 decisions in meta.json; reference.txt with speaker tags. Regenerate the manifest + sha256 the way NT0
         documents (never hand-edit hashes).
      2. Metrics (pure functions, unit-tested in tests/meeting_eval_metrics.rs): echo_dup_rate (share of call
         windows whose word trigrams are >= 30% contained in the mic window within +-1.5 s; it CALLS the production pure fn
         meeting_echo::word_trigram_containment in desktop/src-tauri/src/meeting_echo.rs, which this item creates as pub so the tests and the
         notes echo-owner guard (yap-audit-notes-guards) share ONE definition), mid_sentence_end_rate,
         median_segment_seconds, notes_owner_accuracy and summary_verbatim_ratio (word 4-gram containment).
         meeting_eval prints them for the fixture; gates are added by the items that fix them (NT11 sets echo_dup_rate
         <= 0.10, yap-audit-utterance-segments sets median < 12 s, NT4 sets owners).
      3. scripts/eval/compare-reference.py (python3 -I, executable): args = a COPY of wilson_voice.db, a meeting id, a
         reference transcript ("M:SS - Speaker" blocks). Prints ONLY numbers: offset, S/I/D and WER per track and as
         rendered, per-reference-speaker share in each track, echo rate, coverage. Never prints transcript text or
         names (speakers become A, B, C...). --self-test runs on a tiny synthetic pair and exits 0.
         Two extra inputs (panel 3, 2026-10-26): --tracks <t0 timed json> <t1 timed json> <reference> compares two timed-transcript files
         (the output of wilson-voice --transcribe-file <wav> --timed) with no DB, which is how the AEC pre-pass is measured on audio on disk;
         --notes prints, from a COPY of the DB, numbers only for one meeting's notes: Summary 4-gram verbatim ratio, duplicate lines across
         sections, max items per cited segment, and owner counts (Me / Them / Someone here / blank).
    DEPENDS: yap24-NT0 provisioning (corpus path); consumers are yap24-NT9 (CLI), yap24-NT4, yap24-NT11.
  `,
  acceptance: `
    grep -q 'call-bleed-2voice' desktop/src-tauri/tests/fixtures/meeting_eval_manifest.json
    test -x scripts/eval/compare-reference.py
    python3 -I scripts/eval/compare-reference.py --self-test
    grep -q 'fn word_trigram_containment' desktop/src-tauri/src/meeting_echo.rs
    (cd desktop/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test meeting_eval_metrics)
  `,
})

ITEMS.push({
  id: 'yap-audit-notes-quit-cancel', prompt: 'yap-audit', branch: 'loop/yap-audit-notes-quit-cancel-one-line', gated: null, builder: 'sonnet',
  title: 'Quitting during notes interrupts the job with one line ("resumes next launch") instead of N failed MAP passes',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/summary_jobs.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 250 (summary_jobs.rs) has not landed; land it, then build this"; exit 0; fi; test -f desktop/src-tauri/tests/notes_quit_interrupts.rs )
  `,
  spec: `
    Audit issue #14 (LOW). On the live 31-min meeting the app was quit 16 s into notes: the log shows 8 warn lines
    "a MAP pass failed (unavailable) - continuing" and "every one of 8 MAP pass(es) failed - refusing to overwrite"
    in 1 s. The launch resume then worked (27 s after relaunch). Wrong signal: it reads like a model failure.
    DO: when the app is quitting, the notes client returns Cancelled-by-quit (not Unavailable) so summarize stops at
    the next pass with one info line "notes for <id> interrupted by quit - resumes next launch"; the ledger is not
    marked cancelled (the resume must still run). tests/notes_quit_interrupts.rs with a stub client.
  `,
  acceptance: `
    (cd desktop/src-tauri && YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --test notes_quit_interrupts)
  `,
})

ITEMS.push({
  id: 'yap-audit-remote-speaker-names', prompt: 'yap-audit', branch: 'loop/yap-audit-remote-speaker-names-active-speaker', gated: 'panel',
  title: 'DEFERRED to the next release: name remote speakers locally from the call app\'s active-speaker indicator (Accessibility); v1 uses the calendar 1:1 name inside yap24-NT4 instead. SPIKE, or record NO-GO',
  preflight: `
    ( if ! ( test -f desktop/src-tauri/src/call_signals.rs && test -f desktop/src-tauri/src/meeting_aec.rs ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): yap-call-0 (call_signals.rs, call-app catalog) or yap24-NT11 (meeting_aec.rs; a name on Them leaves the echoed half in Me until echo_dup_rate <= 0.10) has not landed; land them, then build this"; exit 0; fi; head -1 docs/loop/evidence/yap-audit-remote-speaker-names.md | grep -qE '^SPIKE-VERDICT yap-audit-remote-speaker-names (GO|NO-GO)' )
  `,
  spec: `
    Panel 3 2026-10-26: DEFERRED to the next release (both seats: RETHINK). NOT in panelApproved. Kept here, gated, so nothing ages out.
    WHY DEFERRED: it reads another app's participant tree every second (a new SECURITY-class surface beyond yap-call-2's title-only Accessibility read, same review
    as yap-call-2); Zoom's grid is custom-drawn with no Accessibility speaker flag, and Meet needs Chrome's renderer accessibility forced on (CPU); indicator lag
    plus echo puts a wrong name on a line, which is worse than "Them". Today a name on Them leaves the echoed half in Me, so this needs the AEC pre-pass first.
    V1 ALTERNATIVE (owned by yap24-NT4, DO R5): for a 1:1 call the one other calendar attendee is an honest name with no new OS surface; the notes owner of a
    Them-track item uses that name when exactly one non-self attendee exists, otherwise stays "Them".
    NEXT-RELEASE NOTE (not v1): once diarization (yap24-NT5, deferred until after 100 users) runs on the call track it gives Them A / Them B with NO new
    permission; that is the second alternative to evaluate before any Accessibility grid read.
    Audit section 10 (biggest gap vs the cloud notetaker). The reference product on this Mac ships remote-speaker attribution from the meeting app's own UI
    (diarization/remote_speaker.rs, active_speaker_heuristics/odd_one_out.rs), sampled per second. Yap labels the whole call track "Them".
    SPIKE DO: for Zoom desktop and Google Meet in Chrome only, read the active-speaker element per second during a test call (synthetic voices), map seconds to
    call-track utterances, report precision of the name per utterance; write docs/loop/evidence/yap-audit-remote-speaker-names.md whose first line is
    "SPIKE-VERDICT yap-audit-remote-speaker-names GO" or "... NO-GO" with the numbers. No product code ships from the spike.
    GO BAR (all required): per-utterance name precision >= 0.9 on Zoom AND on Meet; sampler CPU <= 3%; no new TCC prompt beyond the one yap-call-2 already shows; and
    NT11 echo_dup_rate <= 0.10 on call-bleed-2voice. Anything else is NO-GO.
    SECURITY NOTE (seat A): participant names are other people's data read from another app; the read is local-only, never logged with names, never stored beyond the
    meeting row, and needs its own consent line; class SEC.
    DEPENDS: yap-call-0, yap24-NT11, yap-audit-utterance-segments.
  `,
  acceptance: `
    head -1 docs/loop/evidence/yap-audit-remote-speaker-names.md | grep -qE '^SPIKE-VERDICT yap-audit-remote-speaker-names (GO|NO-GO)'
  `,
})
