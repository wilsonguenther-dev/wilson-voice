//! YV107 / OS-2 acceptance — "the measured `Δsamples ÷ Δhost_seconds` for each
//! track is computed and written into the meeting's `diagnostics` JSON under
//! `track_rates`, and a track whose measured rate diverges from its nominal rate
//! by more than the ppm figures above is flagged in the log (not silently
//! accepted)."
//!
//! Two halves, and both are load-bearing.
//!
//! **The measurement** is a pure function over `IndexRecord`s, so it is asserted
//! against a fixture whose true rate is known by construction. This is the datum
//! OS-2 says the escalation decision turns on — the single private aggregate
//! device with `kAudioSubDeviceDriftCompensationKey`, which is expensive
//! (forking the mic off `cpal` breaks the dictation fan-out) and is therefore
//! deliberately gated on a measurement rather than on a hunch. A measurement
//! that is never recorded is a decision that will be made on a hunch anyway.
//!
//! **The write** is asserted end to end, through the real `MeetingController`
//! stop path into a real SQLite file, because the interesting failure is not
//! "the number is wrong" — it is "the number is right and lands nowhere."

mod support;

#[path = "support/two_track.rs"]
mod two_track;

use std::sync::Arc;

use support::{open_db, temp_dir};
use two_track::{index_records, index_records_lossy, index_records_rebased};
use wilson_voice_lib::meeting::{
    measure_true_rate, MIC_TRACK, SYSTEM_TRACK, TARGET_RATE, TRUE_RATE_MIN_SPAN_SECONDS,
    TRUE_RATE_PPM_LIMIT,
};
use wilson_voice_lib::meeting_control::{MeetingController, MeetingStatus, StatusSink};
use wilson_voice_lib::meeting_energy::MeetingDiagnostics;

fn quiet_sink() -> StatusSink {
    Arc::new(|_: &MeetingStatus| {})
}

// ── the measurement ─────────────────────────────────────────────────────────

#[test]
fn the_measured_rate_is_delta_samples_over_delta_host_seconds() {
    for ppm in [0.0, -37.5, 12.0, 100.0, -220.0] {
        let records = index_records(ppm, 600);
        let rate = measure_true_rate(MIC_TRACK, &records)
            .unwrap_or_else(|| panic!("{ppm} ppm over 600 s is measurable"));

        assert_eq!(rate.track, MIC_TRACK);
        assert_eq!(rate.nominal_rate, TARGET_RATE);
        assert_eq!(
            rate.intervals, 600,
            "one interval per index record after the first"
        );
        assert!((rate.span_seconds - 600.0).abs() < 1e-9);

        let expected_rate = TARGET_RATE as f64 * (1.0 + ppm / 1e6);
        assert!(
            (rate.measured_rate - expected_rate).abs() < 0.01,
            "{ppm} ppm: measured {} Hz, expected ≈{expected_rate} Hz",
            rate.measured_rate
        );
        assert!(
            (rate.ppm - ppm).abs() < 0.2,
            "{ppm} ppm: reported {:.3} ppm",
            rate.ppm
        );

        // OS-2's own projection: ≈216 ms at 20 ppm, ≈430 ms at 40 ppm, ≈1.08 s
        // at 100.
        let expected_drift = ppm.abs() / 1e6 * 3.0 * 3600.0 * 1000.0;
        assert!((rate.drift_at_cap_ms - expected_drift).abs() < 1.0);
    }
}

#[test]
fn a_track_outside_the_crystal_tolerance_is_flagged_and_one_inside_it_is_not() {
    // ±20–50 ppm is the band OS-2 names for consumer audio crystals; Apple's own
    // aggregate-device documentation illustrates drift with 44099 vs 44100 Hz,
    // which is ≈23 ppm. Inside the band is normal and is recorded without
    // alarm.
    for ok in [0.0, 12.0, -23.0, 49.0] {
        let rate = measure_true_rate(MIC_TRACK, &index_records(ok, 600)).unwrap();
        assert!(
            !rate.flagged,
            "{ok} ppm is inside the ±{TRUE_RATE_PPM_LIMIT} ppm crystal band and \
             must not be flagged"
        );
    }
    // Past it is not a crystal tolerance — it is a rate that was misreported, a
    // device that renegotiated without saying so, or a clock domain nobody
    // expected. The recording is still correct (the merge is on host time), but
    // it is never SILENTLY accepted.
    for bad in [51.0, 100.0, -180.0] {
        let rate = measure_true_rate(SYSTEM_TRACK, &index_records(bad, 600)).unwrap();
        assert!(rate.flagged, "{bad} ppm must be flagged");
        assert!(rate.drift_at_cap_ms > 50.0);
    }
    // The exact escalation line OS-2 names, at the horizon it names it at: past
    // 50 ppm a 2-hour session is already worth more than 50 ms of offset, which
    // is the trigger for the deferred single-aggregate-device fix.
    let at_limit = measure_true_rate(SYSTEM_TRACK, &index_records(51.0, 600)).unwrap();
    assert!(at_limit.ppm.abs() / 1e6 * 7200.0 * 1000.0 > 50.0);
}

#[test]
fn a_rate_that_cannot_honestly_be_measured_is_none_rather_than_zero() {
    assert!(measure_true_rate(MIC_TRACK, &[]).is_none(), "no records");
    assert!(
        measure_true_rate(MIC_TRACK, &index_records(0.0, 0)).is_none(),
        "one record is not an interval"
    );
    // Under the minimum span the numerator is scheduler jitter rather than the
    // crystal, and reporting a guess as a measurement is worse than reporting
    // nothing.
    let short = (TRUE_RATE_MIN_SPAN_SECONDS as u64) - 1;
    assert!(
        measure_true_rate(MIC_TRACK, &index_records(0.0, short)).is_none(),
        "{short} s is under the {TRUE_RATE_MIN_SPAN_SECONDS} s floor"
    );
    assert!(measure_true_rate(MIC_TRACK, &index_records(0.0, 600)).is_some());
}

#[test]
fn a_stream_rebuilt_mid_meeting_rebases_its_clock_and_does_not_invent_a_rate() {
    // A reopen rebases `host_ns` to its own first callback, so the records run
    // backwards at the seam. Believing that interval would report a wildly
    // negative rate; the measurement closes the run there and reports the run
    // it can stand behind.
    let records = index_records_rebased(0.0, 600, 0.0, 60);

    let rate = measure_true_rate(MIC_TRACK, &records).expect("the pre-rebase run is measurable");
    assert!(
        (rate.ppm).abs() < 1.0,
        "the rebase must not poison the rate; got {:.1} ppm",
        rate.ppm
    );
    assert!(!rate.flagged);
    assert_eq!(
        rate.segments, 2,
        "the reopen is REPORTED, not merely survived — a reader has to be able \
         to see that the number describes one run of a meeting that has two"
    );
    assert!(
        (rate.span_seconds - 600.0).abs() < 1e-6,
        "the longest run is the 600 s one; got {:.1} s",
        rate.span_seconds
    );
}

/// The same reopen with the runs the other way round — which is the COMMON way
/// round, and the one a short post-reopen fixture cannot fail.
///
/// A five-minute-in swap in a ninety-minute meeting leaves a 300 s run followed
/// by a 5100 s one. Taking `first` from records[0] and letting `last` advance
/// across the seam once the new clock catches up divides the WHOLE meeting's
/// samples by the part of it the new clock reached: 5400 s of audio over a
/// 5100 s span, which is 16941 Hz, +58,824 ppm, flagged, and ≈635,000 ms of
/// projected drift — a fabricated number on a device whose crystal is perfect.
/// It is also the exact datum OS-2 says the deferred single-aggregate-device
/// escalation gets decided on, so fabricating it is worse than reporting
/// nothing.
#[test]
fn a_reopen_whose_second_run_outlives_the_first_still_measures_a_real_rate() {
    const BEFORE: u64 = 300;
    const AFTER: u64 = 5100;

    let records = index_records_rebased(0.0, BEFORE, 0.0, AFTER);
    let rate = measure_true_rate(MIC_TRACK, &records).expect("the long run is measurable");

    assert!(
        rate.ppm.abs() < 1.0,
        "a 0 ppm device stays a 0 ppm device across a reopen; got {:.1} ppm \
         ({:.3} Hz over {:.1} s)",
        rate.ppm,
        rate.measured_rate,
        rate.span_seconds
    );
    assert!(
        !rate.flagged,
        "and a clean device is not flagged for having been reopened"
    );
    assert_eq!(rate.segments, 2);
    assert!(
        (rate.span_seconds - AFTER as f64).abs() < 1e-6,
        "the LONGEST run is the one worth reporting; got {:.1} s",
        rate.span_seconds
    );
    assert!(rate.drift_at_cap_ms < 50.0);

    // Non-vacuity: the fixture really does contain the defect. A first..last
    // span taken ACROSS the seam is what the numbers above describe.
    let (first, last) = (records.first().unwrap(), records.last().unwrap());
    let across = (last.captured_samples - first.captured_samples) as f64
        / ((last.host_ns - first.host_ns) as f64 / 1e9);
    let across_ppm = (across - TARGET_RATE as f64) / TARGET_RATE as f64 * 1e6;
    assert!(
        across_ppm > 10_000.0,
        "a span across the rebase must be absurd ({across:.3} Hz, \
         {across_ppm:+.1} ppm) or this fixture proves nothing"
    );
}

/// The longest run is the one measured, and its OWN rate is what comes back.
///
/// Not the first run's, not an average of the two: a device that renegotiated
/// its rate at the reopen really is running at the new rate for the rest of the
/// meeting, and that is the rate an escalation decision needs.
#[test]
fn the_rate_reported_is_the_longest_runs_own_rate() {
    let records = index_records_rebased(0.0, 300, 12.0, 5100);
    let rate = measure_true_rate(SYSTEM_TRACK, &records).expect("measurable");
    assert!(
        (rate.ppm - 12.0).abs() < 0.2,
        "expected the 5100 s run's +12 ppm, got {:.2} ppm",
        rate.ppm
    );
    assert_eq!(rate.segments, 2);
}

/// A meeting with no reopen says so — `segments` is 1, not 0 and not absent.
#[test]
fn a_track_that_was_never_reopened_reports_one_segment() {
    let rate = measure_true_rate(MIC_TRACK, &index_records(0.0, 600)).unwrap();
    assert_eq!(rate.segments, 1);

    // And a blob written before the field existed reads as the single-run
    // meeting it was, rather than as zero runs.
    let old = r#"{"track":0,"nominalRate":16000,"measuredRate":16000.0,"ppm":0.0,
        "intervals":600,"spanSeconds":600.0,"driftAtCapMs":0.0,"flagged":false}"#;
    let parsed: wilson_voice_lib::meeting::TrackRate = serde_json::from_str(old).unwrap();
    assert_eq!(parsed.segments, 1);
}

/// The rate is what the DEVICE delivered per host second, not what survived the
/// journal.
///
/// A meeting that dropped audio has a perfectly good clock; measuring the rate
/// off `spilled_samples` would report that meeting's device as running slow and
/// flag a track whose crystal is fine — which is how a diagnostic stops being
/// believed.
#[test]
fn the_rate_is_what_the_device_delivered_not_what_reached_disk() {
    // 12 ppm and well inside tolerance, but 30 seconds of audio never reached
    // the spill. Measured on `spilled_samples` that reads as −49,988 ppm.
    let records = index_records_lossy(12.0, 600, 300, 30 * TARGET_RATE as u64);
    let rate = measure_true_rate(MIC_TRACK, &records).expect("still measurable");
    assert!(
        (rate.ppm - 12.0).abs() < 0.2,
        "the device ran at +12 ppm regardless of what the disk accepted; got {:.1} ppm",
        rate.ppm
    );
    assert!(
        !rate.flagged,
        "a journal gap is not a clock problem and must not be reported as one"
    );
}

// ── the write ───────────────────────────────────────────────────────────────

#[test]
fn the_blob_carries_the_rates_under_the_track_rates_key() {
    let mut d = MeetingDiagnostics::start(
        wilson_voice_lib::meeting_energy::ThermalState::Nominal,
        wilson_voice_lib::meeting_energy::BatteryReading::default(),
        true,
    );
    let mic = measure_true_rate(MIC_TRACK, &index_records(12.0, 600)).unwrap();
    let tap = measure_true_rate(SYSTEM_TRACK, &index_records(100.0, 600)).unwrap();
    d.record_track_rates(&[mic, tap]);

    let json: serde_json::Value = serde_json::from_str(&d.to_json()).expect("valid JSON");
    let rates = json
        .get("track_rates")
        .and_then(|v| v.as_array())
        .expect("the key the backlog names, spelled the way it names it");
    assert_eq!(rates.len(), 2);
    assert_eq!(rates[0]["track"], 0);
    assert_eq!(rates[1]["track"], 1);
    assert_eq!(rates[0]["flagged"], false);
    assert_eq!(rates[1]["flagged"], true);
    assert!(rates[1]["measuredRate"].as_f64().unwrap() > TARGET_RATE as f64);
    assert!(rates[1]["ppm"].as_f64().unwrap() > TRUE_RATE_PPM_LIMIT);
    assert!(rates[1]["driftAtCapMs"].as_f64().unwrap() > 1000.0);

    // And it round-trips, so a reader a week later gets values rather than a
    // parse error.
    let back: MeetingDiagnostics = serde_json::from_str(&d.to_json()).unwrap();
    assert_eq!(back.track_rates.len(), 2);
    assert_eq!(back.flagged_track_rates().len(), 1);
    assert_eq!(back.flagged_track_rates()[0].track, SYSTEM_TRACK);
}

#[test]
fn a_blob_written_before_this_item_still_parses_and_says_nothing() {
    // Exactly what migration 2's column holds today, with no `track_rates` key
    // anywhere in it.
    let old = r#"{
        "version": 1,
        "thermalAtStart": "nominal",
        "thermalAtEnd": "fair",
        "thermalTransitions": [],
        "batteryAtStart": {"onAc": true, "percent": 90, "minutesRemaining": null, "hasBattery": true},
        "batteryAtEnd": null,
        "pillVisible": true,
        "stopReason": "user stopped"
    }"#;
    let d: MeetingDiagnostics = serde_json::from_str(old).expect("an older blob still parses");
    assert!(
        d.track_rates.is_empty(),
        "never measured is not measured nothing"
    );
}

/// End to end: a finished two-track meeting's row carries the rates.
///
/// This is the assertion that would catch the interesting failure — a
/// measurement that is computed correctly and then dropped on the floor between
/// the finalize and the row.
#[test]
fn a_finished_meetings_row_carries_the_measured_rates() {
    let _guard = support::exclusive();
    support::install_fake_engine();
    support::set_fake_mode(support::FAKE_TWO_TRACK_DRIFT);
    let dir = temp_dir("yv107-rates");
    let db = Arc::new(open_db(&dir));
    let c = MeetingController::new(Arc::clone(&db), quiet_sink());

    let id = c.start(&dir, Some("Drifty call".into())).expect("start");
    c.stop("user stopped").expect("stop");
    support::set_fake_mode(support::FAKE_OK);

    let blob = db
        .get_meeting(&id)
        .unwrap()
        .unwrap()
        .diagnostics
        .expect("the row carries a diagnostics blob");

    // The raw key, because the column is read by hand as often as by serde.
    let json: serde_json::Value = serde_json::from_str(&blob).unwrap();
    assert!(
        json.get("track_rates").is_some(),
        "the stop path must bank the rates BEFORE the blob is written:\n{blob}"
    );

    let d: MeetingDiagnostics = serde_json::from_str(&blob).unwrap();
    assert_eq!(d.track_rates.len(), 2, "one entry per recorded track");
    assert_eq!(d.track_rates[0].track, MIC_TRACK);
    assert_eq!(d.track_rates[1].track, SYSTEM_TRACK);
    assert!(!d.track_rates[0].flagged, "the mic is inside tolerance");
    assert!(d.track_rates[1].flagged, "the tap is 100 ppm fast");
    // And the rest of the blob survived — the rates are an ADDITION to it, not
    // a replacement for it.
    assert_eq!(d.stop_reason.as_deref(), Some("user stopped"));
    assert!(d.battery_at_end.is_some());
}
