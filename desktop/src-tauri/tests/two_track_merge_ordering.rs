//! YV107 acceptance — "interleaved spans from both tracks come out strictly
//! ordered by host time **even when the two tracks' own internal seam-merge
//! boundaries do not align**."
//!
//! That clause is the interesting one. The two tracks are chunked by two
//! independent chunkers looking at two different audio streams, so their window
//! boundaries have no reason to line up and every reason not to: the mic is cut
//! where the person holding the Mac pauses, the tap where the call does. A merge
//! that walked the two chunk lists in step — or that assumed chunk `i` of one
//! track covers the same wall-clock interval as chunk `i` of the other — would
//! pass a fixture with aligned boundaries and corrupt every real meeting.
//!
//! So this file deliberately misaligns them: track 0 is cut on a fixed 30-second
//! clock, track 1 on 47-second silences, and neither cut lands anywhere near the
//! other. It also plants a real seam duplicate on track 0, because the ordering
//! claim is worthless if the cross-track interleave quietly skipped the
//! per-track seam dedupe on its way through.

#[path = "support/two_track.rs"]
mod two_track;

use two_track::{
    chunk, index_records, index_records_lossy, index_records_rebased, index_records_stalled,
    local_seconds, local_seconds_stalled, turns, word, RESIDUAL_BUDGET_SECONDS,
};
use wilson_voice_lib::asr_engine::TimedSpan;
use wilson_voice_lib::meeting::{plan_silence_splices, IndexRecord};
use wilson_voice_lib::meeting_asr::{
    merge_two_tracks_by_host_time, BoundaryKind, ChunkOutcome, TrackEpochs,
};

const MIC_PPM: f64 = 0.0;
const TAP_PPM: f64 = 100.0;

/// Alternating turns every three seconds for a minute and a half: the far side
/// on the even beats, the near side on the odd ones.
const BEAT: f64 = 3.0;
const BEATS: usize = 28;

fn beat_time(i: usize) -> f64 {
    2.0 + i as f64 * BEAT
}

/// Track 0 — the mic, cut on a FIXED 30-second clock (the boundary kind a
/// chunker falls back to when no pause presents itself).
fn mic_chunks() -> Vec<ChunkOutcome> {
    let mut spans: Vec<TimedSpan> = (0..BEATS)
        .filter(|i| i % 2 == 1)
        .map(|i| word(MIC_PPM, beat_time(i), 0.4, &format!("me-{i}")))
        .collect();
    // A REAL seam duplicate across the 30 s cut: the outgoing window's audio
    // stops at the boundary so it emits the word truncated there, and the
    // incoming window re-sees the overlap and emits the whole word. One word,
    // two windows. `merge_timed` must still delete exactly one of them, and the
    // cross-track interleave must not have skipped that step.
    spans.push(TimedSpan {
        start_seconds: 29.6,
        end_seconds: 30.0,
        text: "boundary".into(),
    });
    let seam_repeat = TimedSpan {
        start_seconds: 29.98,
        end_seconds: 30.42,
        text: "boundary".into(),
    };
    spans.sort_by(|a, b| a.start_seconds.total_cmp(&b.start_seconds));

    let mut chunks = Vec::new();
    for (index, (start, end)) in [(0.0, 30.0), (30.0, 60.0), (60.0, 90.0)]
        .into_iter()
        .enumerate()
    {
        let mut owned: Vec<TimedSpan> = spans
            .iter()
            .filter(|s| {
                let mid = (s.start_seconds + s.end_seconds) / 2.0;
                mid >= start && mid < end
            })
            .cloned()
            .collect();
        if index == 1 {
            owned.insert(0, seam_repeat.clone());
        }
        chunks.push(chunk(
            index,
            start,
            end,
            if index == 0 {
                BoundaryKind::Edge
            } else {
                BoundaryKind::FixedClock
            },
            owned,
        ));
    }
    chunks
}

/// Track 1 — the tap, cut on 47-second SILENCES. Nothing about 47 lines up with
/// 30, which is the point, and its clock runs 100 ppm fast on top.
fn tap_chunks() -> Vec<ChunkOutcome> {
    let spans: Vec<TimedSpan> = (0..BEATS)
        .filter(|i| i % 2 == 0)
        .map(|i| word(TAP_PPM, beat_time(i), 0.4, &format!("them-{i}")))
        .collect();
    let cut = local_seconds(TAP_PPM, 47.0);
    let end = local_seconds(TAP_PPM, 120.0);
    let (early, late): (Vec<_>, Vec<_>) = spans
        .into_iter()
        .partition(|s| (s.start_seconds + s.end_seconds) / 2.0 < cut);
    vec![
        chunk(0, 0.0, cut, BoundaryKind::Edge, early),
        chunk(1, cut, end, BoundaryKind::Silence, late),
    ]
}

#[test]
fn misaligned_seams_still_interleave_strictly_by_host_time() {
    let merged = merge_two_tracks_by_host_time(
        &mic_chunks(),
        &tap_chunks(),
        &index_records(MIC_PPM, 120),
        &index_records(TAP_PPM, 120),
        TrackEpochs::SHARED,
    );

    // 1. Strictly ordered on the session clock.
    for pair in merged.windows(2) {
        assert!(
            pair[0].start_seconds <= pair[1].start_seconds,
            "out of order: {:?} at {:.4} before {:?} at {:.4}",
            pair[0].text,
            pair[0].start_seconds,
            pair[1].text,
            pair[1].start_seconds
        );
    }

    // 2. The conversation reads as a back-and-forth, and the labels are right.
    let mut expected: Vec<(String, String)> = Vec::new();
    for i in 0..BEATS {
        if i == 9 {
            // The seam-duplicate word sits at 29.6 s, between beats 9 (29 s)
            // and 10 (32 s). It belongs to the mic.
            expected.push(("Me".into(), format!("me-{i}")));
            expected.push(("Me".into(), "boundary".into()));
            continue;
        }
        expected.push((
            if i % 2 == 0 { "Them" } else { "Me" }.to_string(),
            if i % 2 == 0 {
                format!("them-{i}")
            } else {
                format!("me-{i}")
            },
        ));
    }
    assert_eq!(turns(&merged), expected);

    // 3. The per-track seam dedupe still ran: the duplicated word survives
    // exactly once.
    assert_eq!(
        merged.iter().filter(|s| s.text == "boundary").count(),
        1,
        "the cross-track interleave must run the per-track seam merge, not \
         bypass it — otherwise every chunk boundary re-emits its overlap"
    );

    // 4. Two speakers, and each span carries the track it came from as well as
    // the label, so a renderer never has to parse the label back.
    for span in &merged {
        assert_eq!(
            span.speaker,
            wilson_voice_lib::meetings::speaker_label(span.track)
        );
    }
    assert!(merged.iter().any(|s| s.track == 0));
    assert!(merged.iter().any(|s| s.track == 1));
}

/// A tap that starts LATE — the normal case, because building the aggregate
/// device takes real time — still lands on the session's clock rather than on
/// its own.
///
/// Without the epoch, both tracks' `host_ns` read zero at their own first
/// callback and the tap's whole hour is shifted forward by however long its
/// setup took. Two seconds is a modest, realistic number and it is more than
/// enough to reverse three turns.
#[test]
fn a_tap_that_started_late_is_placed_on_the_sessions_clock_not_its_own() {
    const TAP_LATE_NS: i64 = 2_000_000_000;

    let mic = mic_chunks();
    // The tap's own timeline starts at ITS first callback, so a turn spoken at
    // session second `t` is at `t - 2` on the tap's clock.
    let spans: Vec<TimedSpan> = (0..BEATS)
        .filter(|i| i % 2 == 0)
        .filter(|i| beat_time(*i) >= 2.0)
        .map(|i| word(TAP_PPM, beat_time(i) - 2.0, 0.4, &format!("them-{i}")))
        .collect();
    let tap = vec![chunk(
        0,
        0.0,
        local_seconds(TAP_PPM, 120.0),
        BoundaryKind::Edge,
        spans,
    )];

    let merged = merge_two_tracks_by_host_time(
        &mic,
        &tap,
        &index_records(MIC_PPM, 120),
        &index_records(TAP_PPM, 120),
        TrackEpochs::new(0, TAP_LATE_NS),
    );

    // Beat 0 is at session second 2.0 and beat 1 at 5.0, so the tap's first
    // word must still come first. It only does if the epoch was applied.
    let first = merged.first().expect("the merge produced spans");
    assert_eq!(first.text, "them-0");
    assert!(
        (first.start_seconds - 2.0).abs() < 0.01,
        "them-0 was spoken at session second 2.0, got {:.4}",
        first.start_seconds
    );

    // And the whole thing is still ordered.
    assert!(merged
        .windows(2)
        .all(|w| w[0].start_seconds <= w[1].start_seconds));
}

/// A track that LOST audio is still placed by where the finalized wav puts it,
/// not by what reached the disk.
///
/// This is the COUNTER rule's shape: the two counters diverge once at the gap
/// and never converge again, the finalize splices the difference back in, and a
/// consumer reading `spilled_samples` raw shifts every word after the gap by
/// the whole length of the gap — silently, on the recordings that already had a
/// problem. It is the case where the finalized position happens to equal
/// `captured_samples`, which is why it needs
/// `a_device_stall_does_not_shift_the_rest_of_the_meeting` beside it: on its
/// own, this test cannot tell those two apart.
#[test]
fn a_track_that_lost_audio_is_mapped_on_what_the_device_delivered() {
    // Two seconds of the tap's audio never reached the journal, 50 seconds in
    // (spread across the two intervals it really takes to lose two seconds —
    // `spilled_samples` is a counter and counters do not run backwards). The
    // finalize splices two seconds of silence there, so the finalized wav is
    // `captured` long here and a word spoken at real second 80 is still
    // timestamped at second 80 of that wav (times the tap's 100 ppm).
    const LOST_SAMPLES: u64 = 2 * 16_000;
    let tap = vec![chunk(
        0,
        0.0,
        local_seconds(TAP_PPM, 120.0),
        BoundaryKind::Edge,
        vec![word(TAP_PPM, 80.0, 0.4, "them-late")],
    )];

    let merged = merge_two_tracks_by_host_time(
        &mic_chunks(),
        &tap,
        &index_records(MIC_PPM, 120),
        &index_records_lossy(TAP_PPM, 120, 50, LOST_SAMPLES),
        TrackEpochs::SHARED,
    );

    let them = merged
        .iter()
        .find(|s| s.text == "them-late")
        .expect("the merge kept the tap's word");
    assert!(
        (them.start_seconds - 80.0).abs() < 0.05,
        "a word spoken at real second 80 came back at {:.4}; keying the map on \
         spilled_samples — the raw counter, with the repair the finalize made \
         left out — would put it at ≈82.0, the whole gap late",
        them.start_seconds
    );
}

/// A track whose DEVICE STALLED is still placed by where the finalized wav
/// actually puts it — which is neither of the two columns the record stores.
///
/// This is the case `a_track_that_lost_audio_...` above cannot reach, and the
/// one that decides the keying. A stall means no callback fires, so `captured`
/// and `spilled` freeze **together** — `captured − spilled` stays at zero and
/// the counter rule reports a flawless recording — while
/// `plan_silence_splices`'s wall-clock rule splices the entire shortfall in.
/// The finalized wav is therefore LONGER than `captured`, and a map keyed on
/// `captured_samples` bills every word after the stall for the whole stall:
/// with the ten seconds below, a word spoken at host second 80 comes back at
/// session second 90, a 10,000 ms residual against this item's 50 ms budget.
///
/// It is the track-1 case in particular, which is why the tap is what stalls
/// here: a process tap gets no callbacks at all while the tapped app is silent,
/// so this is the normal shape of a meeting where nobody on the far side has
/// spoken for a while, not a hardware fault.
#[test]
fn a_device_stall_does_not_shift_the_rest_of_the_meeting() {
    const STALL_AT: f64 = 60.0;
    const STALL_SECONDS: f64 = 10.0;
    const SPOKEN_AT: f64 = 80.0;

    let records = index_records_stalled(TAP_PPM, 120, STALL_AT as u64, STALL_SECONDS as u64);

    // The fixture really is the stall the doc comment describes, asserted
    // against the SHIPPING planner rather than trusted: both counters frozen
    // across the gap, and a splice the size of the gap anyway.
    let at = |t: u64| records[t as usize];
    let IndexRecord {
        captured_samples: c_before,
        spilled_samples: s_before,
        ..
    } = at(STALL_AT as u64);
    let IndexRecord {
        captured_samples: c_after,
        spilled_samples: s_after,
        ..
    } = at((STALL_AT + STALL_SECONDS) as u64);
    assert_eq!(
        (c_before, s_before),
        (c_after, s_after),
        "the fixture must freeze BOTH counters — that is what makes it a stall \
         rather than a drop"
    );
    let spliced: u64 = plan_silence_splices(&records)
        .iter()
        .map(|s| s.silence_samples)
        .sum();
    assert_eq!(
        spliced, 160_000,
        "the wall-clock rule must splice the whole ten seconds the counters \
         could not see"
    );

    let tap = vec![chunk(
        0,
        0.0,
        local_seconds_stalled(TAP_PPM, 120.0, STALL_AT, STALL_SECONDS),
        BoundaryKind::Edge,
        vec![TimedSpan {
            start_seconds: local_seconds_stalled(TAP_PPM, SPOKEN_AT, STALL_AT, STALL_SECONDS),
            end_seconds: local_seconds_stalled(TAP_PPM, SPOKEN_AT + 0.4, STALL_AT, STALL_SECONDS),
            text: "them-after-the-stall".into(),
        }],
    )];

    let merged = merge_two_tracks_by_host_time(
        &mic_chunks(),
        &tap,
        &index_records(MIC_PPM, 120),
        &records,
        TrackEpochs::SHARED,
    );

    let them = merged
        .iter()
        .find(|s| s.text == "them-after-the-stall")
        .expect("the merge kept the tap's word");
    let residual_ms = (them.start_seconds - SPOKEN_AT).abs() * 1000.0;
    assert!(
        residual_ms <= RESIDUAL_BUDGET_SECONDS * 1000.0,
        "a word spoken at host second {SPOKEN_AT} came back at {:.4} — a \
         {residual_ms:.1} ms residual against a {:.0} ms budget. Keying the map \
         on EITHER stored counter (a stall freezes both together) puts it at \
         ≈{:.1}, the whole stall late.",
        them.start_seconds,
        RESIDUAL_BUDGET_SECONDS * 1000.0,
        SPOKEN_AT + STALL_SECONDS
    );

    // And the interleave is still ordered — the mic's words around it did not
    // get dragged anywhere either.
    assert!(merged
        .windows(2)
        .all(|w| w[0].start_seconds <= w[1].start_seconds));
}

/// A stream REOPENED mid-meeting does not re-zero the rest of the meeting.
///
/// This is the stall test's defect class one turn further round, and strictly
/// worse. `record.rs::build_capture_stream` rebases `host_ns` to each stream
/// build's own first callback, and `MeetingCapture::retune_track` — the shipped,
/// documented handler for exactly that reopen, whose own doc comment names the
/// AirPods swap and YV103's `RebuildAggregate` — deliberately keeps
/// `captured_samples` running across the seam. So the wav is continuous while
/// the clock underneath it restarts at zero.
///
/// A map that merely DROPS the backwards records survives only until the new
/// clock passes the old maximum, and then starts believing it: from there on,
/// every word is placed on the NEW zero and the whole post-reopen remainder of
/// the meeting is early by the length of the pre-reopen run. With the numbers
/// below — a five-minute-in swap in a ninety-minute meeting — a word spoken at
/// session second 4000 comes back at 3700, a 300,000 ms residual against a 50 ms
/// budget.
///
/// **The post-reopen run has to OUTLIVE the pre-reopen one or the fixture cannot
/// fail**, and that ordering is the realistic one: the aggregate gets rebuilt
/// minutes in, the meeting has hours left.
#[test]
fn a_stream_reopened_early_does_not_re_zero_the_rest_of_the_meeting() {
    const REOPEN_AT: u64 = 300;
    const AFTER: u64 = 5100;
    const SPOKEN_AT: f64 = 4000.0;

    let records = index_records_rebased(TAP_PPM, REOPEN_AT, TAP_PPM, AFTER);

    // The fixture really is the seam the doc comment describes, asserted rather
    // than trusted: the clock runs backwards while the capture counter does not.
    let seam = records[REOPEN_AT as usize + 1];
    let before = records[REOPEN_AT as usize];
    assert!(
        seam.host_ns < before.host_ns,
        "the reopen must restart the clock"
    );
    assert!(
        seam.captured_samples >= before.captured_samples,
        "and must NOT restart the capture counter — retune_track carries it \
         across the seam via captured_base"
    );

    let tap = vec![chunk(
        0,
        0.0,
        local_seconds(TAP_PPM, (REOPEN_AT + AFTER) as f64),
        BoundaryKind::Edge,
        vec![word(TAP_PPM, SPOKEN_AT, 0.4, "them-after-the-reopen")],
    )];

    let merged = merge_two_tracks_by_host_time(
        &mic_chunks(),
        &tap,
        &index_records(MIC_PPM, REOPEN_AT + AFTER),
        &records,
        TrackEpochs::SHARED,
    );

    let them = merged
        .iter()
        .find(|s| s.text == "them-after-the-reopen")
        .expect("the merge kept the tap's word");
    let residual_ms = (them.start_seconds - SPOKEN_AT).abs() * 1000.0;
    assert!(
        residual_ms <= RESIDUAL_BUDGET_SECONDS * 1000.0,
        "a word spoken at session second {SPOKEN_AT} came back at {:.4} — a \
         {residual_ms:.1} ms residual against a {:.0} ms budget",
        them.start_seconds,
        RESIDUAL_BUDGET_SECONDS * 1000.0
    );

    // Non-vacuity: the post-reopen records really do carry a re-zeroed clock, so
    // a map that drops the backwards records and then believes the new ones —
    // the filter this test exists to keep deleted — lands this word on the new
    // zero, {REOPEN_AT} s early.
    let post = records
        .iter()
        .rev()
        .find(|r| (r.captured_samples as f64) <= local_seconds(TAP_PPM, SPOKEN_AT) * 16_000.0)
        .expect("a record before the word");
    let drop_only_seconds = post.host_ns as f64 / 1e9;
    assert!(
        (drop_only_seconds - (SPOKEN_AT - REOPEN_AT as f64)).abs() < 2.0,
        "the fixture must contain the defect: the new clock reads \
         {drop_only_seconds:.1} s where the session is at {SPOKEN_AT} s, so a \
         drop-only filter is {REOPEN_AT} s early. Got a clock that is not \
         re-zeroed, which means this fixture proves nothing."
    );

    assert!(merged
        .windows(2)
        .all(|w| w[0].start_seconds <= w[1].start_seconds));
}

/// What a reopen still costs, measured rather than claimed: the dead air across
/// the stream rebuild, once, and nothing that accumulates.
///
/// The old stream's last callback and the new stream's first are rebased to
/// different instants and nothing in the sidecar measures the distance between
/// them, so audio that was never captured cannot be invented. Post-reopen spans
/// are therefore early by exactly that gap. The assertion that matters is the
/// SECOND one: the offset an hour later is the same offset, not a growing one —
/// which is the whole difference between this and the ppm drift the item
/// deletes.
#[test]
fn a_reopen_is_early_by_its_own_dead_air_and_by_nothing_else() {
    const REOPEN_AT: u64 = 300;
    const AFTER: u64 = 5100;
    // How long the stream rebuild took. No audio exists for it on this track.
    const DEAD_AIR: f64 = 1.5;

    let records = index_records_rebased(TAP_PPM, REOPEN_AT, TAP_PPM, AFTER);
    // A word spoken at session second `t` sits at `t − DEAD_AIR` in the wav,
    // because the wav is missing the dead air entirely.
    let at = |t: f64, text: &str| {
        let local = local_seconds(TAP_PPM, t - DEAD_AIR);
        wilson_voice_lib::asr_engine::TimedSpan {
            start_seconds: local,
            end_seconds: local + 0.4,
            text: text.to_string(),
        }
    };
    let tap = vec![chunk(
        0,
        0.0,
        local_seconds(TAP_PPM, (REOPEN_AT + AFTER) as f64),
        BoundaryKind::Edge,
        vec![
            at(320.0, "them-just-after"),
            at(4000.0, "them-an-hour-later"),
        ],
    )];

    let merged = merge_two_tracks_by_host_time(
        &mic_chunks(),
        &tap,
        &index_records(MIC_PPM, REOPEN_AT + AFTER),
        &records,
        TrackEpochs::SHARED,
    );

    let offset = |text: &str, truth: f64| {
        let span = merged
            .iter()
            .find(|s| s.text == text)
            .unwrap_or_else(|| panic!("the merge dropped {text:?}"));
        truth - span.start_seconds
    };
    let just_after = offset("them-just-after", 320.0);
    let an_hour_later = offset("them-an-hour-later", 4000.0);

    for (label, offset) in [("just after", just_after), ("an hour later", an_hour_later)] {
        assert!(
            (offset - DEAD_AIR).abs() * 1000.0 <= RESIDUAL_BUDGET_SECONDS * 1000.0,
            "{label}: expected the reopen's own {DEAD_AIR} s of dead air and \
             nothing else, got {offset:.4} s"
        );
    }
    assert!(
        (an_hour_later - just_after).abs() * 1000.0 <= RESIDUAL_BUDGET_SECONDS * 1000.0,
        "the reopen's cost must not GROW with the meeting: {just_after:.4} s \
         twenty seconds in against {an_hour_later:.4} s an hour later"
    );
}
