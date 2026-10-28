//! yap24-NT11 — the AEC3 pre-pass: cancel the far side out of the mic track.
//!
//! On built-in speakers a call's far side leaks into the microphone, and the
//! mic ("Me") track then transcribes the other people a second time (audit
//! issue #1: 0.71-0.89 of the call track's trigrams inside the mic track). The
//! system-tap track (t1) is a clean copy of exactly what the speakers played,
//! so it is the far-end reference an echo canceller needs, and it is already
//! on disk next to the mic WAV, sample-aligned (measured lag -1.7 .. 7.1 ms,
//! constant across the file).
//!
//! Two functions carry the whole thing:
//!
//! * [`aec_coupling`] — does the mic actually hear the speakers? GCC-PHAT over
//!   the windows where the call is playing; `Some(lag_ms)` only when a
//!   confident lag is found in at least [`COUPLING_MIN_SHARE`] of them. A
//!   headphones call has no acoustic path from t1 into t0 and returns `None`,
//!   so its mic is read raw and untouched.
//! * [`aec_prepass`] — streams both WAVs through the pure-Rust `aec3` crate
//!   (noise suppression and AGC off) in 10 ms frames and writes
//!   `<audio>.t0.aec.wav` beside the raw WAV. Memory is bounded (one frame
//!   plus the canceller's own state); there is never a whole-meeting buffer.
//!   It is written to a temp name and renamed, so a crash leaves no half file,
//!   and a second run is a no-op. The raw WAV is never modified: a
//!   re-transcribe re-derives the cleaned file from it.
//!
//! The reference is read [`REFERENCE_LEAD_MS`] ahead so a slightly negative
//! measured lag (mic ahead of tap, -1.7 ms on one real call) stays causal for
//! the canceller.
//!
//! What AEC cannot fix (a tap hole, a far side that never reached the tap) is
//! handled after transcription by [`residue_verdict`] and
//! [`crate::meetings::speaker_label_for_span`].

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Instant;

use aec3::nodes::audio::AudioFormat;
use aec3::pipelines::linear;
use rustfft::num_complex::Complex;
use rustfft::FftPlanner;

/// The reference is read this far ahead of the capture (see module docs).
pub const REFERENCE_LEAD_MS: u32 = 20;
/// The share of call-playing windows that must show a confident lag.
pub const COUPLING_MIN_SHARE: f64 = 0.30;
/// GCC-PHAT search range either side of zero.
pub const COUPLING_MAX_LAG_MS: f64 = 100.0;
/// Peak-to-mean ratio of the PHAT correlation over the search range above
/// which a window's lag is "confident". Two unrelated signals peak at about
/// 4-5x the mean over a few thousand lags; a real acoustic path peaks far
/// higher.
pub const COUPLING_PEAK_RATIO: f32 = 9.0;
/// The system track counts as "playing" in a 1 s window above this RMS
/// (about -46 dBFS).
pub const PLAYING_RMS: f32 = 0.005;
/// Below this RMS the system track is silent for a window (about -60 dBFS).
pub const SILENT_RMS: f32 = 0.001;
/// Fewest call-playing windows a coupling verdict needs.
pub const COUPLING_MIN_WINDOWS: usize = 3;

/// The derived file for a mic WAV: `X.t0.wav` -> `X.t0.aec.wav`.
pub fn aec_path_for(mic_wav: &Path) -> PathBuf {
    let stem = mic_wav
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    mic_wav.with_file_name(format!("{stem}.aec.wav"))
}

/// Every file DERIVED from one captured WAV that must live and die with it:
/// the AEC output (track 0) and the per-track host-time anchors
/// (`X.tN.index.jsonl`, kept by finalize since NT11). yap24-NT1's purge and the
/// meeting-delete cascade both unlink these with the raw WAV; neither is ever
/// kept alone.
pub fn derived_paths_for_wav(wav: &Path) -> Vec<PathBuf> {
    let stem = wav
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    if stem.is_empty() || stem.ends_with(".aec") {
        return Vec::new();
    }
    let mut out = vec![
        aec_path_for(wav),
        wav.with_file_name(format!("{stem}.index.jsonl")),
    ];
    // Track 0 owns BOTH anchor spellings: the legacy `X.index.jsonl` and the
    // per-track `X.t0.index.jsonl`. A `.t0` WAV adds the legacy one; a WAV with
    // no `.tN` suffix at all IS track 0 under its legacy name (a single-file
    // mic recording), so it adds the per-track one.
    if let Some(id) = stem.strip_suffix(".t0") {
        out.push(wav.with_file_name(format!("{id}.index.jsonl")));
    } else if track_suffix(&stem).is_none() {
        out.push(wav.with_file_name(format!("{stem}.t0.index.jsonl")));
    }
    out
}

/// The track number of a `X.tN` stem, or `None` when the stem has no `.tN`.
fn track_suffix(stem: &str) -> Option<u32> {
    let (_, tail) = stem.rsplit_once('.')?;
    let digits = tail.strip_prefix('t')?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// A mono WAV read as f32 samples, one at a time.
struct MonoStream {
    rate: u32,
    samples: Box<dyn Iterator<Item = f32>>,
}

fn open_mono(path: &Path) -> Result<MonoStream, String> {
    let reader = hound::WavReader::open(path).map_err(|e| format!("open {}: {e}", base(path)))?;
    let spec = reader.spec();
    if spec.channels != 1 {
        return Err(format!(
            "{}: {} channels, expected mono",
            base(path),
            spec.channels
        ));
    }
    let samples: Box<dyn Iterator<Item = f32>> = match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Float, 32) => {
            Box::new(reader.into_samples::<f32>().map(|s| s.unwrap_or(0.0)))
        }
        (hound::SampleFormat::Int, 16) => Box::new(
            reader
                .into_samples::<i16>()
                .map(|s| s.map(|v| v as f32 / 32768.0).unwrap_or(0.0)),
        ),
        (hound::SampleFormat::Int, bits) if bits <= 32 => {
            let scale = (1i64 << (bits - 1)) as f32;
            Box::new(
                reader
                    .into_samples::<i32>()
                    .map(move |s| s.map(|v| v as f32 / scale).unwrap_or(0.0)),
            )
        }
        (fmt, bits) => return Err(format!("{}: unsupported {fmt:?}/{bits}", base(path))),
    };
    Ok(MonoStream {
        rate: spec.sample_rate,
        samples,
    })
}

/// File name only — never a full user path in a log line or an error.
fn base(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

fn fill(stream: &mut MonoStream, buf: &mut [f32]) -> usize {
    let mut n = 0;
    for slot in buf.iter_mut() {
        match stream.samples.next() {
            Some(v) => {
                *slot = v;
                n += 1;
            }
            None => *slot = 0.0,
        }
    }
    n
}

/// GCC-PHAT lag of `mic` behind `sys` in samples, with its peak-to-mean
/// ratio over the search range.
fn gcc_phat(planner: &mut FftPlanner<f32>, mic: &[f32], sys: &[f32], max_lag: usize) -> (i64, f32) {
    let n = (mic.len().max(sys.len()) * 2).next_power_of_two();
    let fwd = planner.plan_fft_forward(n);
    let inv = planner.plan_fft_inverse(n);
    let mut a: Vec<Complex<f32>> = (0..n)
        .map(|i| Complex::new(*mic.get(i).unwrap_or(&0.0), 0.0))
        .collect();
    let mut b: Vec<Complex<f32>> = (0..n)
        .map(|i| Complex::new(*sys.get(i).unwrap_or(&0.0), 0.0))
        .collect();
    fwd.process(&mut a);
    fwd.process(&mut b);
    for (x, y) in a.iter_mut().zip(&b) {
        let c = *x * y.conj();
        let m = c.norm();
        *x = if m > 1e-12 {
            c / m
        } else {
            Complex::new(0.0, 0.0)
        };
    }
    inv.process(&mut a);
    let mut best = (0i64, 0.0f32);
    let mut sum = 0.0f32;
    let mut count = 0usize;
    for k in 0..=max_lag.min(n / 2 - 1) {
        for (lag, idx) in [(k as i64, k), (-(k as i64), (n - k) % n)] {
            if lag < 0 && k == 0 {
                continue;
            }
            let v = a[idx].re.abs();
            sum += v;
            count += 1;
            if v > best.1 {
                best = (lag, v);
            }
        }
    }
    let mean = if count > 0 { sum / count as f32 } else { 0.0 };
    let ratio = if mean > 0.0 { best.1 / mean } else { 0.0 };
    (best.0, ratio)
}

/// Is there an acoustic path from the system track into the mic? `Some(lag_ms)`
/// (mic behind tap, median over confident windows) only when at least
/// [`COUPLING_MIN_SHARE`] of the windows where the system track is playing show
/// a confident GCC-PHAT peak. Streaming: one second of each track at a time.
pub fn aec_coupling(mic: &Path, sys: &Path) -> Option<f64> {
    let started = Instant::now();
    let (mut m, mut s) = match (open_mono(mic), open_mono(sys)) {
        (Ok(m), Ok(s)) => (m, s),
        (Err(e), _) | (_, Err(e)) => {
            log::warn!("meeting_aec coupling_failed reason=\"{e}\"");
            return None;
        }
    };
    if m.rate != s.rate {
        log::warn!(
            "meeting_aec coupling_failed reason=rate_mismatch mic_hz={} sys_hz={}",
            m.rate,
            s.rate
        );
        return None;
    }
    let win = m.rate as usize;
    let max_lag = (COUPLING_MAX_LAG_MS / 1000.0 * m.rate as f64) as usize;
    let mut planner = FftPlanner::<f32>::new();
    let (mut mb, mut sb) = (vec![0.0f32; win], vec![0.0f32; win]);
    let mut playing = 0usize;
    let mut lags: Vec<f64> = Vec::new();
    loop {
        let got_m = fill(&mut m, &mut mb);
        let got_s = fill(&mut s, &mut sb);
        if got_m == 0 && got_s == 0 {
            break;
        }
        if rms(&sb) >= PLAYING_RMS {
            playing += 1;
            if rms(&mb) >= SILENT_RMS {
                let (lag, ratio) = gcc_phat(&mut planner, &mb, &sb, max_lag);
                if ratio >= COUPLING_PEAK_RATIO {
                    lags.push(lag as f64 * 1000.0 / m.rate as f64);
                }
            }
        }
        if got_m < win && got_s < win {
            break;
        }
    }
    let share = if playing > 0 {
        lags.len() as f64 / playing as f64
    } else {
        0.0
    };
    let verdict = if playing >= COUPLING_MIN_WINDOWS && share >= COUPLING_MIN_SHARE {
        lags.sort_by(|a, b| a.total_cmp(b));
        Some(lags[lags.len() / 2])
    } else {
        None
    };
    log::info!(
        "meeting_aec coupling playing_windows={playing} confident_windows={} share={share:.2} lag_ms={} wall_ms={}",
        lags.len(),
        verdict.map(|l| format!("{l:.1}")).unwrap_or_else(|| "none".into()),
        started.elapsed().as_millis()
    );
    verdict
}

/// What one pre-pass did, in numbers only.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct AecReport {
    /// The derived file was already there: nothing was processed.
    pub skipped_existing: bool,
    pub seconds: f64,
    pub wall_ms: u128,
    /// 1 s windows where the system track was playing.
    pub call_windows: usize,
    /// 1 s windows where the system track was silent and the mic was not.
    pub local_windows: usize,
    /// Mic energy removed on call-playing windows, dB (input / output).
    pub call_atten_p50_db: f64,
    pub call_atten_p25_db: f64,
    pub call_atten_p90_db: f64,
    /// Mic energy lost on local-only windows, dB (should be ~0).
    pub local_loss_p50_db: f64,
    pub local_loss_p90_db: f64,
}

fn percentile(v: &mut [f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let idx = ((v.len() - 1) as f64 * p).round() as usize;
    v[idx.min(v.len() - 1)]
}

/// The AEC3 pre-pass (see module docs). Writes `out_wav` once; a second call
/// with the file present is a no-op that reports `skipped_existing`.
pub fn aec_prepass(mic_wav: &Path, sys_wav: &Path, out_wav: &Path) -> Result<AecReport, String> {
    if out_wav.exists() {
        log::info!(
            "meeting_aec prepass_skip file={} reason=exists",
            base(out_wav)
        );
        return Ok(AecReport {
            skipped_existing: true,
            ..AecReport::default()
        });
    }
    let started = Instant::now();
    log::info!(
        "meeting_aec prepass_start mic={} sys={}",
        base(mic_wav),
        base(sys_wav)
    );
    let result = run_prepass(mic_wav, sys_wav, out_wav, started);
    match &result {
        Ok(r) => log::info!(
            "meeting_aec prepass_finish file={} seconds={:.1} wall_ms={} call_windows={} local_windows={} call_atten_p50_db={:.1} call_atten_p25_db={:.1} local_loss_p90_db={:.2}",
            base(out_wav),
            r.seconds,
            r.wall_ms,
            r.call_windows,
            r.local_windows,
            r.call_atten_p50_db,
            r.call_atten_p25_db,
            r.local_loss_p90_db
        ),
        Err(e) => log::error!("meeting_aec prepass_failed file={} reason=\"{e}\"", base(out_wav)),
    }
    result
}

fn run_prepass(
    mic_wav: &Path,
    sys_wav: &Path,
    out_wav: &Path,
    started: Instant,
) -> Result<AecReport, String> {
    let mut mic = open_mono(mic_wav)?;
    let mut sys = open_mono(sys_wav)?;
    if mic.rate != sys.rate {
        return Err(format!(
            "rate mismatch: mic {} Hz, system {} Hz",
            mic.rate, sys.rate
        ));
    }
    if !(16_000..=48_000).contains(&mic.rate) {
        return Err(format!("AEC3 needs 16-48 kHz, got {} Hz", mic.rate));
    }
    let rate = mic.rate;
    let format = AudioFormat::ten_ms(rate, 1);
    let frame = format.sample_count();
    let mut pipeline = linear::builder(format, format)
        .enable_noise_suppression(false)
        .enable_gain_controller2(false)
        .initial_delay_ms(REFERENCE_LEAD_MS as i32)
        .build()
        .map_err(|e| format!("aec3 pipeline: {e:?}"))?;

    // Read the reference REFERENCE_LEAD_MS ahead.
    let lead = (rate as usize * REFERENCE_LEAD_MS as usize) / 1000;
    for _ in 0..lead {
        if sys.samples.next().is_none() {
            break;
        }
    }

    let tmp = out_wav.with_file_name(format!("{}.partial", base(out_wav)));
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer =
        hound::WavWriter::create(&tmp, spec).map_err(|e| format!("create {}: {e}", base(&tmp)))?;

    let (mut cap, mut ren, mut out) = (
        vec![0.0f32; frame],
        vec![0.0f32; frame],
        vec![0.0f32; frame],
    );
    // Per 1 s window energies for the numbers the eval and BUDGETS.md read.
    let win_frames = 100usize;
    let (mut e_in, mut e_out, mut e_sys, mut wf) = (0.0f64, 0.0f64, 0.0f64, 0usize);
    let (mut call_db, mut local_db) = (Vec::new(), Vec::new());
    let mut total = 0usize;
    let close_window = |e_in: f64,
                        e_out: f64,
                        e_sys: f64,
                        n: usize,
                        call_db: &mut Vec<f64>,
                        local_db: &mut Vec<f64>| {
        if n == 0 {
            return;
        }
        let sys_rms = (e_sys / n as f64).sqrt() as f32;
        let mic_rms = (e_in / n as f64).sqrt() as f32;
        let db = 10.0 * ((e_in + 1e-12) / (e_out + 1e-12)).log10();
        if sys_rms >= PLAYING_RMS && mic_rms >= SILENT_RMS {
            call_db.push(db);
        } else if sys_rms < SILENT_RMS && mic_rms >= PLAYING_RMS {
            local_db.push(db);
        }
    };
    let result = (|| -> Result<(), String> {
        loop {
            let got = fill(&mut mic, &mut cap);
            if got == 0 {
                break;
            }
            fill(&mut sys, &mut ren);
            pipeline
                .handle_render_frame(&ren)
                .map_err(|e| format!("aec3 render: {e:?}"))?;
            let produced = pipeline
                .process_capture_frame(&cap, &mut out)
                .map_err(|e| format!("aec3 capture: {e:?}"))?;
            if !produced {
                out.copy_from_slice(&cap);
            }
            for i in 0..got {
                let v = (out[i].clamp(-1.0, 1.0) * 32767.0).round() as i16;
                writer.write_sample(v).map_err(|e| format!("write: {e}"))?;
                e_in += (cap[i] as f64).powi(2);
                e_out += (out[i] as f64).powi(2);
                e_sys += (ren[i] as f64).powi(2);
            }
            total += got;
            wf += got;
            if wf >= win_frames * frame {
                close_window(e_in, e_out, e_sys, wf, &mut call_db, &mut local_db);
                (e_in, e_out, e_sys, wf) = (0.0, 0.0, 0.0, 0);
            }
            if got < frame {
                break;
            }
        }
        close_window(e_in, e_out, e_sys, wf, &mut call_db, &mut local_db);
        Ok(())
    })();
    let finalized = writer.finalize().map_err(|e| format!("finalize: {e}"));
    if let Err(e) = result.and(finalized) {
        if let Err(rm) = std::fs::remove_file(&tmp) {
            log::warn!(
                "meeting_aec partial_cleanup_failed file={} reason=\"{rm}\"",
                base(&tmp)
            );
        }
        return Err(e);
    }
    std::fs::rename(&tmp, out_wav).map_err(|e| format!("rename {}: {e}", base(out_wav)))?;
    Ok(AecReport {
        skipped_existing: false,
        seconds: total as f64 / rate as f64,
        wall_ms: started.elapsed().as_millis(),
        call_windows: call_db.len(),
        local_windows: local_db.len(),
        call_atten_p50_db: percentile(&mut call_db, 0.5),
        call_atten_p25_db: percentile(&mut call_db, 0.25),
        call_atten_p90_db: percentile(&mut call_db, 0.9),
        local_loss_p50_db: percentile(&mut local_db, 0.5),
        local_loss_p90_db: percentile(&mut local_db, 0.9),
    })
}

/// The track-0 audio the meeting pipeline should decode. The cleaned file only
/// for a VIRTUAL meeting with a system track AND measured coupling; a
/// headphones call (no coupling), an in-person meeting or any pre-pass failure
/// reads the raw mic unchanged — the failure is logged, never fatal, because
/// the raw transcript is still the user's meeting.
pub fn mic_source_for(
    meeting_id: &str,
    is_virtual: bool,
    mic: &Path,
    sys: Option<&Path>,
) -> PathBuf {
    let Some(sys) = sys.filter(|_| is_virtual) else {
        return mic.to_path_buf();
    };
    let out = aec_path_for(mic);
    if out.exists() {
        log::info!(
            "meeting_aec source meeting={meeting_id} track=0 file={} reason=existing",
            base(&out)
        );
        return out;
    }
    match aec_coupling(mic, sys) {
        None => {
            log::info!(
                "meeting_aec source meeting={meeting_id} track=0 file={} reason=no_coupling",
                base(mic)
            );
            mic.to_path_buf()
        }
        Some(lag_ms) => match aec_prepass(mic, sys, &out) {
            Ok(_) => {
                log::info!(
                    "meeting_aec source meeting={meeting_id} track=0 file={} reason=aec lag_ms={lag_ms:.1}",
                    base(&out)
                );
                out
            }
            Err(e) => {
                log::warn!("meeting_aec source meeting={meeting_id} track=0 file={} reason=prepass_failed error=\"{e}\"", base(mic));
                mic.to_path_buf()
            }
        },
    }
}

// ---------------------------------------------------------------------------
// The text fallback, after AEC (step 4b)
// ---------------------------------------------------------------------------

/// A decoded word with its meeting-time span.
#[derive(Debug, Clone, PartialEq)]
pub struct TimedWord {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

/// A mic run that matches a system run within this many seconds is echo.
pub const RESIDUE_WINDOW_SECONDS: f64 = 1.5;
/// Fewest CONSECUTIVE matching words that count as echo residue. A single
/// shared word ("yeah") is never evidence.
pub const RESIDUE_MIN_RUN: usize = 3;

const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "but", "by", "for", "from", "i", "if", "in", "is",
    "it", "its", "it's", "of", "on", "or", "so", "that", "the", "this", "to", "uh", "um", "was",
    "we", "with", "you", "yeah", "oh", "okay", "ok", "like", "just", "mm", "hmm", "right",
];

fn fold(w: &str) -> String {
    crate::meeting_echo::echo_words(w).join("")
}

/// The mic word ranges that are echo residue: runs of at least
/// [`RESIDUE_MIN_RUN`] consecutive folded mic words that equal a consecutive
/// system run, each pair within [`RESIDUE_WINDOW_SECONDS`].
pub fn echo_residue_runs(mic: &[TimedWord], sys: &[TimedWord]) -> Vec<Range<usize>> {
    let mf: Vec<String> = mic.iter().map(|w| fold(&w.text)).collect();
    let sf: Vec<String> = sys.iter().map(|w| fold(&w.text)).collect();
    let mut runs = Vec::new();
    let mut i = 0;
    while i < mic.len() {
        let mut best = 0usize;
        if !mf[i].is_empty() {
            for j in 0..sys.len() {
                if sf[j] != mf[i] || (sys[j].start - mic[i].start).abs() > RESIDUE_WINDOW_SECONDS {
                    continue;
                }
                let mut k = 0;
                while i + k < mic.len()
                    && j + k < sys.len()
                    && !mf[i + k].is_empty()
                    && mf[i + k] == sf[j + k]
                    && (sys[j + k].start - mic[i + k].start).abs() <= RESIDUE_WINDOW_SECONDS
                {
                    k += 1;
                }
                best = best.max(k);
            }
        }
        if best >= RESIDUE_MIN_RUN {
            runs.push(i..i + best);
            i += best;
        } else {
            i += 1;
        }
    }
    runs
}

/// What happens to one mic segment span under the residue rule.
#[derive(Debug, Clone, PartialEq)]
pub enum SpanVerdict {
    /// Nothing matched: the span is the user's, untouched.
    Keep,
    /// Echo runs were removed; what remains is real speech, flagged mixed.
    KeepMixed { residue: Vec<String> },
    /// The span was echo: persist it flagged `suppressed_as_echo = 1`.
    Drop,
}

/// The residue rule for one mic span. `text_may_drop` is true only where AEC
/// ran or the span has no system audio — text agreement is never a drop
/// condition anywhere else.
pub fn residue_verdict(span: &[TimedWord], sys: &[TimedWord], text_may_drop: bool) -> SpanVerdict {
    if !text_may_drop {
        return SpanVerdict::Keep;
    }
    let runs = echo_residue_runs(span, sys);
    if runs.is_empty() {
        return SpanVerdict::Keep;
    }
    let residue: Vec<String> = span
        .iter()
        .enumerate()
        .filter(|(i, _)| !runs.iter().any(|r| r.contains(i)))
        .map(|(_, w)| w.text.clone())
        .collect();
    let content = residue
        .iter()
        .map(|w| fold(w))
        .filter(|w| !w.is_empty() && !STOPWORDS.contains(&w.as_str()))
        .count();
    if content <= 1 {
        SpanVerdict::Drop
    } else {
        SpanVerdict::KeepMixed { residue }
    }
}

/// Whether `path` is a pre-pass output (`<audio>.t0.aec.wav`), i.e. whether
/// the pipeline decoded track 0 from the cleaned file.
pub fn is_aec_output(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with(".aec.wav"))
}

/// A stored segment's words spread evenly over its span. Segment rows carry
/// span times, not word times; a 30 s window's segments are a few seconds
/// long, so an even spread stays well inside [`RESIDUE_WINDOW_SECONDS`] of
/// where each word was said.
pub fn span_words(text: &str, start: f64, end: f64) -> Vec<TimedWord> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    let n = parts.len().max(1) as f64;
    let step = ((end - start).max(0.0)) / n;
    parts
        .iter()
        .enumerate()
        .map(|(i, w)| TimedWord {
            text: (*w).to_string(),
            start: start + i as f64 * step,
            end: start + (i as f64 + 1.0) * step,
        })
        .collect()
}

/// What one residue pass over a meeting did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResidueCounts {
    pub mic_spans: usize,
    pub dropped: usize,
    pub mixed: usize,
}

/// Step 4b, run by the pipeline AFTER both tracks are decoded and ONLY where
/// the AEC pre-pass produced track 0 (the one place text agreement may drop
/// words). Every mic span is judged by [`residue_verdict`] against the system
/// words around it: a Drop is persisted as the same row flagged
/// `suppressed_as_echo = 1`; a KeepMixed span keeps its residue and the
/// matched run becomes its own suppressed row. Idempotent: a second pass sees
/// only residue, which no longer matches.
pub fn apply_echo_residue(
    db: &crate::db::Database,
    meeting_id: &str,
) -> Result<ResidueCounts, String> {
    let started = Instant::now();
    let segments = db.list_meeting_segments(meeting_id)?;
    let mut sys: Vec<TimedWord> = segments
        .iter()
        .filter(|s| s.track == crate::meetings::SYSTEM_TRACK)
        .flat_map(|s| span_words(&s.text, s.start_seconds, s.end_seconds))
        .collect();
    sys.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut counts = ResidueCounts::default();
    for seg in segments
        .iter()
        .filter(|s| s.track == crate::meetings::MIC_TRACK)
    {
        counts.mic_spans += 1;
        let span = span_words(&seg.text, seg.start_seconds, seg.end_seconds);
        let lo = seg.start_seconds - RESIDUE_WINDOW_SECONDS;
        let hi = seg.end_seconds + RESIDUE_WINDOW_SECONDS;
        let near: Vec<TimedWord> = sys
            .iter()
            .filter(|w| w.start >= lo && w.start <= hi)
            .cloned()
            .collect();
        match residue_verdict(&span, &near, true) {
            SpanVerdict::Keep => {}
            SpanVerdict::Drop => {
                if db.mark_segment_suppressed_as_echo(meeting_id, &seg.id)? {
                    counts.dropped += 1;
                }
            }
            SpanVerdict::KeepMixed { residue } => {
                let runs = echo_residue_runs(&span, &near);
                let echo: Vec<&str> = span
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| runs.iter().any(|r| r.contains(i)))
                    .map(|(_, w)| w.text.as_str())
                    .collect();
                db.split_segment_echo(meeting_id, &seg.id, &residue.join(" "), &echo.join(" "))?;
                counts.mixed += 1;
            }
        }
    }
    log::info!(
        "meeting_aec residue meeting={meeting_id} mic_spans={} dropped={} mixed={} sys_words={} wall_ms={}",
        counts.mic_spans,
        counts.dropped,
        counts.mixed,
        sys.len(),
        started.elapsed().as_millis()
    );
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str, start: f64, step: f64) -> Vec<TimedWord> {
        text.split_whitespace()
            .enumerate()
            .map(|(i, w)| TimedWord {
                text: w.into(),
                start: start + i as f64 * step,
                end: start + i as f64 * step + step * 0.8,
            })
            .collect()
    }

    #[test]
    fn meeting_aec_paths() {
        let p = Path::new("/x/abc.t0.wav");
        assert_eq!(aec_path_for(p), PathBuf::from("/x/abc.t0.aec.wav"));
        let d = derived_paths_for_wav(p);
        assert!(d.contains(&PathBuf::from("/x/abc.t0.aec.wav")));
        assert!(d.contains(&PathBuf::from("/x/abc.t0.index.jsonl")));
        assert!(d.contains(&PathBuf::from("/x/abc.index.jsonl")));
        assert!(derived_paths_for_wav(Path::new("/x/abc.t0.aec.wav")).is_empty());
        // A legacy single-file mic WAV is track 0 under its old name: it takes
        // its AEC output and both anchor spellings with it.
        let legacy = derived_paths_for_wav(Path::new("/x/abc.wav"));
        assert_eq!(legacy.len(), 3);
        assert!(legacy.contains(&PathBuf::from("/x/abc.aec.wav")));
        assert!(legacy.contains(&PathBuf::from("/x/abc.index.jsonl")));
        assert!(legacy.contains(&PathBuf::from("/x/abc.t0.index.jsonl")));
        // The call track owns only its own anchor, never track 0's.
        let t1 = derived_paths_for_wav(Path::new("/x/abc.t1.wav"));
        assert!(t1.contains(&PathBuf::from("/x/abc.t1.index.jsonl")));
        assert!(!t1.contains(&PathBuf::from("/x/abc.index.jsonl")));
        assert!(!t1.contains(&PathBuf::from("/x/abc.t0.index.jsonl")));
    }

    #[test]
    fn meeting_aec_isolated_single_word_is_kept() {
        let mic = words("yeah", 10.0, 0.3);
        let sys = words("yeah totally agree with that", 10.2, 0.3);
        assert!(echo_residue_runs(&mic, &sys).is_empty());
        assert_eq!(residue_verdict(&mic, &sys, true), SpanVerdict::Keep);
    }

    #[test]
    fn meeting_aec_commitment_inside_overlapping_system_sentence_keeps_its_residue() {
        let far = "so the plan is that we ship the beta on friday and then we collect feedback from the first ten schools over the long weekend";
        assert_eq!(far.split_whitespace().count(), 25);
        let sys = words(far, 100.0, 0.3);
        // The mic hears the far side's middle eight words, then the user says seven of their own.
        let mut mic = words("we ship the beta on friday and then", 101.5, 0.3);
        mic.extend(words("I will send the contract by Monday", 104.0, 0.3));
        match residue_verdict(&mic, &sys, true) {
            SpanVerdict::KeepMixed { residue } => {
                assert_eq!(residue.join(" "), "I will send the contract by Monday")
            }
            other => panic!("expected the commitment kept as residue, got {other:?}"),
        }
    }

    #[test]
    fn meeting_aec_pure_echo_span_drops_and_words_only_on_mic_survive() {
        let sys = words("the community hall has the second weekend open", 5.0, 0.3);
        let echo = words("community hall has the second weekend", 5.3, 0.3);
        assert_eq!(residue_verdict(&echo, &sys, true), SpanVerdict::Drop);
        let own = words("budget venue projector stage morning sessions", 5.3, 0.3);
        assert_eq!(residue_verdict(&own, &sys, true), SpanVerdict::Keep);
        // No AEC and system audio present: text agreement never drops.
        assert_eq!(residue_verdict(&echo, &sys, false), SpanVerdict::Keep);
    }

    #[test]
    fn meeting_aec_match_outside_window_is_not_echo() {
        let sys = words("the community hall has the second weekend open", 5.0, 0.3);
        let late = words("community hall has the second weekend", 9.0, 0.3);
        assert_eq!(residue_verdict(&late, &sys, true), SpanVerdict::Keep);
    }

    #[test]
    fn meeting_aec_route_change_seam_drops_nothing_without_evidence() {
        // A mic reopen at 60 s: words either side of the seam, the system track
        // saying something else. Nothing within +-1 s of the seam may go.
        let mut mic = words("let me check that", 59.0, 0.25);
        mic.extend(words("okay I am back now", 60.2, 0.25));
        let sys = words("can you hear me now is that better", 59.5, 0.25);
        assert_eq!(residue_verdict(&mic, &sys, true), SpanVerdict::Keep);
    }
}
