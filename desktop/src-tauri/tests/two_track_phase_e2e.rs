//! YV109 — **the yap22-B phase-closing end-to-end**, with no audio hardware,
//! no TCC grant, no ASR model and no eval corpus, so it runs on every commit.
//!
//! 22-A closed with `matrix_phase_offline` plus a camera demo
//! (`docs/yap22a-phase-demo.md`). 22-B's closing claim is a different shape:
//! *a synthetic two-source, deliberately clock-offset capture completes
//! start-to-finish — record both tracks → transcribe both → merge by host time
//! → render Me/Them — producing an ordered transcript with zero out-of-order
//! spans.* Every item before this one proved its own link; this is the only
//! test that walks the whole chain, which is the same closing-proof role YV99
//! played for 22-A.
//!
//! ## The chain, link by link, and which of them is REAL here
//!
//! | link | what runs | real? |
//! |---|---|---|
//! | capture | `MeetingCapture::with_tracks(…, 2, …)`, two block streams at two callback sizes with their own `host_ns` clocks | the shipped consumer |
//! | journal | `MeetingJournal::start_with_depth(dir, 2, …)` — two spills, two index sidecars, real writer thread | the shipped journal, on a deeper queue ([`E2E_QUEUE_DEPTH`]) |
//! | finalize | `MeetingJournal::finalize` → two wavs through the crash-recovery path | the shipped finalize |
//! | ASR | pre-baked `ChunkOutcome`s per track, merged by the shipped `merge_timed` | the shipped seam merge; the DECODER is stubbed (no model in CI) |
//! | host-time merge | `support::two_track::TrackTimeline`, fitted from the journal's OWN persisted index records | the harness's reference — see below |
//! | storage | `Database::append_meeting_segments` with `on_track`, real SQLite file | the shipped schema (migration 3) |
//! | render | `meetings::render_transcript` + `render_markdown` | the shipped renderer (YV108) |
//!
//! **The one link that is not the shipped function is the host-time merge, and
//! it is not shipped because it has not landed.** YV107 (PR #130 — per-track
//! true-rate logging and the `<50 ms` residual) is open at the time this file
//! was written; `main` carries no cross-track rebase at all. Rather than write
//! a second one in `src/`, this E2E rebases with the eval harness's independent
//! reference and says so out loud. When #130 merges, the rebase line here
//! becomes a call to `HostTimeline` and this file becomes the test that proves
//! the shipped merge agrees with an implementation that was written without
//! looking at it. Claiming the merge is wired today would be the false
//! capability claim; measuring the chain around it is the honest version.
//!
//! ## What makes it falsifiable rather than lucky
//!
//! The two tracks are given genuinely different clocks — a 0.75 s start offset
//! and 290 ppm of relative rate error — and the markers are placed so that FOUR
//! Me/Them pairs are less than 0.75 s apart in the order they were really
//! spoken. `without_the_rebase_four_pairs_render_in_the_wrong_order` runs the
//! identical chain with the rebase removed (the pre-22-B status quo: compare
//! two unrelated clocks directly) and asserts those four pairs come out
//! swapped. A gate whose negative control also passes is not a gate.

mod support;

use std::path::PathBuf;

use wilson_voice_lib::asr_engine::{TimedKind, TimedSpan};
use wilson_voice_lib::meeting::{
    IndexRecord, MeetingCapture, MeetingJournal, MeetingState, MIC_TRACK, SYSTEM_TRACK, TARGET_RATE,
};
use wilson_voice_lib::meeting_asr::{merge_timed, BoundaryKind, ChunkOutcome, ChunkStatus};
use wilson_voice_lib::meetings::{
    is_two_track, render_markdown, render_transcript, MeetingKind, MIC_SPEAKER_LABEL,
    SYSTEM_SPEAKER_LABEL, UNCLUSTERED_SPEAKER_LABEL,
};
use wilson_voice_lib::rtring::CaptureAnchor;

use support::two_track::{
    cross_track_residual_ms, marker_sequence, out_of_order, segments_from_host_spans,
    wait_for_index_records, HostSpan, TrackTimeline, RESIDUAL_BUDGET_MS, RESIDUAL_HORIZON_SECONDS,
};

// ---------------------------------------------------------------------------
// The synthetic two-source capture
// ---------------------------------------------------------------------------

/// Both devices deliver at 16 kHz here, which makes the resampler an identity
/// and keeps the finalized sample positions exactly the block stream — so any
/// timing error this test reports is the CLOCK's, never the resampler's.
const NATIVE_RATE: u32 = 16_000;

/// The mic's callback size (10 ms) and the tap's (20 ms). Different on purpose:
/// an index record's `host_ns` is the last drained anchor's capture timestamp
/// while its sample counters run through the END of that callback, so each
/// track's fitted origin carries a constant bias of exactly one callback — and
/// two DIFFERENT callback sizes make that bias differential, i.e. visible in
/// the cross-track residual instead of cancelling. That 10 ms is a real number
/// this test measures rather than a hypothetical.
const MIC_FRAMES: usize = 160;
const SYS_FRAMES: usize = 320;

/// The tap's aggregate device starts after the mic's stream does — the tap has
/// to create a process tap and an aggregate device first (YV100's call
/// sequence), and the mic is already running by then.
const SYSTEM_START_OFFSET_SECONDS: f64 = 0.750;

/// Each device's true rate, as a fraction off nominal. A built-in mic and a
/// Bluetooth output device do not share a crystal (OS-2); these are the sign
/// and scale of the mismatch that phase deferred the shared-aggregate
/// drift-compensation approach behind a measurement gate.
const MIC_PPM: f64 = -40e-6;
const SYSTEM_PPM: f64 = 250e-6;

const MEETING_SECONDS: f64 = 45.0;

/// `(host_seconds, track, marker word)` — the conversation as it was really
/// spoken, on the one clock both devices could see.
///
/// Four Me/Them pairs sit closer together than [`SYSTEM_START_OFFSET_SECONDS`]
/// with **Me** first, which is what makes the un-rebased control fail: dropping
/// the rebase slides every "Them" 0.75 s early and swaps exactly those four.
const CONVERSATION: [(f64, i64, &str); 13] = [
    (2.0, MIC_TRACK as i64, "avocado"),
    (5.0, SYSTEM_TRACK as i64, "bramble"),
    (8.0, MIC_TRACK as i64, "kettle"),
    (8.4, SYSTEM_TRACK as i64, "custard"),
    (12.0, SYSTEM_TRACK as i64, "harpoon"),
    (15.0, MIC_TRACK as i64, "marigold"),
    (20.0, MIC_TRACK as i64, "penguin"),
    (20.5, SYSTEM_TRACK as i64, "meadow"),
    (28.6, MIC_TRACK as i64, "cobalt"),
    (30.0, MIC_TRACK as i64, "sandal"),
    (30.3, SYSTEM_TRACK as i64, "quilt"),
    (34.0, MIC_TRACK as i64, "violin"),
    (34.6, SYSTEM_TRACK as i64, "turnip"),
];

fn markers() -> Vec<String> {
    CONVERSATION
        .iter()
        .map(|(_, _, w)| (*w).to_string())
        .collect()
}

/// The order the rendered transcript must come out in, as `Me:word` /
/// `Them:word`. Derived from [`CONVERSATION`] by sorting on the shared clock —
/// never typed twice, so the ground truth and the fixture cannot disagree.
fn expected_sequence() -> Vec<String> {
    let mut rows: Vec<(f64, i64, &str)> = CONVERSATION.to_vec();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    rows.iter()
        .map(|(_, track, word)| {
            let speaker = if *track == MIC_TRACK as i64 {
                MIC_SPEAKER_LABEL
            } else {
                SYSTEM_SPEAKER_LABEL
            };
            format!("{speaker}:{word}")
        })
        .collect()
}

fn truth(track: usize) -> TrackTimeline {
    match track {
        MIC_TRACK => TrackTimeline::exact(0.0, TARGET_RATE as f64 * (1.0 + MIC_PPM)),
        _ => TrackTimeline::exact(
            SYSTEM_START_OFFSET_SECONDS,
            TARGET_RATE as f64 * (1.0 + SYSTEM_PPM),
        ),
    }
}

/// A host second, as this track's own finalized-wav second. The inverse of
/// [`TrackTimeline::host_seconds`], used only to BUILD the fixture: the words
/// are declared on the shared clock and then placed where each device's clock
/// would have recorded them.
fn local_seconds(track: usize, host_seconds: f64) -> f64 {
    let (origin, ppm) = match track {
        MIC_TRACK => (0.0, MIC_PPM),
        _ => (SYSTEM_START_OFFSET_SECONDS, SYSTEM_PPM),
    };
    (host_seconds - origin) * (1.0 + ppm)
}

fn tmpdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "yap-yv109-e2e-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// One callback of audible tone plus the anchor a real capture callback would
/// have stamped on it.
///
/// Tone, never a constant: the capture path high-passes, so DC decays to zero
/// and "audio" becomes indistinguishable from spliced silence — the same trap
/// `two_track_journal_round_trip` documents.
fn block(
    track: usize,
    k: u64,
    frames: usize,
    ppm: f64,
    origin_seconds: f64,
) -> (Vec<f32>, [CaptureAnchor; 1]) {
    let start = k as usize * frames;
    let phase = if track == MIC_TRACK { 0.05 } else { 0.31 };
    let samples: Vec<f32> = (0..frames)
        .map(|i| (((start + i) as f32) * phase).sin() * 0.6)
        .collect();
    // The device's OWN clock: `frames` frames take `frames / (rate * (1+ppm))`
    // host seconds, which is the whole mismatch this phase exists to correct.
    let host_seconds = origin_seconds + (start as f64) / (NATIVE_RATE as f64 * (1.0 + ppm));
    (
        samples,
        [CaptureAnchor {
            host_ns: (host_seconds * 1_000_000_000.0) as u64,
            sample_index: start as u64,
            frames: frames as u32,
            sample_rate: NATIVE_RATE,
            lost_frames: 0,
        }],
    )
}

/// Record both tracks into one real journal and hand back the persisted index
/// records plus the finalized wavs.
struct Recorded {
    dir: PathBuf,
    records: Vec<Vec<IndexRecord>>,
    wavs: Vec<Option<PathBuf>>,
    spliced_silence_samples: u64,
}

/// The journal queue depth this test opens with, and why it is not the shipped
/// [`MEETING_QUEUE_DEPTH`] (512).
///
/// The shipped depth is sized for a REAL-TIME producer: a callback every 10 ms,
/// a writer thread flushing every 250 ms, and a hard bound on memory so a disk
/// hiccup cannot grow a three-hour meeting in RAM (finding #1). This test is not
/// that producer. It pushes 45 seconds of audio in about 0.3 seconds — roughly
/// 150x real time, 6 609 blocks with no pause between them — so the bound is
/// reached by the FEEDER outrunning the disk, which is a property of how fast
/// the machine under it happens to be.
///
/// That is not a hypothetical. The first cut of this file used the shipped
/// depth, passed on a quiet developer machine, and failed in CI with
/// `spliced_silence_samples = 30080` — 1.88 s of audio the queue refused.
/// Reproduced locally afterwards by running twelve copies of the test at once:
/// 12 of 12 failed, losing between 176 160 and 1 263 838 samples. A test whose
/// result depends on how loaded the machine is is worse than no test.
///
/// So the queue is opened deep enough that it cannot be the limit, and the
/// backpressure behaviour keeps its own test — `matrix_row5_journal_backpressure`
/// is where a full queue is the subject rather than the weather.
const E2E_QUEUE_DEPTH: usize = 32_768;

fn record_two_tracks(tag: &str, tracks: usize) -> Recorded {
    let dir = tmpdir(tag);
    let journal = MeetingJournal::start_with_depth(&dir, tracks, E2E_QUEUE_DEPTH).expect("journal");
    let id = journal.id().to_string();
    let capture = MeetingCapture::with_tracks(NATIVE_RATE, 1, tracks, Some(journal));

    let mic_blocks = (MEETING_SECONDS * NATIVE_RATE as f64) as usize / MIC_FRAMES;
    let sys_blocks = ((MEETING_SECONDS - SYSTEM_START_OFFSET_SECONDS) * NATIVE_RATE as f64)
        as usize
        / SYS_FRAMES;
    for k in 0..mic_blocks as u64 {
        let (audio, anchors) = block(MIC_TRACK, k, MIC_FRAMES, MIC_PPM, 0.0);
        capture.accept_track(MIC_TRACK, &audio, &anchors);
    }
    if tracks > SYSTEM_TRACK {
        for k in 0..sys_blocks as u64 {
            let (audio, anchors) = block(
                SYSTEM_TRACK,
                k,
                SYS_FRAMES,
                SYSTEM_PPM,
                SYSTEM_START_OFFSET_SECONDS,
            );
            capture.accept_track(SYSTEM_TRACK, &audio, &anchors);
        }
    }

    // The index sidecars are flushed by the writer thread on its own timer and
    // then REMOVED by finalize, so they are read here, in between, and waited
    // for rather than slept on.
    let expected = MEETING_SECONDS as usize - 2;
    let records = wait_for_index_records(&dir, &id, tracks, expected);

    let journal = capture.close().expect("the journal comes back");
    let finalized = journal.finalize(MeetingState::Complete).expect("finalized");
    let wavs: Vec<Option<PathBuf>> = (0..tracks)
        .map(|t| finalized.wav_for_track(t).cloned())
        .collect();
    Recorded {
        dir,
        records,
        wavs,
        spliced_silence_samples: finalized.spliced_silence_samples,
    }
}

// ---------------------------------------------------------------------------
// The stubbed decoder — real seam merge, no model
// ---------------------------------------------------------------------------

/// The shipped chunk geometry: 30 s windows decoding from 2 s before their own
/// boundary. Mirrored (not re-derived) so the pre-baked windows are the ones
/// `plan_windows` would have produced; `meeting_eval.rs` asserts the mirror
/// against `ChunkConfig::default` on every run.
const CHUNK_SECONDS: f64 = 30.0;
const CHUNK_OVERLAP_SECONDS: f64 = 2.0;

/// One track's decode, as the two `ChunkOutcome`s a 45 s meeting produces.
///
/// The words are pre-baked instead of decoded because CI has no ASR model —
/// but the SEAM MERGE is the shipped one, and the words are placed so it has
/// real work to do: `cobalt` sits at 28.6 s, inside the `[28, 30]` region both
/// windows decode, so it arrives twice and must come out once.
fn decode_track(track: usize) -> Vec<TimedSpan> {
    let mut first: Vec<(f64, f64, String)> = Vec::new();
    let mut second: Vec<(f64, f64, String)> = Vec::new();
    for (host, on_track, word) in CONVERSATION {
        if on_track as usize != track {
            continue;
        }
        let at = local_seconds(track, host);
        let span = (at, at + 0.45, (word).to_string());
        if at < CHUNK_SECONDS {
            // Everything the first window decoded — which includes the overlap
            // the second window also sees.
            first.push(span.clone());
        }
        if at >= CHUNK_SECONDS - CHUNK_OVERLAP_SECONDS {
            second.push(span);
        }
    }
    let chunks = vec![
        chunk(0, 0.0, CHUNK_SECONDS, &first),
        chunk(1, CHUNK_SECONDS, MEETING_SECONDS, &second),
    ];
    merge_timed(&chunks)
}

fn chunk(index: usize, start: f64, end: f64, spans: &[(f64, f64, String)]) -> ChunkOutcome {
    ChunkOutcome {
        index,
        audio_start_seconds: (start - CHUNK_OVERLAP_SECONDS).max(0.0),
        content_start_seconds: start,
        content_end_seconds: end,
        start_boundary: BoundaryKind::Silence,
        end_boundary: BoundaryKind::Silence,
        status: ChunkStatus::Done,
        text: spans
            .iter()
            .map(|(_, _, t)| t.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        spans: spans
            .iter()
            .map(|(a, b, t)| TimedSpan {
                start_seconds: *a,
                end_seconds: *b,
                text: t.clone(),
            })
            .collect(),
        timestamp_kind: TimedKind::Word,
        error: None,
    }
}

/// The rebase step: one track's local spans, on the shared clock.
fn onto_host_clock(track: usize, spans: &[TimedSpan], timeline: &TrackTimeline) -> Vec<HostSpan> {
    spans
        .iter()
        .map(|s| HostSpan {
            track: track as i64,
            host_start_seconds: timeline.host_seconds(s.start_seconds),
            host_end_seconds: timeline.host_seconds(s.end_seconds),
            text: s.text.clone(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The phase-closing E2E
// ---------------------------------------------------------------------------

#[test]
fn a_two_source_clock_offset_meeting_renders_one_ordered_me_them_transcript() {
    let _lock = support::exclusive();
    let recorded = record_two_tracks("phase", 2);
    assert_eq!(
        recorded.spliced_silence_samples, 0,
        "a clean synthetic capture must lose nothing — a lossy track is YV107's \
         finalized-position problem and this fixture deliberately has none"
    );
    for track in [MIC_TRACK, SYSTEM_TRACK] {
        let wav = recorded.wavs[track]
            .as_ref()
            .unwrap_or_else(|| panic!("track {track} finalized into a wav"));
        assert!(wav.exists(), "track {track}'s wav is on disk");
    }

    // ── merge by host time, from the journal's OWN persisted records ─────────
    let timelines: Vec<TrackTimeline> = recorded
        .records
        .iter()
        .map(|r| TrackTimeline::from_records(r))
        .collect();
    for (track, timeline) in timelines.iter().enumerate() {
        let ppm = (timeline.measured_rate() / TARGET_RATE as f64 - 1.0) * 1e6;
        eprintln!(
            "  track {track}: {} records, measured rate {:.4} Hz ({ppm:+.1} ppm), \
             origin {:+.4} s",
            recorded.records[track].len(),
            timeline.measured_rate(),
            timeline.origin_seconds(),
        );
    }

    let residual_mic =
        timelines[MIC_TRACK].residual_ms_at(&truth(MIC_TRACK), RESIDUAL_HORIZON_SECONDS);
    let residual_sys =
        timelines[SYSTEM_TRACK].residual_ms_at(&truth(SYSTEM_TRACK), RESIDUAL_HORIZON_SECONDS);
    let residual_cross = cross_track_residual_ms(
        (&timelines[MIC_TRACK], &truth(MIC_TRACK)),
        (&timelines[SYSTEM_TRACK], &truth(SYSTEM_TRACK)),
        RESIDUAL_HORIZON_SECONDS,
    );
    eprintln!(
        "  residual at the {:.0} h mark: mic {residual_mic:.1} ms, system \
         {residual_sys:.1} ms, CROSS-TRACK {residual_cross:.1} ms (budget \
         {RESIDUAL_BUDGET_MS:.0} ms)",
        RESIDUAL_HORIZON_SECONDS / 3600.0
    );
    assert!(
        residual_cross <= RESIDUAL_BUDGET_MS,
        "Me and Them slide {residual_cross:.1} ms apart by three hours, past the \
         {RESIDUAL_BUDGET_MS:.0} ms budget"
    );

    // ── the chain: decode → rebase → store → render ──────────────────────────
    let mut spans: Vec<HostSpan> = Vec::new();
    for track in [MIC_TRACK, SYSTEM_TRACK] {
        spans.extend(onto_host_clock(
            track,
            &decode_track(track),
            &timelines[track],
        ));
    }
    // Stored the way the pipeline stores them: one transcription pass per wav,
    // so the tap's rows carry a LATER `created_at` than mic rows spoken after
    // them. A transcript that comes back ordered came back that way because of
    // the host clock, not because of insert order.
    spans.sort_by_key(|s| s.track);

    let dir = support::temp_dir("yv109-e2e");
    let db = support::open_db(&dir);
    let meeting = db
        .create_meeting_with_kind("Two-source phase demo", "manual", MeetingKind::Virtual)
        .expect("create meeting");
    let rows = segments_from_host_spans(&meeting.id, &spans);
    for track in [MIC_TRACK as i64, SYSTEM_TRACK as i64] {
        let batch: Vec<_> = rows
            .iter()
            .filter(|r| r.track == track)
            .map(|r| {
                wilson_voice_lib::meetings::NewMeetingSegment::new(
                    r.start_seconds,
                    r.end_seconds,
                    r.text.clone(),
                )
                .on_track(track)
            })
            .collect();
        db.append_meeting_segments(&meeting.id, &batch)
            .expect("append segments");
    }
    let stored = db
        .list_meeting_segments(&meeting.id)
        .expect("list segments");
    assert_eq!(
        stored.len(),
        CONVERSATION.len(),
        "every word survived the round trip, and `cobalt` survived exactly once"
    );
    assert!(
        is_two_track(&stored),
        "this meeting has a real second track"
    );
    // YV125 — read back from the ROW rather than from the local variable, so
    // the kind the recording was started under is proved to have survived the
    // round trip that the transcript is then rendered under.
    let meeting_kind = db
        .get_meeting(&meeting.id)
        .expect("get")
        .expect("exists")
        .kind();
    assert_eq!(
        meeting_kind,
        MeetingKind::Virtual,
        "a call with a live second track is the one configuration whose mic \
         track is a single speaker"
    );

    // Rendered TWICE, from two different orderings of the same rows, because
    // they answer different questions. `list_meeting_segments` sorts in SQL
    // (`ORDER BY start_seconds, track, created_at`), so rendering its output
    // would be green even if the renderer's own comparator were deleted — and
    // the renderer is what the React mirror and the Markdown export both go
    // through. So the same rows are also rendered in INSERT order, one whole
    // track after the other, which is the shape the renderer has to fix itself.
    let unsorted = render_transcript(&rows, MeetingKind::Virtual);
    assert_eq!(
        marker_sequence(&unsorted, &markers()),
        expected_sequence(),
        "the renderer did not order the two tracks itself"
    );

    let lines = render_transcript(&stored, meeting_kind);
    // Row ids differ (SQLite minted its own), so the comparison is on what a
    // reader sees: who, when, what.
    let shape = |ls: &[wilson_voice_lib::meetings::TranscriptLine]| -> Vec<(i64, String, String)> {
        ls.iter()
            .map(|l| (l.track, l.offset.clone(), l.text.clone()))
            .collect()
    };
    assert_eq!(
        shape(&lines),
        shape(&unsorted),
        "the two orderings of the same rows must render identically — SQL and \
         the renderer disagreeing is how a screen and an export drift apart"
    );
    let starts: Vec<f64> = lines.iter().map(|l| l.start_seconds).collect();
    assert!(
        out_of_order(&starts).is_empty(),
        "the rendered transcript goes backwards in time: {:?}",
        out_of_order(&starts)
    );
    assert_eq!(
        marker_sequence(&lines, &markers()),
        expected_sequence(),
        "the merged transcript is not the conversation that was spoken"
    );

    // …and the file somebody sends round says the same thing the screen did.
    let meeting = db.get_meeting(&meeting.id).expect("get").expect("exists");
    let markdown = render_markdown(&meeting, &stored);
    // The export's line shape is `**[hh:mm:ss] Speaker:** text` (YV94's stamp,
    // YV108's label), so the label is matched with its punctuation rather than
    // as a bare word — a substring that also matches ordinary transcript text
    // would make this assertion pass on an export with no labels in it at all.
    assert!(
        markdown.contains(&format!("] {MIC_SPEAKER_LABEL}:**")),
        "{markdown}"
    );
    assert!(
        markdown.contains(&format!("] {SYSTEM_SPEAKER_LABEL}:**")),
        "{markdown}"
    );
    let me_kettle = markdown.find("kettle").expect("kettle is in the export");
    let them_custard = markdown.find("custard").expect("custard is in the export");
    assert!(
        me_kettle < them_custard,
        "the export reordered a pair the screen got right"
    );

    let _ = std::fs::remove_dir_all(&recorded.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

/// **The negative control.** The same chain, with the one step this phase
/// added removed: compare the two tracks' own local clocks directly, which is
/// what any code before 22-B could have done.
#[test]
fn without_the_rebase_four_pairs_render_in_the_wrong_order() {
    let _lock = support::exclusive();
    let mut spans: Vec<HostSpan> = Vec::new();
    for track in [MIC_TRACK, SYSTEM_TRACK] {
        // The identity "timeline": local seconds, taken at face value.
        let naive = TrackTimeline::exact(0.0, TARGET_RATE as f64);
        spans.extend(onto_host_clock(track, &decode_track(track), &naive));
    }
    let rows = segments_from_host_spans("no-rebase", &spans);
    let lines = render_transcript(&rows, MeetingKind::Virtual);
    let got = marker_sequence(&lines, &markers());
    let want = expected_sequence();
    assert_eq!(
        got.len(),
        want.len(),
        "the same words, in a different order"
    );

    let swapped: Vec<usize> = got
        .iter()
        .zip(want.iter())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        swapped.len(),
        8,
        "four swapped PAIRS is eight displaced rows; got {swapped:?}\n  \
         rendered: {got:?}\n  spoken:   {want:?}"
    );
}

/// The other half of every 22-B claim: a 22-A meeting is untouched. One track,
/// the same chain, and nothing about it acquires a second speaker.
#[test]
fn a_mic_only_meeting_walks_the_same_chain_and_never_grows_a_them() {
    let _lock = support::exclusive();
    let recorded = record_two_tracks("mic-only", 1);
    assert_eq!(recorded.records.len(), 1, "one track, one index sidecar");
    assert!(recorded.wavs[0].is_some(), "the mic track finalized");

    let timeline = TrackTimeline::from_records(&recorded.records[MIC_TRACK]);
    let spans = onto_host_clock(MIC_TRACK, &decode_track(MIC_TRACK), &timeline);
    let rows = segments_from_host_spans("mic-only", &spans);
    assert!(
        !is_two_track(&rows),
        "a mic-only meeting must not claim a second track"
    );
    // Rendered as the CALL it was started as, which is the strong version of
    // the claim: even under `Virtual`, a meeting whose second track never
    // arrived falls back to clustering Track A (YV125), so every line is the
    // one unnamed speaker and none of them is a phantom "Them".
    let lines = render_transcript(&rows, MeetingKind::Virtual);
    assert!(
        lines.iter().all(|l| l.speaker == UNCLUSTERED_SPEAKER_LABEL),
        "a phantom Them appeared in a one-track meeting"
    );
    assert!(
        lines.iter().all(|l| l.speaker != MIC_SPEAKER_LABEL),
        "and with no second track carrying the other people, the microphone \
         cannot claim to be only this user"
    );
    let starts: Vec<f64> = lines.iter().map(|l| l.start_seconds).collect();
    assert!(out_of_order(&starts).is_empty());

    let _ = std::fs::remove_dir_all(&recorded.dir);
}

/// The rate half of the claim, stated as its own failure: assuming both devices
/// ran at exactly 16 kHz — which is what the wav headers say and what nothing
/// before this phase questioned — misses the budget by two orders of magnitude.
///
/// Pure, and it uses the SAME record sequences the E2E above rebased from, so
/// it cannot pass against a hand-built input the real chain never produces.
#[test]
fn the_nominal_rate_assumption_misses_the_budget_by_two_orders_of_magnitude() {
    let _lock = support::exclusive();
    let recorded = record_two_tracks("nominal", 2);
    let nominal: Vec<TrackTimeline> = recorded
        .records
        .iter()
        .map(|r| TrackTimeline::nominal_rate(r))
        .collect();
    let residual = cross_track_residual_ms(
        (&nominal[MIC_TRACK], &truth(MIC_TRACK)),
        (&nominal[SYSTEM_TRACK], &truth(SYSTEM_TRACK)),
        RESIDUAL_HORIZON_SECONDS,
    );
    eprintln!("  nominal-rate cross-track residual at 3 h: {residual:.0} ms");
    assert!(
        residual > 20.0 * RESIDUAL_BUDGET_MS,
        "the nominal-rate assumption was supposed to be badly wrong; it drifted \
         only {residual:.1} ms, so this fixture's clock mismatch is too small to \
         make the measured gate mean anything"
    );
    let _ = std::fs::remove_dir_all(&recorded.dir);
}
