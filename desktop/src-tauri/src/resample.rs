//! Rate conversion and the filters that belong around it (YV92).
//!
//! Split out of `record.rs` for one reason: the decimation path is now the
//! thing an integration test has to be able to measure. Everything here is
//! pure, hardware-free and deterministic — a sine in, a sine out — so the
//! anti-alias claim is falsifiable from `tests/biquad_lowpass_response.rs`
//! rather than asserted in a comment.
//!
//! ## Why there is a filter here at all (plan finding OS-8)
//!
//! Capture runs at the device's native rate (48 kHz on a MacBook mic, 44.1 kHz
//! on plenty of USB interfaces, 24 kHz on AirPods) and everything downstream
//! runs at 16 kHz, so the capture path decimates. It used to do that with pure
//! linear interpolation and *no* lowpass ahead of the 3:1 rate reduction.
//! Linear interpolation's kernel is a triangle, i.e. a `sinc²` magnitude
//! response: at 10 kHz it attenuates by ≈1.3 dB and at 16 kHz by ≈3.3 dB before
//! that content folds down into the 0–8 kHz band. For a five-second, close-mic,
//! high-SNR dictation clip the ASR shrugs it off, which is why it never
//! surfaced. A three-hour far-field room recording — HVAC, fans, keyboards, all
//! with real energy above 8 kHz — is a different signal, and the folded noise
//! lands directly in the band the recognizer reads. Worse, it is invisible to
//! the eval harness's *interpretation*: the WER comes back bad and the model
//! gets the blame.
//!
//! So: an 8th-order Butterworth lowpass at [`ANTI_ALIAS_CUTOFF_HZ`] runs ahead
//! of the interpolator whenever the conversion actually reduces the rate. Why
//! 8th order and not the "at absolute minimum, a 2nd-order Butterworth" the
//! plan's fix line offers: a single biquad at 7.2 kHz gives only ≈6.7 dB at
//! 10 kHz, and the acceptance bar for this item is ≥20 dB at 10 kHz. Four
//! cascaded biquads measure **28.5 dB** at 10 kHz, 46.8 dB at 12 kHz and 65+ dB
//! from 14 kHz up (`tests/biquad_lowpass_response.rs` prints the table), while
//! staying flat to 0.000 dB across 100 Hz–3.4 kHz. It costs four multiply-adds
//! per sample on a thread that is not the audio callback.
//!
//! ## What did NOT change
//!
//! [`resample_linear`] is untouched and still the reference the streaming
//! resampler is proved equal to. Upsampling (16 kHz → 48 kHz for the RNNoise
//! round trip) never gets a filter — there is nothing to fold.

/// Anti-alias corner for a 16 kHz target: 7.2 kHz = 0.45 · 16 kHz, which leaves
/// the whole speech band flat and puts the stopband edge below the 8 kHz
/// Nyquist everything downstream of the mic works at.
pub const ANTI_ALIAS_CUTOFF_HZ: f32 = 7_200.0;

/// The anti-alias cutoff for an arbitrary target rate: the fixed 7.2 kHz corner
/// when decimating to 16 kHz (the only case the app ships), and 0.45 · target
/// otherwise so the filter can never sit above the new Nyquist.
fn cutoff_for(target_rate: u32) -> f32 {
    (0.45 * target_rate as f32).min(ANTI_ALIAS_CUTOFF_HZ)
}

/// Second-order (biquad) section, Direct Form I. The state lives in the struct
/// so the SAME filter can run over a whole clip at once (the batch/fallback
/// paths) or across the capture frames as they arrive (YV37) with identical
/// output — a property the tests assert rather than assume.
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    /// High-pass at `cutoff_hz` (Q = 1/√2, maximally flat). `None` for a
    /// degenerate rate/cutoff, which callers treat as "leave the audio alone".
    pub fn high_pass(sample_rate: u32, cutoff_hz: f32) -> Option<Self> {
        Self::rbj(
            sample_rate,
            cutoff_hz,
            std::f32::consts::FRAC_1_SQRT_2,
            true,
        )
    }

    /// Low-pass at `cutoff_hz` with an explicit `q` — explicit because a
    /// Butterworth cascade needs a different Q per section (see
    /// [`butterworth_qs`]). `None` for a degenerate rate/cutoff/Q.
    pub fn low_pass(sample_rate: u32, cutoff_hz: f32, q: f32) -> Option<Self> {
        Self::rbj(sample_rate, cutoff_hz, q, false)
    }

    /// RBJ cookbook biquad. One constructor for both shapes so the guards
    /// (degenerate rate, cutoff at/above Nyquist, non-finite coefficients) can
    /// never drift apart between them.
    fn rbj(sample_rate: u32, cutoff_hz: f32, q: f32, high_pass: bool) -> Option<Self> {
        if sample_rate == 0
            || !cutoff_hz.is_finite()
            || cutoff_hz <= 0.0
            || cutoff_hz >= sample_rate as f32 / 2.0
            || !q.is_finite()
            || q <= 0.0
        {
            return None;
        }
        let w0 = 2.0 * std::f32::consts::PI * cutoff_hz / sample_rate as f32;
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / (2.0 * q);
        let a0 = 1.0 + alpha;
        if a0 == 0.0 || !a0.is_finite() {
            return None;
        }
        let (b0, b1, b2) = if high_pass {
            ((1.0 + cos_w0) / 2.0, -(1.0 + cos_w0), (1.0 + cos_w0) / 2.0)
        } else {
            ((1.0 - cos_w0) / 2.0, 1.0 - cos_w0, (1.0 - cos_w0) / 2.0)
        };
        let filter = Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: (-2.0 * cos_w0) / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        };
        if [filter.b0, filter.b1, filter.b2, filter.a1, filter.a2]
            .iter()
            .any(|c| !c.is_finite())
        {
            return None;
        }
        Some(filter)
    }

    /// Filter a frame in place. Returns false if the biquad went non-finite: it
    /// then stops at that sample (the rest of the frame passes through
    /// unfiltered) and clears its state, so one bad sample can never poison the
    /// rest of the take — the batch caller falls back to the input outright.
    pub fn process(&mut self, frame: &mut [f32]) -> bool {
        for slot in frame.iter_mut() {
            let x0 = *slot;
            let y0 = self.b0 * x0 + self.b1 * self.x1 + self.b2 * self.x2
                - self.a1 * self.y1
                - self.a2 * self.y2;
            if !y0.is_finite() {
                self.x1 = 0.0;
                self.x2 = 0.0;
                self.y1 = 0.0;
                self.y2 = 0.0;
                return false;
            }
            self.x2 = self.x1;
            self.x1 = x0;
            self.y2 = self.y1;
            self.y1 = y0;
            *slot = y0;
        }
        true
    }
}

/// Section Qs for a Butterworth lowpass of even `order`, in the classic
/// `1 / (2·sin(π(2k+1)/2n))` form. For order 8 this is
/// `[2.5629, 0.9000, 0.6013, 0.5098]`.
fn butterworth_qs(order: usize) -> Vec<f32> {
    (0..order / 2)
        .map(|k| {
            let theta = std::f32::consts::PI * (2 * k + 1) as f32 / (2.0 * order as f32);
            1.0 / (2.0 * theta.sin())
        })
        .collect()
}

/// Butterworth order used ahead of decimation. See the module docs for why it
/// is not 2: the acceptance bar is ≥20 dB at 10 kHz and a single biquad at
/// 7.2 kHz gives ≈6.7 dB.
const ANTI_ALIAS_ORDER: usize = 8;

/// A cascade of biquad sections — the anti-alias lowpass. Stateful, so it runs
/// identically over one buffer or across a stream of capture frames.
pub struct LowPassCascade {
    sections: Vec<Biquad>,
}

impl LowPassCascade {
    /// Butterworth lowpass of `order` (rounded down to even) at `cutoff_hz`.
    /// `None` when the cutoff is degenerate for this rate — the caller then
    /// does no filtering, which is exactly right for "the cutoff is already
    /// above Nyquist, there is nothing up there to fold".
    pub fn butterworth(sample_rate: u32, cutoff_hz: f32, order: usize) -> Option<Self> {
        let sections: Vec<Biquad> = butterworth_qs(order)
            .into_iter()
            .map(|q| Biquad::low_pass(sample_rate, cutoff_hz, q))
            .collect::<Option<Vec<_>>>()?;
        if sections.is_empty() {
            return None;
        }
        Some(Self { sections })
    }

    /// The filter the capture path puts ahead of a rate REDUCTION, or `None`
    /// when `from → to` is not a reduction (upsampling and the identity case
    /// fold nothing, so they get no filter and no phase shift).
    pub fn for_decimation(from: u32, to: u32) -> Option<Self> {
        if from == 0 || to == 0 || from <= to {
            return None;
        }
        Self::butterworth(from, cutoff_for(to), ANTI_ALIAS_ORDER)
    }

    /// Filter a frame in place through every section. False if any section went
    /// non-finite (same degrade-to-passthrough contract as [`Biquad::process`]).
    pub fn process(&mut self, frame: &mut [f32]) -> bool {
        let mut ok = true;
        for section in self.sections.iter_mut() {
            ok &= section.process(frame);
        }
        ok
    }

    /// How many biquad sections are in the cascade (order / 2).
    pub fn sections(&self) -> usize {
        self.sections.len()
    }
}

/// Streaming resampler: fed arbitrary frames, produces exactly what the batch
/// function over the concatenated input would (asserted in the tests below).
/// The capture path streams through this; the fallback path runs the batch
/// function.
///
/// It emits an output sample as soon as both of its input neighbours have
/// arrived, keeping only the couple of input samples the next output still
/// needs (so a long hold costs nothing extra), and `finish` flushes the tail
/// with the same clamped last-neighbour rule the batch version uses.
///
/// YV92 adds two things: the anti-alias cascade ahead of the interpolator when
/// the conversion decimates, and [`StreamResampler::retune`] — because a ratio
/// must never survive an input format change (OS-9).
pub struct StreamResampler {
    /// Input rate the ratio was computed from. Read back by the format-change
    /// state machine to prove the ratio was actually retuned.
    from: u32,
    to: u32,
    /// Input samples per output sample (`from / to`).
    ratio: f64,
    /// The anti-alias lowpass, or `None` when this conversion cannot fold
    /// (upsample/identity) or was explicitly built linear-only.
    anti_alias: Option<LowPassCascade>,
    /// Reused filter scratch so a steady-state callback allocates nothing.
    scratch: Vec<f32>,
    /// Retained input, `pending[0]` being global input index `base`.
    pending: Vec<f32>,
    base: u64,
    /// Index of the next output sample to emit.
    next_out: u64,
    /// Total input samples seen so far.
    seen: u64,
}

impl StreamResampler {
    /// The capture path's resampler: anti-aliased whenever `from > to`.
    pub fn new(from: u32, to: u32) -> Self {
        let mut r = Self::linear_only(from, to);
        r.anti_alias = LowPassCascade::for_decimation(from, to);
        r
    }

    /// Pure linear interpolation, no lowpass — the pre-YV92 behaviour, kept as
    /// the reference the equivalence test holds the streaming path to and for
    /// conversions where filtering is not wanted.
    pub fn linear_only(from: u32, to: u32) -> Self {
        let ratio = if from == 0 || to == 0 {
            1.0
        } else {
            from as f64 / to as f64
        };
        Self {
            from,
            to,
            ratio,
            anti_alias: None,
            scratch: Vec::new(),
            pending: Vec::new(),
            base: 0,
            next_out: 0,
            seen: 0,
        }
    }

    /// The input rate this resampler is currently converting FROM.
    pub fn from_rate(&self) -> u32 {
        self.from
    }

    /// Input samples per output sample.
    pub fn ratio(&self) -> f64 {
        self.ratio
    }

    /// Whether an anti-alias filter is in front of the interpolator.
    pub fn is_anti_aliased(&self) -> bool {
        self.anti_alias.is_some()
    }

    /// YV92/OS-9 — the input format changed (AirPods engaged, HFP renegotiated,
    /// the default input device swapped): throw the whole conversion away and
    /// build a new one. Every bit of state goes, filter included, because all of
    /// it belongs to the old rate. Callers `finish` into their output buffer
    /// FIRST, so the tail captured at the old rate is not lost.
    pub fn retune(&mut self, from: u32, to: u32) {
        *self = Self::new(from, to);
    }

    /// Feed one frame, appending every output sample it completes.
    pub fn push(&mut self, frame: &[f32], out: &mut Vec<f32>) {
        if frame.is_empty() {
            return;
        }
        if self.anti_alias.is_none() {
            self.interpolate(frame, out);
            return;
        }
        // Filter into reusable scratch — `mem::take` swaps in an empty Vec
        // rather than allocating, and the capacity comes straight back.
        let mut scratch = std::mem::take(&mut self.scratch);
        scratch.clear();
        scratch.extend_from_slice(frame);
        if let Some(filter) = self.anti_alias.as_mut() {
            // A non-finite section leaves the frame partially filtered rather
            // than dropping it: the same never-lose-audio posture the high-pass
            // takes. The interpolator's own guards handle the rest.
            let _ = filter.process(&mut scratch);
        }
        self.interpolate(&scratch, out);
        self.scratch = scratch;
    }

    fn interpolate(&mut self, frame: &[f32], out: &mut Vec<f32>) {
        self.pending.extend_from_slice(frame);
        self.seen += frame.len() as u64;
        // Only emit outputs the batch version would also emit for the input seen
        // so far — the trailing partial sample is dropped there, and `finish`
        // decides it here.
        let ready = (self.seen as f64 / self.ratio).floor() as u64;
        while self.next_out < ready {
            let src = self.next_out as f64 * self.ratio;
            let i0 = src.floor() as u64;
            // Needs its right-hand neighbour; wait for the next frame otherwise.
            if i0 + 1 >= self.seen {
                break;
            }
            let k = (i0 - self.base) as usize;
            let t = (src - i0 as f64) as f32;
            out.push(self.pending[k] * (1.0 - t) + self.pending[k + 1] * t);
            self.next_out += 1;
        }
        // Release everything before the next output's left neighbour.
        let next_src = self.next_out as f64 * self.ratio;
        let keep_from = next_src.floor() as u64;
        let drop = keep_from.saturating_sub(self.base) as usize;
        let drop = drop.min(self.pending.len());
        if drop > 0 {
            self.pending.drain(..drop);
            self.base += drop as u64;
        }
    }

    /// Flush the tail (the last output sample clamps to the final input sample,
    /// exactly like [`resample_linear`]).
    pub fn finish(&mut self, out: &mut Vec<f32>) {
        let total = (self.seen as f64 / self.ratio).floor() as u64;
        while self.next_out < total {
            let src = self.next_out as f64 * self.ratio;
            let i0 = src.floor() as u64;
            let k = (i0 - self.base) as usize;
            if k >= self.pending.len() {
                break;
            }
            let k1 = (k + 1).min(self.pending.len() - 1);
            let t = (src - i0 as f64) as f32;
            out.push(self.pending[k] * (1.0 - t) + self.pending[k1] * t);
            self.next_out += 1;
        }
    }

    /// How much input the resampler is holding — the "no backlog" assertion.
    #[cfg(test)]
    fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// The output rate, for symmetry with [`Self::from_rate`].
    pub fn to_rate(&self) -> u32 {
        self.to
    }
}

/// Batch linear interpolation. Unchanged by YV92 and deliberately so: it is the
/// reference [`StreamResampler::linear_only`] is proved identical to, and it is
/// what the RNNoise round trip (16 kHz → 48 kHz → 16 kHz) uses in the direction
/// that cannot fold.
pub fn resample_linear(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if input.is_empty() || from == 0 {
        return Vec::new();
    }
    let ratio = from as f64 / to as f64;
    let out_len = ((input.len() as f64) / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src = i as f64 * ratio;
        let i0 = src.floor() as usize;
        let i1 = (i0 + 1).min(input.len() - 1);
        let t = (src - i0 as f64) as f32;
        out.push(input[i0] * (1.0 - t) + input[i1] * t);
    }
    out
}

/// Batch decimation the way YV92 says it must happen: anti-alias filter first,
/// interpolate second. Identical to [`resample_linear`] when `from <= to`
/// (nothing folds, so nothing is filtered).
pub fn resample_decimate(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    let Some(mut filter) = LowPassCascade::for_decimation(from, to) else {
        return resample_linear(input, from, to);
    };
    let mut filtered = input.to_vec();
    if !filter.process(&mut filtered) {
        // The filter degenerated — resample the untouched input rather than a
        // half-filtered buffer. Aliased is bad; garbage is worse.
        return resample_linear(input, from, to);
    }
    resample_linear(&filtered, from, to)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NATIVE_SR: u32 = 48_000;
    const SR: u32 = 16_000;

    fn tone_at(rate: u32, seconds: f32, hz: f32, amp: f32) -> Vec<f32> {
        let n = (rate as f32 * seconds) as usize;
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin() * amp)
            .collect()
    }

    /// Split a buffer into frames of the given repeating sizes.
    fn frames<'a>(input: &'a [f32], sizes: &[usize]) -> Vec<&'a [f32]> {
        let mut out = Vec::new();
        let mut rest = input;
        let mut i = 0;
        while !rest.is_empty() {
            let n = sizes[i % sizes.len()].min(rest.len());
            let (head, tail) = rest.split_at(n);
            out.push(head);
            rest = tail;
            i += 1;
        }
        out
    }

    /// RMS of a steady tone, skipping the filter's settling transient.
    fn settled_rms(samples: &[f32], skip: usize) -> f32 {
        let body = &samples[skip.min(samples.len())..];
        if body.is_empty() {
            return 0.0;
        }
        (body.iter().map(|s| (s * s) as f64).sum::<f64>() / body.len() as f64).sqrt() as f32
    }

    #[test]
    fn stream_resampler_matches_batch_for_every_framing() {
        // The pre-YV92 contract, kept exactly: the LINEAR-ONLY streaming path is
        // sample-identical to the batch linear resample, whatever the framing.
        for (from, to) in [
            (48_000u32, 16_000u32),
            (44_100, 16_000),
            (16_000, 16_000),
            (8_000, 16_000),
        ] {
            let input = tone_at(from, 0.05, 220.0, 0.5);
            let batch = resample_linear(&input, from, to);
            for sizes in [vec![512], vec![480, 137, 1, 999], vec![1], vec![7, 3]] {
                let mut stream = StreamResampler::linear_only(from, to);
                let mut out = Vec::new();
                for frame in frames(&input, &sizes) {
                    stream.push(frame, &mut out);
                }
                stream.finish(&mut out);
                assert_eq!(
                    out.len(),
                    batch.len(),
                    "{from}→{to} in {sizes:?}-sized frames must yield the batch length"
                );
                assert!(
                    out.iter().zip(&batch).all(|(a, b)| a == b),
                    "{from}→{to} in {sizes:?}-sized frames must be sample-identical to the batch resample"
                );
            }
        }
    }

    #[test]
    fn anti_aliased_stream_resampler_matches_the_batch_decimator() {
        // Same equivalence, now for the filtered path: the cascade carries its
        // state across callbacks, so streaming and batch must agree bit for bit
        // — otherwise every frame boundary would restart the filter and click.
        for (from, to) in [(48_000u32, 16_000u32), (44_100, 16_000), (24_000, 16_000)] {
            let input = tone_at(from, 0.05, 900.0, 0.5);
            let batch = resample_decimate(&input, from, to);
            for sizes in [vec![512], vec![480, 137, 1, 999], vec![7, 3]] {
                let mut stream = StreamResampler::new(from, to);
                assert!(stream.is_anti_aliased(), "{from}→{to} must be filtered");
                let mut out = Vec::new();
                for frame in frames(&input, &sizes) {
                    stream.push(frame, &mut out);
                }
                stream.finish(&mut out);
                assert_eq!(out.len(), batch.len(), "{from}→{to} {sizes:?}");
                assert!(
                    out.iter().zip(&batch).all(|(a, b)| a == b),
                    "{from}→{to} in {sizes:?}-sized frames must equal the batch decimation"
                );
            }
        }
    }

    #[test]
    fn upsampling_and_identity_conversions_are_never_filtered() {
        // Nothing folds when the rate goes up (or stays put), so nothing is
        // filtered — the RNNoise round trip must not pick up a phase shift.
        for (from, to) in [(16_000u32, 48_000u32), (16_000, 16_000), (8_000, 16_000)] {
            let r = StreamResampler::new(from, to);
            assert!(!r.is_anti_aliased(), "{from}→{to} must not be filtered");
            assert!(LowPassCascade::for_decimation(from, to).is_none());
            let input = tone_at(from, 0.02, 500.0, 0.4);
            assert_eq!(
                resample_decimate(&input, from, to),
                resample_linear(&input, from, to),
                "{from}→{to} decimate must be exactly the linear resample"
            );
        }
    }

    #[test]
    fn stream_resampler_emits_during_capture_and_holds_no_backlog() {
        // The point of YV37: 16 kHz samples exist WHILE the user speaks, not
        // after release — and the retained input never grows with hold length.
        // Re-asserted on the YV92 (filtered) resampler, because the filter must
        // not introduce buffering of its own.
        let input = tone_at(NATIVE_SR, 0.5, 220.0, 0.4);
        let mut stream = StreamResampler::new(NATIVE_SR, SR);
        let mut out = Vec::new();
        let mut after_first = 0usize;
        for (i, frame) in frames(&input, &[512]).into_iter().enumerate() {
            stream.push(frame, &mut out);
            if i == 0 {
                after_first = out.len();
            }
            assert!(
                stream.pending_len() <= 512 + 4,
                "retained input must stay bounded, got {}",
                stream.pending_len()
            );
        }
        assert!(
            after_first > 100,
            "the first 512-frame callback should already produce ~170 samples at 16 kHz, got {after_first}"
        );
        stream.finish(&mut out);
        let expected = input.len() / 3; // 48 kHz → 16 kHz
        assert!(
            out.len().abs_diff(expected) <= 2,
            "expected ~{expected} samples, got {}",
            out.len()
        );
    }

    #[test]
    fn anti_alias_cascade_is_eighth_order_and_flat_in_the_speech_band() {
        let c = LowPassCascade::for_decimation(NATIVE_SR, SR).expect("48→16 is a decimation");
        assert_eq!(c.sections(), 4, "8th order == 4 biquad sections");
        for hz in [100.0, 300.0, 1_000.0, 3_400.0] {
            let input = tone_at(NATIVE_SR, 0.2, hz, 0.4);
            let mut filtered = input.clone();
            let mut f =
                LowPassCascade::for_decimation(NATIVE_SR, SR).expect("48→16 is a decimation");
            assert!(f.process(&mut filtered));
            let skip = NATIVE_SR as usize / 20; // 50 ms of settling
            let db = 20.0 * (settled_rms(&filtered, skip) / settled_rms(&input, skip)).log10();
            assert!(
                db.abs() < 0.05,
                "{hz} Hz must pass untouched, got {db:.3} dB"
            );
        }
    }

    #[test]
    fn retune_replaces_the_ratio_and_the_filter_wholesale() {
        // OS-9: a ratio must never survive a format change.
        let mut r = StreamResampler::new(48_000, 16_000);
        let mut out = Vec::new();
        r.push(&tone_at(48_000, 0.01, 440.0, 0.3), &mut out);
        assert_eq!(r.from_rate(), 48_000);
        assert!((r.ratio() - 3.0).abs() < 1e-9);
        r.retune(24_000, 16_000);
        assert_eq!(r.from_rate(), 24_000);
        assert!((r.ratio() - 1.5).abs() < 1e-9);
        assert!(r.is_anti_aliased());
        assert_eq!(r.pending_len(), 0, "no old-rate input may survive a retune");
        // And a retune down to the target rate drops the filter entirely.
        r.retune(16_000, 16_000);
        assert!(!r.is_anti_aliased());
    }

    #[test]
    fn degenerate_rates_never_build_a_filter_or_panic() {
        assert!(LowPassCascade::for_decimation(0, 16_000).is_none());
        assert!(LowPassCascade::for_decimation(48_000, 0).is_none());
        assert!(LowPassCascade::butterworth(48_000, 0.0, 8).is_none());
        assert!(LowPassCascade::butterworth(48_000, 24_000.0, 8).is_none());
        assert!(Biquad::low_pass(48_000, 7_200.0, 0.0).is_none());
        assert!(resample_decimate(&[], 48_000, 16_000).is_empty());
        let mut r = StreamResampler::new(0, 0);
        let mut out = Vec::new();
        r.push(&[0.1, 0.2], &mut out);
        r.finish(&mut out);
    }
}
