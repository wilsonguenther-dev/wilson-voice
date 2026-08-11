//! YV92 — the anti-alias filter, measured (plan finding OS-8).
//!
//! The claim this file has to make falsifiable: **≥20 dB of attenuation at
//! 10 kHz before the 3:1 decimation to 16 kHz**, with the speech band left
//! alone. Everything here is a synthetic sine through the real filter — no
//! audio hardware, no corpus, no model — so it runs everywhere the crate builds
//! and it fails loudly if anyone lowers the order or moves the cutoff.
//!
//! The measurement is deliberately naive: drive the filter with a steady tone,
//! throw away the settling transient, and compare RMS in to RMS out. That is
//! the same quantity the folding argument is about, and it needs no FFT to be
//! believed.
//!
//! Why the negative control at the bottom matters more than the rest: showing
//! the new filter rejects 10 kHz proves nothing on its own unless the OLD path
//! demonstrably did not. `linear_interpolation_alone_folds_a_10khz_tone_into_the_speech_band`
//! reproduces the actual defect — a 10 kHz tone decimated 48 kHz → 16 kHz by
//! pure linear interpolation reappears as a 6 kHz tone at nearly full level —
//! and then shows the shipped decimator removing it.

use wilson_voice_lib::resample::{
    resample_decimate, resample_linear, Biquad, LowPassCascade, ANTI_ALIAS_CUTOFF_HZ,
};

const NATIVE_SR: u32 = 48_000;
const TARGET_SR: u32 = 16_000;
/// The acceptance bar for this item.
const REQUIRED_REJECTION_DB: f32 = 20.0;

fn tone(rate: u32, seconds: f32, hz: f32) -> Vec<f32> {
    let n = (rate as f32 * seconds) as usize;
    (0..n)
        .map(|i| (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin() * 0.5)
        .collect()
}

/// RMS over everything after the filter has settled (50 ms).
fn settled_rms(samples: &[f32], rate: u32) -> f32 {
    let skip = (rate as usize / 20).min(samples.len());
    let body = &samples[skip..];
    if body.is_empty() {
        return 0.0;
    }
    (body.iter().map(|s| (s * s) as f64).sum::<f64>() / body.len() as f64).sqrt() as f32
}

/// Attenuation in dB the anti-alias cascade applies to a steady `hz` tone at
/// `rate`. Positive dB = rejection.
fn rejection_db(rate: u32, hz: f32) -> f32 {
    let input = tone(rate, 0.3, hz);
    let mut filtered = input.clone();
    let mut filter =
        LowPassCascade::for_decimation(rate, TARGET_SR).expect("a decimation has a filter");
    assert!(filter.process(&mut filtered), "the filter must stay finite");
    let before = settled_rms(&input, rate);
    let after = settled_rms(&filtered, rate);
    assert!(before > 0.0);
    -20.0 * (after.max(1e-12) / before).log10()
}

/// Correlate a buffer against a reference sine at `hz` — the amplitude of that
/// component, normalised so a full-scale tone reads ~1.0. Enough to say "the
/// alias is (not) there" without pulling in an FFT crate.
fn component_amplitude(samples: &[f32], rate: u32, hz: f32) -> f32 {
    let n = samples.len();
    if n == 0 {
        return 0.0;
    }
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (i, s) in samples.iter().enumerate() {
        let w = 2.0 * std::f64::consts::PI * hz as f64 * i as f64 / rate as f64;
        re += *s as f64 * w.cos();
        im += *s as f64 * w.sin();
    }
    (2.0 * (re * re + im * im).sqrt() / n as f64) as f32
}

#[test]
fn anti_alias_filter_rejects_at_least_20_db_at_10_khz() {
    let db = rejection_db(NATIVE_SR, 10_000.0);
    eprintln!("anti-alias rejection at 10 kHz (48 kHz input): {db:.2} dB");
    assert!(
        db >= REQUIRED_REJECTION_DB,
        "the acceptance bar is ≥{REQUIRED_REJECTION_DB} dB at 10 kHz pre-decimation, measured {db:.2} dB"
    );
}

#[test]
fn anti_alias_filter_rejects_the_whole_band_that_would_fold() {
    // Everything from 10 kHz to just under the 24 kHz Nyquist folds into
    // 0–8 kHz on the way to 16 kHz, so the bar applies across the whole band —
    // and the WORST point in it must be the one nearest the corner, which is
    // what rules out a resonant section dipping somewhere in the middle.
    //
    // Not asserted: monotonicity. A bilinear-transformed filter's response
    // flattens as it approaches Nyquist rather than falling forever (measured:
    // 68.5 dB at 16 kHz, 65.4 dB at 20 kHz), which is a property of the digital
    // filter, not a defect.
    let mut worst = f32::MAX;
    for hz in [10_000.0, 12_000.0, 14_000.0, 16_000.0, 20_000.0, 23_000.0] {
        let db = rejection_db(NATIVE_SR, hz);
        eprintln!("  {hz:>8.0} Hz → {db:6.2} dB");
        assert!(
            db >= REQUIRED_REJECTION_DB,
            "{hz} Hz folds into the speech band and must be rejected by ≥{REQUIRED_REJECTION_DB} dB, got {db:.2}"
        );
        worst = worst.min(db);
    }
    let at_corner = rejection_db(NATIVE_SR, 10_000.0);
    assert!(
        (worst - at_corner).abs() < 0.01,
        "the least-rejected frequency in the fold band must be the one nearest the corner \
         ({at_corner:.2} dB at 10 kHz), got a worse {worst:.2} dB further up"
    );
}

#[test]
fn anti_alias_filter_leaves_the_speech_band_alone() {
    // A filter that cleans up the fold band by eating the voice is not a fix.
    for hz in [100.0, 300.0, 1_000.0, 2_000.0, 3_400.0] {
        let db = rejection_db(NATIVE_SR, hz);
        eprintln!("  {hz:>8.0} Hz → {db:6.3} dB");
        assert!(
            db.abs() < 0.5,
            "{hz} Hz is speech and must pass within 0.5 dB, got {db:.3} dB"
        );
    }
}

#[test]
fn airpods_rates_get_a_filter_too_and_16khz_input_gets_none() {
    // AirPods report 24 kHz normally and drop further on HFP. 24 kHz → 16 kHz is
    // still a decimation (8–12 kHz folds) so it is still filtered…
    let db = rejection_db(24_000, 10_000.0);
    eprintln!("anti-alias rejection at 10 kHz (24 kHz input): {db:.2} dB");
    assert!(db >= REQUIRED_REJECTION_DB);
    // …while an input already at the target rate has nothing to fold, and must
    // not be touched at all (no phase shift, no transient).
    assert!(LowPassCascade::for_decimation(TARGET_SR, TARGET_SR).is_none());
    assert!(LowPassCascade::for_decimation(8_000, TARGET_SR).is_none());
}

#[test]
fn linear_interpolation_alone_folds_a_10khz_tone_into_the_speech_band() {
    // THE NEGATIVE CONTROL — this is the defect, reproduced.
    //
    // A 10 kHz tone decimated 48 kHz → 16 kHz aliases to |10000 - 16000| =
    // 6000 Hz. With the pre-YV92 pure-linear path that 6 kHz ghost comes
    // through at nearly full level; with the shipped decimator it is gone.
    let input = tone(NATIVE_SR, 0.5, 10_000.0);
    let aliased = resample_linear(&input, NATIVE_SR, TARGET_SR);
    let clean = resample_decimate(&input, NATIVE_SR, TARGET_SR);
    assert_eq!(aliased.len(), clean.len(), "same conversion, same length");

    let ghost_before = component_amplitude(&aliased, TARGET_SR, 6_000.0);
    let ghost_after = component_amplitude(&clean, TARGET_SR, 6_000.0);
    let improvement_db = 20.0 * (ghost_before / ghost_after.max(1e-12)).log10();
    eprintln!(
        "6 kHz alias of a 10 kHz tone — linear: {ghost_before:.4}, anti-aliased: {ghost_after:.4} ({improvement_db:.1} dB better)"
    );
    assert!(
        ghost_before > 0.3,
        "the old path must actually exhibit the defect (got {ghost_before:.4}) or this test proves nothing"
    );
    assert!(
        improvement_db >= REQUIRED_REJECTION_DB,
        "the fix must remove the alias by ≥{REQUIRED_REJECTION_DB} dB, got {improvement_db:.1} dB"
    );
}

#[test]
fn the_cutoff_sits_below_the_target_nyquist_and_degenerate_filters_are_none() {
    assert!(
        ANTI_ALIAS_CUTOFF_HZ < TARGET_SR as f32 / 2.0,
        "the corner must be below the 8 kHz Nyquist everything downstream works at"
    );
    // The guards: a cutoff at or above the input Nyquist, a zero rate and a
    // zero Q all refuse to build rather than producing a garbage filter.
    assert!(Biquad::low_pass(0, ANTI_ALIAS_CUTOFF_HZ, 0.707).is_none());
    assert!(Biquad::low_pass(NATIVE_SR, NATIVE_SR as f32, 0.707).is_none());
    assert!(Biquad::low_pass(NATIVE_SR, ANTI_ALIAS_CUTOFF_HZ, 0.0).is_none());
    assert_eq!(
        LowPassCascade::butterworth(NATIVE_SR, ANTI_ALIAS_CUTOFF_HZ, 8)
            .expect("valid")
            .sections(),
        4
    );
}
