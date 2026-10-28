# Meeting notetaker audit — 2026-10-26

Stamp: 2026-10-26T16:44:50Z (audit started 16:33:19Z, `date -u`). Seat: MEETING NOTETAKER AUDIT (read-only).
Installed build: `/Applications/Yap.app` 0.9.3-dev, built 2026-10-19T20:45:52Z = main `a740614` + PR #250 head `0a87249`.
Line citations: `a740614:` for code that #250 does not touch, `0a87249:` for #250 files (summarize.rs, summary_jobs.rs, Meetings.tsx, NotesView.tsx, progress.ts).
Data: `~/Library/Application Support/WilsonVoice/` (`wilson_voice.db` read through a scratch COPY of db+wal, and with `?immutable=1`; `logs/yap.log`; `meetings/*.wav`, `*.asr-progress.json`).
Privacy: snippets are at most 10 words, other participants are never named ("Speaker A–D", "a participant"), and no transcript or notes text is copied here beyond those snippets.

What happened during the audit: the live 31-min meeting `c6187806` finished transcription at 16:34:41Z. At 16:34:57Z the app was quit (not by this seat), which stopped the notes sidecar mid-MAP. It relaunched at 16:35:03Z, resumed the notes at 16:35:24Z, and wrote them at 16:38:35Z.

## Prioritized issues

| # | Sev | What | Evidence | User impact | Fix (one sentence) | Owner |
|---|---|---|---|---|---|---|
| 1 | CRITICAL | On built-in speakers the far side is transcribed twice. The mic ("Me") track re-hears the call, and the notes give the call's words to "Me". | Share of call-track word trigrams also found in the mic track's same 30 s window: 61d72bdd 0.71, 445654ee 0.79, c6187806 0.89. Call windows that are echoes: 8/12, 18/19, 45/51. Notes owners: 18/18 actions "Me" on 61d72bdd, 17/19 on c6187806. Small talk "Have you seen Breaking Bad?" appears as two actions (Me and Them) from segments faf752ca/93138b0f. Ground truth (§10): far-side Speakers A/C have 64%/74% of their words in BOTH tracks. | "Speaker separation is bad" exactly as Wilson saw: every remote sentence shows up twice, once as Me, and other people's commitments are filed under Wilson. | Cancel the echo before dedupe: run AEC on the mic with the system track as far-end reference (the pure-Rust `aec3` crate is what Read AI ships). Fall back to NT11's residue dedupe, and un-defer NT11 now that the spike evidence exists. | yap24-NT11-0, yap24-NT11 (appended) |
| 2 | CRITICAL | The #250 notes are re-transcripts, not summaries. The Summary is lifted transcript, key points are raw lines, "actions" are narration or small talk, and the same line appears in up to 3 sections. | 4-gram verbatim ratio of the Summary paragraph: 0.97 (61d72bdd, starts with a leaked `Me:` prefix), 1.00 (445654ee), 0.91 (c6187806). Key points backfilled from raw sentences (`backfill_key_points`, 0a87249:summarize.rs:2430). Cross-section duplicates: 4 and 8. One segment (4e61a602) yields 10 near-identical actions, e.g. "Start practicing the way they smell". Rejections per run: 17 runaway + 38 ungrounded (61d72bdd); 3 + 15 (445654ee); 17 + 22 + 1 preamble (c6187806). | The notes are unreadable and untrustworthy, the headline promise of Notetaker. | Mechanical guards now (refuse a verbatim narrative, strip speaker prefixes, one section per item, at most 2 items per segment, a non-fragment title). Then a panel decision on the strategy: the 0.80 content-word groundedness floor (0a87249:summarize.rs:135, :1418) rewards copying and rejects paraphrase, and a 1.5B model cannot summarize under that gate. | NEW yap-audit-notes-guards, NEW yap-audit-notes-strategy (panel); yap24-NT4 (appended) |
| 3 | HIGH | Chunk boundaries are a fixed 30 s clock. VAD-aligned cutting is built but switched off in production. | `vad: None` at a740614:meeting_pipeline.rs:522. All 414 chunks in the progress files are `fixed_clock`/`edge` with zero silence boundaries. Segments ending mid-sentence (no terminal punctuation): t0 8/28, 8/48, 12/47, 15/62. | Words are split at seams and every segment is cut wherever the clock fell. | Pass the warm Silero VAD into `MeetingAsr` (`plan_windows` already supports it, a740614:meeting_asr.rs:379-440). | NEW yap-audit-vad-boundaries |
| 4 | HIGH | A stored "segment" is a whole 30 s window, so the transcript reads as alternating 30 s Me/Them paragraphs with no turns. | Average segment length 29.4–30.1 s in every meeting. Token spans exist in `*.asr-progress.json` (`timestampKind: token` in 273+7+4+3 chunks) but are never used to split. | A 2-person exchange cannot be followed. Labels apply to 30 s of mixed speech. | Split each window into utterance segments at token-span pauses and sentence ends before insert. | NEW yap-audit-utterance-segments (panel); yap-notetaker-notes (appended) |
| 5 | HIGH | The call (system) track dies mid-call, or never attaches. | 445654ee: `all_zero_buffers` at 12:00, rebuild 2/3 timed out, call track ends at 690 s of 883 s (tap_rebuilds JSON). d8761579 (kind unknown): `no_system_audio_observed` after 3 rebuilds, mic only for 9 min. | The far side is missing for the last 3 min (only the echo in the mic keeps it). | Track the output device and retarget the tap on route change instead of rebuilding the same dead tap. | yap24-NT3 (appended) |
| 6 | HIGH | In-person and "Not sure" meetings show "Speaker" on every row. The diarization sidecar ships in the bundle but has no caller. | ba4dac81 (in_person): 47/47 rows → `UNCLUSTERED_SPEAKER_LABEL` (a740614:meetings.rs:960-968). `diarize::pool()` has no caller (meeting_matrix.rs:143). `/Applications/Yap.app/Contents/MacOS/yap-diarize` is installed but idle. cluster_index is NULL on all 273 rows. | No speaker separation at all in a room. | Wire the diarize pool after transcription for the ClusterTrackA target. | yap24-NT5 (appended) |
| 7 | HIGH | Notes written by the pre-#250 build are broken and cannot be rewritten. | ba4dac81: the same 3 sentences repeat 5×, then the text is cut mid-sentence (one 9-word loop sentence: "The economic order quantity of $155 is not worth"). d8761579: a 56-word verbatim excerpt. The backlog offer only covers meetings WITHOUT notes (0a87249:summary_jobs.rs:522-540). No Rewrite button on a complete meeting (0a87249:Meetings.tsx:344-350 shows "Write notes" only when `!hasSummary`). | Two 10+ min meetings keep garbage notes forever. | A "Rewrite notes" action on every complete meeting (card + detail) that keeps the old notes until the new run succeeds. | NEW yap-audit-rewrite-notes (panel) |
| 8 | MEDIUM | A dictation spoken during meeting transcription waits on the meeting decode. | 16:31:38Z "cancelling the in-flight meeting chunk decode", then `asr_decode_ms=5166` for a 51.8 s take, against `asr_decode_ms=338` for a 22.8 s take that morning (15:11:18Z). | About 4–5 s of lag on the first dictation after a meeting. | The cancel must preempt within one decoder step. Measure take latency while a meeting job runs. | yap24-NT2 (appended) |
| 9 | MEDIUM | The ASR model (in-process, ~1.1 GB) stays resident ~20 min after a meeting and overlaps the notes sidecar (1.37 GB RSS). | ASR unloaded 21:17:43Z after transcript 20:57:48Z (10-21). 16:49:05Z after 16:16:19Z (10-23). yap-polish RSS 1,366,672 KB at 16:37:08Z. Host RSS 1,345.8 MB at 16:31:43Z. | About 2.5 GB peak on the 8 GB M1 floor, next to a browser and a call app. | Unload ASR before spawning the notes sidecar when no dictation ran in the last 60 s (YV80 lazy reload already exists). | NEW yap-audit-asr-unload-before-notes |
| 10 | MEDIUM | No per-chunk or per-phase timing in the log, so slow runs cannot be explained. | c6187806 took 4:01 to transcribe (stop 16:30:40Z → 16:34:41Z, RTF 0.13), against 0:28 for 24 min on 10-21 (RTF 0.019). ps at 16:35Z showed loop CI on the same Mac (3× yap-diarize ~95% CPU, 2× debug wilson-voice ~165%) plus thermal `fair`. Cause unprovable from the log. | Wilson waits minutes with no explanation, and the team cannot tell contention from regression. | Log decode ms per chunk, an RTF line per track, and notes phase timings with rejection counts by reason. | NEW yap-audit-meeting-timings |
| 11 | MEDIUM | Crash recovery for a meeting that was still recording is still unwired. | `recover_orphaned_meetings` (a740614:meeting.rs:1599) is called only from its test (meeting.rs:4026). | A crash mid-meeting loses the recording row's journal. | As NT1 specifies. | yap24-NT1 (appended) |
| 12 | MEDIUM | The Meetings UI shows engine words and stale state. | Detail shows the raw state `complete` (0a87249:Meetings.tsx:428). The card counts "segments", which are 30 s windows (:320-321). Detail "does not refresh itself" (:452). No Write/Retry notes in detail (only on the card, :339-350). No copy, edit or rewrite for notes (NotesView.tsx:45-60 is render-only). No find-in-transcript or copy (TranscriptList.tsx:44-55). | The UI feels unfinished, and a user cannot act on bad notes from where they read them. | Plain-words state, minutes instead of segments, a live-refresh detail, a notes action row (Copy, Rewrite). | yap24-NT6 (appended); NEW yap-audit-rewrite-notes |
| 13 | MEDIUM | No repeatable test reproduces findings 1–4 offline. | The NT0 corpus (46 MB, 7 fixtures, `say` voice Samantha) has a two-track ORDERING fixture but no bleed (far side mixed into mic), no 2-voice conversation, and no planted commitments with owners. | Every fix above would ship unmeasured. | Add a `call-bleed-2voice` fixture and an echo-dup / notes-owner metric to meeting_eval, plus a private reference-comparison script for real meetings. | NEW yap-audit-eval-call-bleed (feeds NT9, NT4, NT11) |
| 14 | LOW | Quitting the app mid-notes logs 8 "MAP pass failed (unavailable)" warnings instead of one "interrupted". | 16:34:57Z–16:34:58Z log. Resume worked 27 s after relaunch (16:35:24Z). | Noise in the log, misread as a failure. | The quit path cancels the job with one INFO line "resumes next launch". | NEW yap-audit-notes-quit-cancel |
| 15 | LOW | Every launch still GETs the decommissioned Forge host. | 16:35:12Z "revocation refresh skipped (error sending request for url (https://forge.87-99-149-214.sslip.io/...))". license.rs:156. | A network call to a dead third-party-resolvable name. Not in the meeting path. | Delete it, as the item says. | yap-lic-00-kill-forge-fetch (appended) |
| 16 | LOW | A second recorder (Read AI) on the same Mac flipped the MacBook mic from 1 ch to 3 ch mid-meeting. Yap retuned, but the log line misleads. | Read AI's file closed 16:29:05Z. Yap: 16:29:31Z "48000Hz/1ch → 48000Hz/3ch (0 samples captured so far…)" 30 min into capture. Track 0 then measured −93.5 ppm over 2 clock segments (earlier meetings −8.7). | Harmless today. The ppm jump would matter to NT11's merge. | Count meeting samples, not the dictation fallback, in that line, and record the retune in diagnostics. | NEW yap-audit-meeting-timings |
| 17 | LOW | Audio is stored as 16-bit WAV: 1.92 MB/min per track (60 MB per 31-min track, 320 MB on disk for 7 kept tracks). | `meetings/` listing. Read AI stores both channels as 122 kbps AAC (≈0.92 MB/min total). | Disk only. Purged at 7 days. | Consider FLAC/Opus when NT1's 5 GB ceiling lands. | yap24-NT1 (appended) |
| 18 | LOW | The first post-upgrade meeting downloads the 1.1 GB notes model automatically from huggingface.co. This is the only network the meeting path can trigger. | `allows_auto` (0a87249:summary_jobs.rs:495-498) → `fetch_model` (0a87249:lib.rs:5132-5160). The banner discloses it only for the backlog. | A surprise download. No meeting data leaves the Mac. | Say "downloading the notes model (1.1 GB) once" on the card, which already exists as "Getting the notes model N%". Just make it visible before the first run. | yap24-NT4 (appended) |
| 19 | LOW | One ASR repetition run at the end of c6187806. | Segment a38e6f0a (29:00): "thank you yo yo yo yo yo yo". No other 5-gram×3 loops in any meeting. | Cosmetic. | Covered by the eval gate (13). | NEW yap-audit-eval-call-bleed |

## 1. Inventory (DB copy at 16:35Z + 16:39Z, `immutable=1` cross-check)

| id | started (UTC) | dur | kind | state | tracks | audio on disk | segs t0/t1 | speaker labels shown | notes / model | error |
|---|---|---|---|---|---|---|---|---|---|---|
| 9ba4f7e4 | 08-14 18:01 | 0:10 | unknown | failed | — | purged | 0 | — | — | audio removed before transcription |
| 56964c52 | 08-15 12:15 | 0:04 | unknown | failed | — | purged | 0 | — | — | same |
| cc9f6153 | 08-16 23:18 | 0:08 | in_person | failed | — | purged | 0 | — | — | same |
| 0a052e58 | 08-20 22:01 | 0:01 | unknown | failed | — | purged | 0 | — | — | same |
| e4cd21f0 | 10-10 05:40 | 0:06 | unknown | failed | — | purged | 0 | — | — | same |
| 8f9b3c78 | 10-16 20:05 | 0:10 | unknown | complete | t0 | purged | 1/— | none (mic only) | 153 ch, qwen2.5-1.5b q4_k_m (pre-#250) | — |
| f38cb44e | 10-19 16:42 | 0:10 | unknown | complete | t0+t1 | 316 KB + 315 KB | 1/1 | Speaker / Them | 51 ch (pre-#250) | — |
| d8761579 | 10-19 17:12 | 9:15 | unknown | complete | t0 (tap lost) | 18 MB | 17/— | none | 409 ch (pre-#250) | — |
| ba4dac81 | 10-19 17:22 | 24:56 | in_person | complete | t0 | 48 MB | 47/— | none (mic only; "Speaker" target) | 1,826 ch (pre-#250) | — |
| 61d72bdd | 10-21 20:33 | 24:11 | virtual | complete | t0+t1 | 46 MB + 46 MB | 48/12 | Me / Them | 3,263 ch (#250) | — |
| 445654ee | 10-23 16:01 | 14:43 | virtual | complete | t0+t1 (t1 dies 11:30) | 28 MB + 28 MB | 28/19 | Me / Them | 920 ch (#250) | — |
| **c6187806 (live)** | 10-26 15:59 | **31:04** | virtual | transcribing 1217 s at 16:33Z → complete 16:34:41Z → notes 16:38:35Z | t0+t1 | 60 MB + 60 MB | 62/51 | Me / Them | 4,489 ch (#250) | — |

There are 12 meetings. 5 of them run over 9 min, and 4 run over 10 min: ba4dac81, 61d72bdd, 445654ee, c6187806. `cluster_index` is NULL on every segment. `processed_through_seconds` equals the duration on every complete row.

## 2. Pipeline timelines (log stamps)

| meeting | stop | ASR load | transcript done | ASR wall | RTF (wall/audio) | notes start → done | notes wall | MAP chunks | actions / dropped | notes "truncated" |
|---|---|---|---|---|---|---|---|---|---|---|
| ba4dac81 (old build) | 17:47:33 | 17:47:34 | 17:47:49 | 16 s | 0.011 | 17:47:49 → 17:48:24 | 35 s | 3 | 3 / 0 | true |
| 61d72bdd | 20:57:20 | 20:57:20 | 20:57:48 | 28 s | 0.019 (both tracks) | 20:57:48 → 21:00:05 | 2:17 | 6 | 18 / 49 | false |
| 445654ee | 16:15:54 | (warm) | 16:16:19 | 25 s | 0.028 | 16:16:19 → 16:17:07 | 48 s | 2 | 3 / 60 | false |
| c6187806 | 16:30:40 | 16:30:40 | 16:34:41 | **4:01** | 0.13 | 16:34:41 → quit 16:34:57 → resume 16:35:24 → 16:38:35 | 3:11 after resume | 8 | 19 / 32 | false |

The ASR window is a fixed 30 s with a 2 s overlap. Model: `parakeet-unified-en-0.6b-Q8_0.gguf` on Metal (M4 Pro). The log has no per-chunk latency. Notes sidecar model load time is not logged. In every run, "meeting pipeline: X → complete" is logged BEFORE notes start, so the card reads "Transcript ready" and then "Writing notes".

Where the time goes: ASR is fast when the Mac is idle (≤0.03 RTF). Notes take 35 s–3 min, dominated by MAP passes that are mostly rejected (55 rejections on 61d72bdd). The 31-min run's 4 min of ASR overlapped a hands-free dictation (16:30:45–16:31:37Z), loop CI load, and thermal `fair`. There were no crashes, OOMs, or "waiting for model". There was one preemption (16:31:38Z) and one quit-induced notes interruption with a clean resume.

## 3. Transcript quality proxies (no audio listened; text + timing only)

| meeting | t0 segs / wpm | t1 segs / wpm | identical consecutive | overlaps / non-monotonic | starts lowercase | ends mid-sentence t0 / t1 | 5-gram loops | numbers digits / spelled | echo windows (≥30% trigram) | trigram containment t1⊂t0 |
|---|---|---|---|---|---|---|---|---|---|---|
| d8761579 | 17 / 73 | — | 0 | 0 / 0 | 0 | 3/17 | 0 | 18 / 13 | — | — |
| ba4dac81 | 47 / 86 | — | 0 | 0 / 0 | 0 | 12/47 | 0 | 78 / 42 | — | — |
| 61d72bdd | 48 / **196** | 12 / 10 | 0 | 0 / 0 | 2 | 8/48, 2/12 | 0 | 14 / 24 | **8/12 (0.67)** | **0.71** |
| 445654ee | 28 / 68 | 19 / 29 | 0 | 0 / 0 | 0 | 8/28, 9/19 | 0 | 3 / 7 | **18/19 (0.95)** | **0.79** |
| c6187806 | 62 / 146 | 51 / 84 | 0 | 0 / 0 | 0+2 | 15/62, 9/51 | 0 (one 6× "yo" run, a38e6f0a) | 22 / 17 + 33 / 36 | **45/51 (0.88)** | **0.89** |

Timestamps are clean (merge and seam handling works). The defects are structural: fixed-clock cuts (issue 3), 30 s segment granularity (issue 4), and echo duplication (issue 1). 61d72bdd's mic track runs at 196 wpm against 10 wpm on the call track, so the mic captured both sides, and the call track caught only fragments. Numbers mix digits and spelled-out words about 50/50, with no meeting-specific number formatting. Punctuation is present. No silence-as-text was found: no ≤3-word windows except 1–2 per meeting, and empty windows are not stored.

## 4. Speaker separation — what the app really does

The app does this: a speaker = the TRACK. Mic = "Me" only when kind = `virtual` AND a call track has text. Otherwise mic = "Speaker". System track = "Them" (a740614:meetings.rs:284-289, :960-968). There is no diarization in production. `yap-diarize` is bundled, but `diarize::pool()` has no caller. NT5 is deferred, and yap-notetaker-notes struck diarize.

What a user sees:
- In person or "Not sure": every row says "Speaker", one voice for the whole room (ba4dac81: 47/47 rows).
- Calls on built-in speakers: every remote sentence twice, once as "Me" and once as "Them". "Me" also holds anyone else in the room. In c6187806 (§10), a second local voice (Speaker D, 75% in Me, 20% in Them) is labelled "Me". So their commitments read as Wilson's.
- Calls whose tap dies (445654ee after 11:30): "Them" goes silent and the remote side survives only as "Me".

Is it honest? The labels are mechanically honest about channels (the code refuses to promote "Me" to a name). The UI implies people where there are only microphones, and the notes then print owners `(Me, 00:27:30)` that are wrong for most remote and second-local commitments. The backlog promises (NT5 diarization, NT11 dedupe) are both deferred. Today's data argues for NT11 (plus AEC) first, because it fixes the most common case (a call on laptop speakers).

## 5. Notes (#250 as installed) — grades

| meeting | build | Summary paragraph | key points | actions (real) | decisions | open q | dups across sections | grade | why |
|---|---|---|---|---|---|---|---|---|---|
| 8f9b3c78 (10 s) | old | 1 verbatim line | — | 0 | 0 | 1 (verbatim) | 0 | 2 | Test clip. Verbatim but harmless. |
| f38cb44e (10 s) | old | 1 verbatim line | — | 0 | 0 | 0 | 0 | 2 | Same. |
| d8761579 (9 min) | old | 56 words, 100% verbatim, "shortened" footer | — | 0 | 0 | 0 | 0 | 1 | An excerpt, not a summary. |
| ba4dac81 (25 min) | old | 240 words: 3 sentences ×5, cut mid-word | — | 3 (1 is a question) | 0 | 0 | 0 | 1 | Runaway repetition loop. |
| 61d72bdd (24 min) | #250 | 251 words, 0.97 verbatim, leaked "Me:" prefix | 3 (2 raw lines) | 18, all "Me". 10 from one segment 4e61a602 ("Start practicing the way they smell") | 3 (copies of actions) | 4 | 4 | 1 | Re-transcript plus list explosion. Owners wrong because of echo. Title names a participant. |
| 445654ee (15 min) | #250 | 21 words, 1.00 verbatim. Title = transcript fragment "I'm gonna do this so that's easier That's" | 3 (all raw lines) | 3: small talk, duplicated Me+Them (echo) | 1, invented narration "The speaker paused while changing music." | 2 | 0 | 1 | Nothing in it is a meeting outcome. |
| c6187806 (31 min) | #250 | 191 words, 0.91 verbatim | 3 (2 raw lines) | 19 (17 Me). ~3 real commitments, wrong owner. First-person narration as actions ("It's really dope.") | 6 (0 real) | 10 (5 are statements) | 8 | 1 | The real outcomes are buried and mis-owned. The decision to split the website is missed. |

Context and truncation: MAP budget is 1400 tokens/chunk (0a87249:summarize.rs:94), and the 4096 ctx was respected. The #250 runs report `truncated=false`. The old build truncated (ba4dac81, d8761579). Output budgets hit: "a pass reached its output budget" on every #250 run. The gates work against invented numbers and contacts (no v4/v5 hits, and the names checked are all in the transcript). They do not stop copying: copying is what they reward.

## 6. Runtime

| phase | process | RSS | CPU | threads | notes |
|---|---|---|---|---|---|
| recording (live, 31 min) | wilson-voice | 51.8–63.3 MB (16:07–16:27Z hygiene lines) | low | — | ASR not loaded while recording. |
| transcription | wilson-voice (ASR in-process, Metal) | 875–1,346 MB | — | 35 (16:37Z, post-relaunch) | No ASR sidecar. The engine is shared with dictation. |
| notes | yap-polish `--model qwen2.5-1.5b-instruct-q4_k_m.gguf` | 1,366,672 KB RSS (ps) / 370 MB footprint (top) | 42–64% | 4 | Spawned per job. |
| coexistence | ASR + notes sidecar | ≈2.5 GB together | | | ASR held ~20 min after the transcript (21:17:43Z, 16:49:05Z unloads). |
| idle after notes | — | sidecar gone by 16:39:28Z (done 16:38:35Z) | | | Sidecar exits within a minute. Good. |
| disk | meetings/*.wav | 16 kHz s16 mono = 1.92 MB/min/track | | | 320 MB for the kept tracks. Progress JSON up to 314 KB/track. |

Against the 8 GB M1 floor (panel PS-43 VM gate): recording is fine. The post-meeting peak of ≈2.5 GB is the risk, and unloading ASR before notes (issue 9) cuts it to ≈1.4 GB.

## 7. Networking — PASS (no CRITICAL)

`lsof -nP -a -i -p` on Yap (pid 59844) and its child yap-polish (60477), sampled 16:37:08Z, 16:37:29Z, and 16:39:28Z: **zero sockets** in all three samples. Caveat: transcription had ended at 16:34:41Z, so the samples cover the notes phase. The live-ASR phase could not be sampled after the relaunch. The source shows no HTTP client in meeting_*.rs, summarize.rs, or summary_jobs.rs.

Every host the app can contact (a740614 unless noted):

| host | where | why | meeting path? |
|---|---|---|---|
| huggingface.co | models.rs:202, :537, :835, :1217, :1274 | ASR / notes model downloads | Only the one-time notes-model fetch (issue 18). |
| blob.handy.computer | catalog.json:5 | Model mirror | Same. |
| github.com (snakers4/silero-vad) | vad.rs:491 | VAD model | No. |
| forge.87-99-149-214.sslip.io | license.rs:156 | Revocation GET at every launch to a dead host | No (issue 15). |
| yap-lemon.vercel.app, github.com releases | tauri.conf.json:65-69 | Updater | No. |
| buy.stripe.com, github.com issues | license.rs:176, lib.rs:7430 | Opened in the browser, not fetched | No. |

No transcript, audio or notes bytes leave the Mac.

## 8. UI/UX of the meeting flow — ceiling table

States and copy (0a87249:progress.ts:60-90): Recording → "Transcribing N%" (percent from `processedThroughSeconds`, real and monotonic: foldProgress :19-29) → "Transcript ready · Notes queued / Getting the notes model N% / Writing notes" → "Notes ready". Failed → "Failed" + Retry (only failed/partial, :93-95). Stop has no confirmation (Meetings.tsx:169-175). That is acceptable, since stop is not destructive. Crash or quit mid-meeting: NOT recovered (issue 11). Mid-notes: recovered (resume at launch). Errors are shown on the card (`m.error`, :334) and as toasts. None are swallowed in this view.

| job | Yap today | Granola (public product, UNVERIFIED here) | Read AI (§10) | verdict |
|---|---|---|---|---|
| Summary | Present, but 91–97% verbatim (§5) | Abstractive notes merged with the user's own jotted notes | Cloud summary (not on disk) | **short** |
| Action items + owners | Present. Owners are Me/Them and wrong under echo | Owners from names | Owners by speaker name | **short** |
| Decisions / questions | Present, but copies of actions | Yes | Key questions | **short** |
| "What did I miss" / ask | None | Chat over the meeting | Search Copilot (`SearchCopilotPage` asset) | **short** (not planned) |
| Export | Markdown export (Meetings.tsx:64-77, :357-363) | Copy / share link | Recap email, integrations | **matches** for local-first |
| Search | Global FTS over titles + words (:180-186) | Yes | Yes | **matches** |
| Transcript copy / find / speaker names | None / none / channel labels | Yes / yes / Me-Them | Names via active-speaker | **short** |
| Rewrite / edit notes | Only "Write notes" when there are none | Templates, re-enhance | Regenerate | **short** (issue 7) |
| Local, no bot, works offline | Yes | Local capture, cloud LLM | Cloud bot plus desktop uploader, Mixpanel | **clears** |

## 9. The "audio box"

What exists:
- NT0 corpus: 46 MB at `~/yap-eval-corpus/meetings`, 7 fixtures, generated by `say` (Samantha, 175 wpm), sha256 manifest `tests/fixtures/meeting_eval_manifest.json`.
- `tests/meeting_eval.rs`: WER gate 0.02 on lecture-15min, a seam-drift gate, and the two-track ordering markers `Me:word`/`Them:word`.
- `YAP_EVAL_REQUIRE` (NT2 makes a skip fatal).
- `--transcribe-meeting` is planned in NT9, which runs the real pipeline headless.

What is missing, and catches every defect found here:
- (a) A **call-bleed two-voice fixture**. Track 1 is the far side (voice B). Track 0 is the near side (voice A) plus the far side mixed in at −12 dB, delayed 40–80 ms, with a short reverb tail (built-in-speaker echo). The script covers turns of 2–8 s, a second local voice for part of it, and 6 planted commitments with owners plus 2 decisions.
- (b) Scored through the REAL pipeline by NT9's CLI (no virtual device), with these metrics:
  - per-track WER against the reference,
  - echo-dup rate (call trigrams found in the mic window; must fall to ≤0.10 after NT11),
  - share of segments that end mid-sentence,
  - median segment length (must fall below 12 s after utterance segments),
  - notes: action recall/precision and owner accuracy (NT4's ≥0.8), Summary 4-gram verbatim ratio ≤0.5, cross-section dups = 0.
- (c) A private `scripts/eval/compare-reference.py` that takes a DB copy, a meeting id and a reference transcript (for example a Read AI export) and prints ONLY numbers. That is how §10 was measured, and Wilson can rerun it on any real meeting.

Live path (optional, human half of NT11-0): no BlackHole is needed. Run `afplay far_side.wav` through the built-in speakers while `say` (or a person) speaks near side. Yap's own global system tap already captures the player process, and the mic hears the echo exactly as in a real call. A per-process `CATapDescription` on the `afplay` pid is NOT needed for this and is deferred (yap-call-3). Recommendation: build (a)+(b) offline first, because it is deterministic, headless and runs in the loop. Use the `afplay` run only as a one-time calibration of the bleed level and delay.

## 10. Ground truth — the 31-min meeting (c6187806) vs Read AI's transcript

Reference: Read AI's transcript export of the same meeting (`~/Documents/…Transcript.txt`, 89 turns, 4 speakers here called A–D, 3,636 normalized words; fillers uh/um dropped on both sides). It is a cloud bot product, so it is a reference, not truth. Alignment is a token `SequenceMatcher` (difflib, no autojunk). Read AI's 0:00 = Yap 3:28 (median offset 208 s over 45 anchored turns).

**10a. Accuracy (Yap span 3:15–30:30 against the full reference)**

| hypothesis | S | I | D | WER-style |
|---|---|---|---|---|
| Yap mic track only (t0) | 139 | 462 | 115 | **0.197** |
| Yap call track only (t1) | 104 | 243 | 1,529 | 0.516 (the call track misses the local speakers by design) |
| What the user sees (both tracks, ordered) | 128 | **2,808** | 111 | **0.838**, dominated by echo-duplicate insertions |

The worst 5 regions by Yap segment:
- 109955b9 (29:30) and 3cd492c4 (30:00): after Read AI's last turn, so there is no reference.
- 3ca51611 (4:30) 38%: e.g. Yap "would this call" vs reference "what is called".
- a38e6f0a (29:00) 34%: includes "thank you yo yo yo yo yo yo".
- 5c436f06 (22:00) 33%: mostly fillers the reference keeps ("you know", "on on a like").

Read AI is not perfect either: some of Yap's "insertions" are words Read AI dropped. So 0.20 is an upper bound on mic-track error.

**10b. Speakers**

| Read AI speaker | turns | words | in Yap "Them" | in Yap "Me" | in both (echo) | reading |
|---|---|---|---|---|---|---|
| A | 17 | 1,403 | 0.65 | 0.66 | 0.64 | Remote. Duplicated into Me. |
| B | 14 | 1,338 | 0.04 | 0.64 | 0.04 | The Mac's owner (local). |
| C | 15 | 476 | 0.77 | 0.75 | 0.74 | Remote. Duplicated into Me. |
| D | 28 | 466 | 0.20 | 0.75 | 0.19 | Mostly reached only the mic: local, or audio not routed through this Mac's output. Labelled "Me". |

Far-side turns missing from Yap's "Them": A 0 of 17, C 0 of 15, D 13 of 28. D turns missing from BOTH tracks: 1. Echo rate on this meeting: 45/51 call windows (0.88), trigram containment 0.89. Read AI separates 4 named people. Yap shows 2 labels, with 3 people under "Me".

**10c. Coverage**

Yap has the whole 31:04 on both tracks (62 and 51 windows, no gaps in t0; t1 skips 4 of the reference's 32 windows, which are local-only stretches). Read AI covers Yap 3:28 → ~29:20, plus its last turn. Its own local m4a is 1,407 s (23.5 min, stereo AAC). Yap holds about 3.5 min before Read AI joined and about 1.5 min after it left.

**10d. Read AI's report shape vs Yap.** Local data holds no report: the report is cloud-side. The shape below was inferred from `strings` on `/Applications/Read AI.app/Contents/MacOS/read_ai_desktop` (Rust + WebKit), the WebKit cache, and `app_settings.json` keys. "local evidence" = string counts seen; "public" = the known product, UNVERIFIED here.

| Read AI data point | evidence | in Yap today | backlog | worth it at $29 local-first |
|---|---|---|---|---|
| Summary / recap | "recap" ×7,037 in the WebKit cache, "summary" | y (notes Summary, NotesView) but verbatim | NT4, NEW notes-guards/-strategy | yes: the core job |
| Chapters / topics with times | "chapter" ×38, "topic" | n | not planned | yes: cheap, MAP chunks already map to time ranges |
| Action items with owners | public | y (Me/Them owners) | NT4, NT11 | yes |
| Key questions | public | y (Open questions) | — | yes |
| Decisions | "decision" ×26 | y | — | yes |
| Speaker names on transcript | `src/diarization/remote_speaker.rs`, `active_speaker_heuristics/odd_one_out.rs`, "no readable speaker grid" (reads the meeting app's active-speaker UI) | n (channels only) | NEW yap-audit-remote-speaker-names | yes, if done locally via Accessibility |
| Echo cancellation mic vs system | `aec3-0.1.8` crate paths | n | NT11 (appended) | **yes, highest leverage** |
| Clock drift control | `[DriftController] Hard reset: drift=` | y (YV107 rate measurement) | NT11 epochs | yes |
| Talk time % / speaker stats | "speaker_per_second" | n | not planned | yes once dedupe lands (segment seconds per track) |
| Pace (wpm) | "wpm" ×458, "pace" ×135 | n | not planned | no: vanity |
| Filler words | "filler" ×2 | n | not planned | no |
| Bias / charisma / sentiment / engagement | "bias" ×68, public | n | not planned | no: unreliable on 1.5B and creepy |
| Highlights / clips / playlists | "highlight" ×16, "clip" ×44, "playlist" ×38 | n | not planned | no (audio-only, 7-day retention) |
| Follow-up email | "followup" ×27 | n | not planned | yes as "Copy as email" from notes |
| Search Copilot ("what did I miss") | `SearchCopilotPage` asset | n (FTS search only) | not planned | later: needs a bigger model or BYOM |
| Translation | "translation" ×12 | n | not planned | no |
| Integrations (Jira, HubSpot…) | "integration" ×186, "jira", "hubspot" | n | yap-res-0 (destinations, files only) | files/Obsidian yes, SaaS no |
| Meeting detect / autostart / consent modal / report-ready notice | settings keys `meeting_detected_known_apps`, `show_recording_consent_modal`, `report_ready_notifications` | partial (pill) | yap-notetaker-detect, yap-call-0/1, NT7 | yes |
| Product telemetry | `mixpanel_enabled`, mixpanel_*.json, "mixpanel" ×217 | none | — | no: Yap's moat |

**10e. Head-to-head for this meeting**

| criterion | Read AI | Yap (#250) |
|---|---|---|
| Coverage | ~26 of 31 min | 31:04, both tracks |
| Transcript accuracy | reference | 0.20 mic-only. The rendered view is unusable (0.84) because of echo dups |
| Speakers | 4 named | 2 channel labels. 2 local voices plus echoes under "Me" |
| Summary | cloud only, not on disk: UNVERIFIED | 0.91 verbatim excerpt (grade 1) |
| Decisions completeness | UNVERIFIED | 0 of 6 listed are decisions. Misses the website-split decision (Yap ~7:15) |
| Actions + owners | UNVERIFIED | ~3 real commitments (assets, specs, demo; Yap ~27:30–29:10) all owned "Me" but spoken by Speaker D. Wilson's own "browser demo" commitment (~18:25) is not an action |
| Hallucination | UNVERIFIED | No invented names or numbers. Invented categories: narration as "decisions" |
| Readability | UNVERIFIED | Poor: 8 cross-section duplicates |
| Privacy | Audio uploaded, Mixpanel | Zero sockets sampled, no bot in the call |

Single biggest gap: **who said what**. Read AI cancels the echo (aec3) and names remote speakers from the meeting app's active-speaker UI. Yap duplicates the far side into "Me" and assigns everyone's commitments to Wilson.

The one thing Yap does better (true on this data): it captured the **whole meeting** (Read AI missed the first ~3.5 min and the last ~1.5 min), **locally, with no bot joining and no upload**. The mic track alone is within ~20% token distance of the cloud transcript.

## Panel revisions 2026-10-26 (panel 3)

Stamp: 2026-10-26T17:05:11Z. Senior Panel 3, two seats plus synthesis. Seat A: audio/DSP + macOS + CTO (built aec3 0.4.0 in scratch and measured it on the real stored WAVs, numbers only). Seat B: PM + The User + AI/models. Base: main 202fea8, PR 250 head 0a87249 (open, conflicting, titled "DRAFT: 11 tests red"), PR 246 open.
Overall verdict: **revise-first** (applied in scripts/loop/items; generated parts STALE until loop:build, which waits for the two live passes).

**Decisions.** (1) Echo: an AEC3 pre-pass on the mic track is the primary fix and yap24-NT11 is rescoped to it; the text dedupe survives only as a fallback (per-span Speaker label where the system track is silent or missing, plus a 3-consecutive-word residue rule run after AEC). yap24-NT11-0 is folded into NT11 and removed. (2) Notes strategy: items first, Summary composed last from the surviving items plus topic labels (arm E); arms A, B, E are measured and C (Qwen2.5-3B) is measured only; the BYOM arm is dropped. (3) Rewrite notes reuses retry_meeting_summary; there is no new Rust command. (4) Utterance segments are a merge of token-span pieces (gap under 1.2 s, piece at most 20 s, minimum 3 s). (5) Remote speaker names are deferred to the next release; the v1 alternative is the calendar 1:1 name inside yap24-NT4 (DO R5), because NT4 owns the notes owner text and the eval that gates it.

**Measured by seat A (aec3 0.4.0, real WAVs of c6187806, 16 kHz s16 mono, NS and AGC off).** Call-playing windows attenuated 25.5 dB at the median (p25 7.4 dB); local-only windows lose 0.17 dB (p90 0.20); lag 7.1 / 0.6 / -1.7 ms on the three real calls, constant, no drift; 12.1 s for 31 minutes on an M4 Pro (RTF 0.0065), about 20-25 s expected on M1; builds on arm64; MIT OR BSD-3-Clause; deps already in Cargo.lock, so one new crate. ASR-level echo_dup_rate after AEC is not yet measured: yap24-NT11 acceptance (a)-(e) decides it.

**Findings after dedupe: PLAN-CHANGE 4, ITEM-CHANGE 19, DEFER 5, REJECTED 4.**
- PLAN-CHANGE: NT11 becomes the AEC3 pre-pass (the v1 blocker for the first-session walk: every remote line appears twice until it lands); NT11-0 folded and removed; notes strategy E (items first, Summary last); PR 250 must not reach a release before guards 1-6 plus F2 and F3 land (comment posted on PR 250).
- ITEM-CHANGE, yap24-NT11: AEC pre-pass design, 30% coupling check, 20 ms reference shift, derived .t0.aec.wav purged by NT1, aec3 pinned "=0.4.0" with NOTICE and supply_chain.rs, acceptance (a)-(e) on real audio, the output_route grep deleted, preflight marker only on NT1's purge test, finalize keeps the index files, and a new DO 0: copy c6187806's WAVs into ~/yap-eval-corpus/private/ now (found by synthesis: the 7-day retention purges that audio about 2026-11-02 and it is the only real two-track evidence).
- ITEM-CHANGE, yap-audit-notes-guards: F2 (Key points never padded), F3 (echo owner via the shared word_trigram_containment), F4 (the no-summary sentence), F5 (headline_source prefix clamp), the mic-track owner rule ("Someone here" until NT5 supplies a voice count), and a repetition penalty for notes decoding only.
- ITEM-CHANGE, yap-audit-notes-strategy: arms A/B/E/C, hold until the eval, fixture, guards and AEC exist, a real cargo acceptance with YAP_EVAL_REQUIRE=1 and at least three numeric arm rows, honest fallback copy, arm C RSS ceiling 2.2 GB.
- ITEM-CHANGE, yap24-NT4: eval table (recall 0.8, precision 0.8, decision recall 0.5, verbatim 0.5, dups 0, items per segment 2, owner accuracy 0.8 or blank), the planted 15-minute fixture spec, a numbers-only human check, and R5 (calendar 1:1 name).
- ITEM-CHANGE, yap-audit-utterance-segments: merge rule and thresholds, median 3-12 s, MAP pass count at most 1.25x whole-window rows, chunks at most 1400 tokens with seg labels, no migration, a corpus-timings test, DEPENDENCY marker on summary_jobs.rs, depends on NT11 not on vad-boundaries.
- ITEM-CHANGE, yap-audit-rewrite-notes: change canRetryNotes (progress.ts), invoke retry_meeting_summary, assert no fn rewrite_notes, hold on the guards, exact copy "Rewriting. Your current notes stay until the new ones are ready.", cut the older-version hint, show the 1.1 GB line before the run.
- ITEM-CHANGE, yap-audit-eval-call-bleed: it owns meeting_echo::word_trigram_containment, and compare-reference.py gains --tracks and --notes.
- DEFER: yap-audit-remote-speaker-names (next release; GO bar and security note recorded in the item); NT5 on the call track for Them A / Them B (next release, after 100 users); two voices on one laptop mic (needs NT5, after 100 users); a headphones nudge as copy (next release); "Re-transcribe and rewrite" (lands with whichever of NT11 and rewrite-notes lands second).
- REJECTED: webrtc-audio-processing (second native C library, breaks the ledger rule); energy ducking (half-duplex deletes real double-talk); text dedupe as the primary fix (cannot fix mixed double-talk ASR, drops real words); the BYOM arm of the notes strategy (yap-byom-* owns BYOM).

**Duels, settled with evidence.** (a) Echo primary: seat A's AEC3 pre-pass beat the audit/NT11 text dedupe on measured acoustics. (b) NT11-0: both seats said fold, done. (c) Notes arms: both applied (B's arms, A's AEC-cleaned input and hold). (d) Rewrite: both applied. (e) Utterance segments: both applied. (f) Remote names: both said defer, done.

**Pre-mortems carried.** Explicit-only plants reach 0.9 recall while real meetings fail (implicit, distractor and end-of-meeting plants are in the NT4 gate). A rewrite run before the guards returns the same grade-1 notes (hold on VERBATIM_NARRATIVE_MAX). A silent 1.1 GB fetch (download line before the run). Residual echo in the p25 windows still reaching the ASR (NT11 acceptance (a) and (d) are the test).
