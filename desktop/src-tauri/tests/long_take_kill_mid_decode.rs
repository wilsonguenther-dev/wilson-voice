//! DB-B — a long take that dies MID-DECODE, killed for real.
//!
//! `long_take_recovery.rs` proves the read half against a file planted in the
//! recovery dir. This file proves the write half produces that file and that it
//! survives a death that runs no destructor: the test re-executes itself as a
//! child process, drives the SAME clip writer a real take uses
//! (`ClipWav::spawn`) and the SAME Y3-B window loop with the SAME observer the
//! live dictation path hands it (`PersistTakeChunks`), and calls
//! `std::process::abort()` from inside the engine on window 9 of 12. Abort is
//! SIGABRT — no unwinding, no `Drop`, the same class as kill -9, a crash, or the
//! tray quit's `app.exit(0)`.
//!
//! The parent then replays startup in the app's order — `sweep_stale_wavs` on
//! `recordings/` FIRST, then `recover_dictation` on the recovery dir — and
//! checks the take is offered with its audio, its decoded prefix, and a resume
//! point that neither repeats nor skips a word.
//!
//! What this does NOT cover: the microphone. The clip's samples are a ramp
//! handed straight to `ClipWav::spawn`, which is what `stop_recording` calls.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use wilson_voice_lib::asr_engine::TimedTranscript;
use wilson_voice_lib::db::Database;
use wilson_voice_lib::meeting_asr::{ChunkConfig, MEETING_RATE};
use wilson_voice_lib::transcribe_progress::NoObserver;
use wilson_voice_lib::transcription::{
    DictationChunking, Transcriber, TranscriptionManager, DICTATION_CHUNK_SAMPLE_RATE,
};
use wilson_voice_lib::{
    read_wav_16k_mono, recover_dictation, sweep_stale_wavs, ClipWav, PersistTakeChunks,
};

#[path = "support/meeting.rs"]
mod support;
use support::{expected_words, window_bounds, RampDecoder};

const CHILD_ENV: &str = "YAP_DBB_KILL_CHILD_DIR";
const TAKE_ID: &str = "dbb-killed-take";
const TAKE_SECONDS: f64 = 60.0;
/// The engine call that aborts the process: the 9th window of 12.
const ABORT_AT: usize = 8;

struct RampEngine {
    decoder: RampDecoder,
    calls: Arc<std::sync::atomic::AtomicUsize>,
    abort_at: Option<usize>,
}

impl Transcriber for RampEngine {
    fn transcribe(
        &mut self,
        samples: &[f32],
        _language: Option<&str>,
        _bias: Option<&str>,
    ) -> Result<String, String> {
        let (a, b) = window_bounds(samples);
        Ok(self.decoder.transcript_for(a, b).text)
    }

    fn transcribe_timed(
        &mut self,
        samples: &[f32],
        _language: Option<&str>,
        _bias: Option<&str>,
    ) -> Result<TimedTranscript, String> {
        let n = self.calls.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        if Some(n) == self.abort_at {
            // THE KILL. No unwind, no destructor, no cleanup of any kind.
            std::process::abort();
        }
        let (a, b) = window_bounds(samples);
        Ok(self.decoder.transcript_for(a, b))
    }
}

fn manager(abort_at: Option<usize>, tag: &str) -> TranscriptionManager {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let m = TranscriptionManager::with_loader(
        Arc::new(move |_p: &Path| {
            Ok(Box::new(RampEngine {
                decoder: RampDecoder::new(true),
                calls: calls.clone(),
                abort_at,
            }) as Box<dyn Transcriber>)
        }),
        Duration::from_secs(600),
        Duration::from_secs(600),
        Duration::from_secs(30),
    );
    let stub = std::env::temp_dir().join(format!("yap-dbb-stub-{tag}-{}.gguf", std::process::id()));
    std::fs::write(&stub, b"stub").expect("stub model file");
    m.load("stub", &stub).expect("stub load");
    m
}

fn plan() -> DictationChunking {
    DictationChunking {
        threshold_seconds: 10.0,
        config: ChunkConfig {
            target_seconds: 5.0,
            min_seconds: 4.0,
            max_seconds: 6.0,
            overlap_seconds: 1.0,
            min_silence_seconds: 0.2,
        },
        sample_rate: DICTATION_CHUNK_SAMPLE_RATE,
    }
}

/// Samples whose value IS their time, from sample `from` to the end of the take.
fn ramp_from(from: usize) -> Vec<f32> {
    let n = (TAKE_SECONDS * MEETING_RATE as f64) as usize;
    (from..n).map(|i| i as f32 / MEETING_RATE as f32).collect()
}

fn tmpdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "yap-dbb-kill-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// The child: a live take, killed mid-decode. Never returns.
fn run_child(dir: &Path) -> ! {
    let db = Database::open(dir.join("wilson_voice.db")).expect("db opens");
    let recordings = dir.join("recordings");
    std::fs::create_dir_all(&recordings).expect("recordings dir");
    let samples = ramp_from(0);
    let mut clip = ClipWav::spawn(
        recordings.join(format!("{TAKE_ID}.wav")),
        samples.clone(),
        Some(&dir.join("recovery")),
    );
    // In the app the decode starts while this write runs; waiting here makes
    // the kill land at a deterministic point instead of racing the writer.
    clip.wait_written();
    let take_id = clip.take_id().expect("the clip names its take");
    let m = manager(Some(ABORT_AT), "child");
    let mut pill = NoObserver;
    let mut persist = PersistTakeChunks::new(&mut pill, &db, Some(&take_id));
    let _ = m.transcribe_take_with_observed(samples, None, None, &plan(), &mut persist);
    // Reaching here means the abort never fired — exit with a code the parent
    // reads as "the test rig is wrong", never as a pass.
    std::process::exit(97);
}

#[test]
fn a_take_killed_mid_decode_is_offered_back_with_its_audio_and_decoded_prefix() {
    if let Ok(dir) = std::env::var(CHILD_ENV) {
        run_child(Path::new(&dir));
    }
    let dir = tmpdir("abort");
    let status = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "a_take_killed_mid_decode_is_offered_back_with_its_audio_and_decoded_prefix",
            "--exact",
            "--test-threads=1",
            "--nocapture",
        ])
        .env(CHILD_ENV, &dir)
        .status()
        .expect("child runs");
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(
            status.signal(),
            Some(6),
            "the child must die by SIGABRT mid-decode, not exit ({status:?})"
        );
    }

    let recordings = dir.join("recordings");
    let recovery = dir.join("recovery");
    assert!(
        recovery.join(format!("{TAKE_ID}.decoding.wav")).exists(),
        "the in-flight link must survive a death that runs no destructor"
    );

    // STARTUP, in the app's order: recordings/ is emptied FIRST…
    assert!(
        sweep_stale_wavs(&recordings) >= 1,
        "the killed take's history wav is in recordings/ and the sweep removes it"
    );
    assert!(!recordings.join(format!("{TAKE_ID}.wav")).exists());
    // …and THEN the recovery sweep runs.
    let db = Database::open(dir.join("wilson_voice.db")).expect("db reopens");
    assert_eq!(
        recover_dictation(&db, &recovery),
        1,
        "the killed take is offered"
    );

    let rows = db.list_failed_dictations().expect("list");
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.take_id.as_deref(), Some(TAKE_ID));
    let audio = read_wav_16k_mono(Path::new(&row.wav_path)).expect("recovered wav reads");
    assert_eq!(
        audio.len(),
        ramp_from(0).len(),
        "the whole take's audio came back, not a prefix of it"
    );
    assert!(
        !recovery.join(format!("{TAKE_ID}.decoding.wav")).exists(),
        "the orphan is consumed once its wav exists"
    );

    // The decoded prefix came back, and the resume point is where it ends.
    let partial = row
        .partial_text
        .clone()
        .expect("the decoded prefix survives the kill");
    let resume = db.take_resume_sample(TAKE_ID).expect("resume reads") as usize;
    assert!(
        resume > 0 && resume < audio.len(),
        "resume point {resume} is mid-take"
    );
    // Decoding ONLY the remainder and folding the prefix in front of it gives
    // the exact take: no seam word doubled, none skipped at the resume point.
    let rest = manager(None, "parent")
        .transcribe_take_with(ramp_from(resume), None, None, &plan())
        .expect("remainder decodes");
    assert_eq!(
        format!("{} {}", partial.trim(), rest.text.trim()),
        expected_words(TAKE_SECONDS as usize).join(" "),
        "prefix + remainder must be the whole take, word for word"
    );

    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_orderly_end_of_a_take_leaves_no_in_flight_link() {
    // Success, failure and cancel all end in `Drop` or `keep_for_recovery`;
    // either way the next launch must find nothing, or a finished take is
    // offered twice.
    let dir = tmpdir("orderly");
    let recordings = dir.join("recordings");
    let recovery = dir.join("recovery");
    std::fs::create_dir_all(&recordings).expect("recordings dir");

    let mut done = ClipWav::spawn(recordings.join("done.wav"), ramp_from(0), Some(&recovery));
    done.wait_written();
    assert!(
        recovery.join("done.decoding.wav").exists(),
        "linked while in flight"
    );
    drop(done);
    assert!(!recovery.join("done.decoding.wav").exists());
    assert!(!recordings.join("done.wav").exists());

    let mut kept = ClipWav::spawn(recordings.join("kept.wav"), ramp_from(0), Some(&recovery));
    let parked = kept
        .keep_for_recovery(&recovery)
        .expect("kept for a retry row");
    assert!(parked.exists());
    assert!(
        !recovery.join("kept.decoding.wav").exists(),
        "the row owns it now"
    );
    drop(kept);
    assert!(parked.exists(), "keeping survives the guard");

    let db = Database::open(dir.join("wilson_voice.db")).expect("db opens");
    std::fs::remove_file(&parked).expect("clear the kept clip");
    assert_eq!(recover_dictation(&db, &recovery), 0, "no phantom offer");
    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_leftover_partial_wav_never_costs_the_spill() {
    // A full disk at a previous launch left a truncated `<id>.wav` beside the
    // spill. That must never be read as "already rebuilt": the spill is the
    // only good copy.
    let dir = tmpdir("partial");
    let recovery = dir.join("recovery");
    std::fs::create_dir_all(&recovery).expect("recovery dir");
    let id = "dbb-partial";
    let samples: Vec<f32> = (0..(MEETING_RATE as usize * 3))
        .map(|i| ((i % 64) as f32 - 32.0) / 64.0)
        .collect();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: MEETING_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(recovery.join(format!("{id}.capture.wav")), spec)
        .expect("spill opens");
    for s in &samples {
        w.write_sample((s * 32767.0) as i16).expect("sample");
    }
    w.finalize().expect("spill finalizes");
    std::fs::write(recovery.join(format!("{id}.wav")), b"RIFF\0\0").expect("truncated wav");

    let db = Database::open(dir.join("wilson_voice.db")).expect("db opens");
    assert_eq!(
        recover_dictation(&db, &recovery),
        1,
        "the take is still offered"
    );
    let rows = db.list_failed_dictations().expect("list");
    let audio = read_wav_16k_mono(Path::new(&rows[0].wav_path)).expect("a complete wav");
    assert_eq!(
        audio.len(),
        samples.len(),
        "the full take, not the truncated stub"
    );
    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_orderly_end_of_the_decode_clears_its_chunk_text() {
    // The live path drops `PersistTakeChunks` when the decode returns by any
    // route; only a death skips that. A prefix outliving a finished decode
    // would be offered back as "unfinished" for a take the user already has.
    use wilson_voice_lib::transcribe_progress::TakeObserver;
    let dir = tmpdir("drop");
    let db = Database::open(dir.join("wilson_voice.db")).expect("db opens");
    let mut pill = NoObserver;
    {
        let mut persist = PersistTakeChunks::new(&mut pill, &db, Some("dbb-orderly"));
        assert!(persist.wants_prefix());
        persist.prefix_done(0, 16_000, "the first window");
        assert_eq!(
            db.take_partial_text("dbb-orderly")
                .expect("reads")
                .as_deref(),
            Some("the first window")
        );
    }
    assert_eq!(db.take_partial_text("dbb-orderly").expect("reads"), None);
    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}
