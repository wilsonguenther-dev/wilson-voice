//! YV107 / OS-2 acceptance — **the plan's own stated line**: "a synthetic
//! two-track capture with clocks deliberately offset 100 ppm produces a
//! correctly-ordered merged transcript and <50 ms residual offset at 3 hours."
//!
//! Three hours are SIMULATED, not waited for: the whole defect lives in a pair
//! of `IndexRecord` sequences, and 10,801 records per track is what three hours
//! of a one-per-second index cadence actually looks like on disk. Nothing here
//! touches audio hardware, which is the point — OS-2's evidence is that this
//! defect is invisible on a desk test (built-in mic and built-in speakers on
//! Apple Silicon are ONE clock domain, drift ≈ 0) and appears only in the
//! configuration people really use.
//!
//! **The test is built to be hard to pass by accident.** Every assertion about
//! the host-time merge is paired with the same assertion against the NAIVE
//! merge — the `samples ÷ nominal_rate` timeline the code had before this item —
//! and the naive one is asserted to FAIL. If someone reverts the merge to
//! sample-derived times, the naive assertions stop failing and this file goes
//! red twice over.

#[path = "support/two_track.rs"]
mod two_track;

use two_track::{
    captured_at, chunk, index_records, local_seconds, turns, word, worst_residual_ms, FIXTURE_PPM,
    RESIDUAL_BUDGET_SECONDS, THREE_HOURS,
};
use wilson_voice_lib::meeting::{MIC_TRACK, SYSTEM_TRACK};
use wilson_voice_lib::meeting_asr::{
    merge_timed, merge_two_tracks_by_host_time, BoundaryKind, MergedSpan, TrackEpochs,
};

/// The mic. Its device is the reference clock in this fixture, so the whole
/// relative offset lives on the tap — which is the honest shape: what a merge
/// can recover is the RELATIVE skew, and putting it all on one side makes the
/// expected numbers readable.
const MIC_PPM: f64 = 0.0;
/// The tap, 100 ppm fast.
const TAP_PPM: f64 = FIXTURE_PPM;

/// Four question-and-answer pairs spread across the three hours, the far side
/// first each time and the near side half a second later. `(real_second,
/// question_text, answer_text)`.
///
/// Half a second is not an arbitrary gap: it is comfortably longer than a
/// conversational turn boundary and comfortably SHORTER than the 1.08 s of skew
/// 100 ppm reaches by the cap, which is exactly the regime OS-2 describes —
/// "at 200 ms the interleave begins placing an answer before its question; at
/// 400 ms it is systematically wrong in the back half of every long meeting."
/// Deliberately not whole seconds and deliberately not multiples of ten. A turn
/// on a round second lands exactly on an index record's own sample count, so the
/// interpolation between records is never exercised and the residual comes back
/// a suspiciously perfect 0.000 ms — a fixture that measures the map's endpoints
/// instead of the map.
const TURNS: [(f64, &str, &str); 4] = [
    (61.3, "question-one", "answer-one"),
    (3607.77, "question-two", "answer-two"),
    (7213.37, "question-three", "answer-three"),
    (10790.63, "question-four", "answer-four"),
];

const ANSWER_DELAY: f64 = 0.5;

fn tap_chunks() -> Vec<wilson_voice_lib::meeting_asr::ChunkOutcome> {
    split_in_two(
        TURNS
            .iter()
            .map(|(t, q, _)| word(TAP_PPM, *t, 0.35, q))
            .collect(),
        TAP_PPM,
    )
}

fn mic_chunks() -> Vec<wilson_voice_lib::meeting_asr::ChunkOutcome> {
    split_in_two(
        TURNS
            .iter()
            .map(|(t, _, a)| word(MIC_PPM, *t + ANSWER_DELAY, 0.35, a))
            .collect(),
        MIC_PPM,
    )
}

/// Two chunks per track, cut at the 90-minute mark on that track's OWN
/// timeline. Two rather than one so the per-track seam merge is genuinely
/// exercised before the cross-track interleave, and cut on the track's own
/// clock because that is where a real chunker cuts.
fn split_in_two(
    spans: Vec<wilson_voice_lib::asr_engine::TimedSpan>,
    ppm: f64,
) -> Vec<wilson_voice_lib::meeting_asr::ChunkOutcome> {
    let cut = local_seconds(ppm, THREE_HOURS / 2.0);
    let end = local_seconds(ppm, THREE_HOURS + 60.0);
    let (early, late): (Vec<_>, Vec<_>) = spans
        .into_iter()
        .partition(|s| (s.start_seconds + s.end_seconds) / 2.0 < cut);
    vec![
        chunk(0, 0.0, cut, BoundaryKind::Edge, early),
        chunk(1, cut, end, BoundaryKind::Silence, late),
    ]
}

/// What the transcript MUST read, in real time: every question before its
/// answer, for all three hours.
fn expected_turns() -> Vec<(String, String)> {
    let mut want = Vec::new();
    for (_, q, a) in TURNS {
        want.push(("Them".to_string(), q.to_string()));
        want.push(("Me".to_string(), a.to_string()));
    }
    want
}

/// Every span's truth: the real second it was actually spoken at.
fn truth() -> Vec<(&'static str, f64)> {
    let mut out = Vec::new();
    for (t, q, a) in TURNS {
        out.push((q, t));
        out.push((a, t + ANSWER_DELAY));
    }
    out
}

/// The merge this item deletes: interleave the two tracks by their OWN seconds,
/// which is `samples ÷ nominal_rate` and therefore wrong by exactly the ppm
/// figure. Reproduced here so the fixture's difficulty is measured rather than
/// asserted.
fn naive_merge() -> Vec<MergedSpan> {
    let mut out: Vec<MergedSpan> = Vec::new();
    for (track, speaker, chunks) in [
        (MIC_TRACK, "Me", mic_chunks()),
        (SYSTEM_TRACK, "Them", tap_chunks()),
    ] {
        for span in merge_timed(&chunks) {
            out.push(MergedSpan {
                track,
                speaker: speaker.to_string(),
                start_seconds: span.start_seconds,
                end_seconds: span.end_seconds,
                text: span.text,
            });
        }
    }
    out.sort_by(|a, b| a.start_seconds.total_cmp(&b.start_seconds));
    out
}

#[test]
fn a_hundred_ppm_offset_merges_in_the_right_order_and_under_fifty_ms_at_three_hours() {
    let mic = mic_chunks();
    let tap = tap_chunks();
    let anchors_mic = index_records(MIC_PPM, THREE_HOURS as u64);
    let anchors_tap = index_records(TAP_PPM, THREE_HOURS as u64);

    // Both streams' first callbacks treated as the same instant: this fixture's
    // whole subject is the RATE offset, and adding an epoch offset would test
    // the caller's bookkeeping instead.
    let merged = merge_two_tracks_by_host_time(
        &mic,
        &tap,
        &anchors_mic,
        &anchors_tap,
        TrackEpochs::SHARED,
        wilson_voice_lib::meetings::MeetingKind::Virtual,
    );

    assert_eq!(
        turns(&merged),
        expected_turns(),
        "every question comes before its answer, for all three hours"
    );

    let residual = worst_residual_ms(&merged, &truth());
    println!(
        "YV107 residual at the simulated 3-hour mark: {residual:.3} ms \
         (budget {:.0} ms)",
        RESIDUAL_BUDGET_SECONDS * 1000.0
    );
    assert!(
        residual <= RESIDUAL_BUDGET_SECONDS * 1000.0,
        "residual {residual:.3} ms exceeds the {:.0} ms the plan's line allows",
        RESIDUAL_BUDGET_SECONDS * 1000.0
    );

    // Strictly ordered, not merely mostly.
    assert!(
        merged
            .windows(2)
            .all(|w| w[0].start_seconds <= w[1].start_seconds),
        "the merged timeline is monotonic"
    );
}

/// Non-vacuity, in the form the eval harness would want it: the fixture is hard.
///
/// If this ever passes, the fixture stopped containing the defect and the test
/// above stopped proving anything.
#[test]
fn the_sample_derived_merge_this_item_replaces_fails_the_same_fixture() {
    let naive = naive_merge();

    assert_ne!(
        turns(&naive),
        expected_turns(),
        "the fixture must actually contain OS-2's defect — a naive merge that \
         gets the order right means the clocks were not really offset"
    );

    let residual = worst_residual_ms(&naive, &truth());
    println!("naive (samples ÷ nominal_rate) residual: {residual:.1} ms");
    assert!(
        residual > 1000.0,
        "100 ppm over three hours is ≈1.08 s of skew; the naive merge measured \
         only {residual:.1} ms, so the fixture is not the one OS-2 describes"
    );

    // And name the specific corruption, so the failure message is a
    // description of the bug rather than a diff of two long lists: by the back
    // half of the meeting the answer precedes its question.
    let order: Vec<&str> = naive.iter().map(|s| s.text.as_str()).collect();
    let q = order.iter().position(|t| *t == "question-four").unwrap();
    let a = order.iter().position(|t| *t == "answer-four").unwrap();
    assert!(
        a < q,
        "the naive merge is supposed to place the last answer BEFORE its \
         question; it did not, so the fixture's drift is too small to matter"
    );
}

/// The arithmetic the fixture rests on, asserted directly, so a later reader
/// does not have to trust the helper.
#[test]
fn the_fixture_drifts_by_the_amount_os2_computes() {
    // 100 ppm × 3 h = 1.08 s.
    let skew = local_seconds(TAP_PPM, THREE_HOURS) - local_seconds(MIC_PPM, THREE_HOURS);
    assert!(
        (skew - 1.08).abs() < 1e-6,
        "expected ≈1.08 s of skew at the cap, got {skew}"
    );
    // And the index records carry the SAME model, or the merge would be
    // correcting a drift the spans do not have.
    let records = index_records(TAP_PPM, THREE_HOURS as u64);
    let last = records.last().unwrap();
    assert_eq!(last.host_ns, THREE_HOURS as u64 * 1_000_000_000);
    assert_eq!(
        last.captured_samples,
        captured_at(TAP_PPM, THREE_HOURS).round() as u64
    );
}
