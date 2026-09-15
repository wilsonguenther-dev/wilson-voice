//! Y3-D — cancel works mid-DECODE, not just mid-recording, and a cancel never
//! loses the audio.
//!
//! What was broken. `cancel_recording` stops a RECORDING. The moment the hold
//! ended and decode started there was nothing left to press — the main window's
//! record button is `disabled={status.busy}` and reads "Transcribing…" — so a
//! fifteen-minute take the user regretted the instant they let go occupied the
//! one shared engine to completion. And there was no cancel HOTKEY at all:
//! `shortcuts::ALL` was PASTE_LAST, UNDO_AI_EDIT, DICTATION_TOGGLE_LEGACY,
//! MEETING_TOGGLE.
//!
//! What must be true now, one test each:
//!
//! * a cancel writes NO transcript row and pastes NOTHING;
//! * a cancel mid-decode stops the decode at the next chunk boundary, without
//!   killing the engine (it is in-process and shared with meetings — killing it
//!   takes a meeting down);
//! * the clip SURVIVES, in the recovery dir, under the same 7-day lifecycle a
//!   failed take gets. A cancel that destroys fifteen minutes of audio is a
//!   worse bug than the one being fixed;
//! * the cancel reason is its own constant, distinguishable from the two that
//!   already existed;
//! * Escape is NOT a global shortcut while idle. A bare Escape registered for
//!   the life of the process would swallow Escape in every other app on the Mac.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use wilson_voice_lib::db::Database;
use wilson_voice_lib::meeting_asr::ChunkConfig;
use wilson_voice_lib::shortcuts::{self, Scope};
use wilson_voice_lib::transcription::{
    CancelHandle, DictationChunking, Transcriber, TranscriptionManager, ABANDONED_FOR_EXIT,
    CANCELLED_BY_USER, DICTATION_CHUNK_SAMPLE_RATE, PREEMPTED_FOR_DICTATION,
};
use wilson_voice_lib::ClipWav;
use wilson_voice_lib::{keep_cancelled_take, CANCELLED_TAKE_REASON};

/// How many chunk boundaries the stub decode crosses if nobody stops it.
const CHUNKS: usize = 20;
/// Wall time per chunk. Small enough that the whole file is fast, large enough
/// that a cancel fired from another thread lands mid-decode rather than racing
/// the decode's own start.
const CHUNK_DECODE: Duration = Duration::from_millis(40);

/// A chunked decode: it crosses [`CHUNKS`] boundaries and CHECKS FOR
/// CANCELLATION AT EACH ONE, which is the cooperative half of this item.
///
/// The flag it polls is the same flag `cancel_handle` hands the manager, so the
/// test drives the real wire: `TranscriptionManager::cancel_in_flight_for_user`
/// → the published `CancelHandle` → this engine's own boundary check. Nothing
/// is killed and nothing is unloaded; the decode chooses to return.
struct ChunkedEngine {
    cancelled: Arc<AtomicBool>,
    /// Chunk boundaries actually crossed, so "it stopped early" is a number
    /// rather than a timing claim.
    chunks_done: Arc<AtomicUsize>,
}

impl Transcriber for ChunkedEngine {
    fn transcribe(
        &mut self,
        _samples: &[f32],
        _language: Option<&str>,
        _bias: Option<&str>,
    ) -> Result<String, String> {
        for _ in 0..CHUNKS {
            if self.cancelled.load(Ordering::SeqCst) {
                // What a real cancelled native decode does: return whatever it
                // has. The manager throws it away and reports the reason.
                return Ok("partial".to_string());
            }
            std::thread::sleep(CHUNK_DECODE);
            self.chunks_done.fetch_add(1, Ordering::SeqCst);
        }
        Ok("the whole fifteen minutes".to_string())
    }

    fn cancel_handle(&self) -> Option<CancelHandle> {
        let flag = self.cancelled.clone();
        Some(Arc::new(move || flag.store(true, Ordering::SeqCst)))
    }

    fn reset_cancel(&mut self) {
        // The real engine's flag is sticky, so the manager clears it between
        // decodes. Modelled here for the same reason it exists there.
        self.cancelled.store(false, Ordering::SeqCst);
    }
}

fn manager(cancelled: Arc<AtomicBool>, chunks_done: Arc<AtomicUsize>) -> TranscriptionManager {
    TranscriptionManager::with_loader(
        Arc::new(move |_p: &Path| {
            Ok(Box::new(ChunkedEngine {
                cancelled: cancelled.clone(),
                chunks_done: chunks_done.clone(),
            }) as Box<dyn Transcriber>)
        }),
        Duration::from_secs(3600),
        Duration::from_secs(3600),
        Duration::from_secs(30),
    )
}

/// A real 16 kHz WAV on disk plus the guard that owns it — the same shape the
/// dictation pipeline hands to the keep-the-audio path.
fn take_wav(dir: &Path, name: &str) -> (std::path::PathBuf, ClipWav) {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(format!("{name}.wav"));
    // 1 s of 220 Hz — a real file, not an empty one, so a "kept" clip that was
    // silently truncated would show up as a size mismatch below.
    let samples: Vec<f32> = (0..16_000)
        .map(|i| 0.4 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / 16_000.0).sin())
        .collect();
    let mut bytes: Vec<u8> = Vec::new();
    let data_len = (samples.len() * 2) as u32;
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&16_000u32.to_le_bytes());
    bytes.extend_from_slice(&32_000u32.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for s in &samples {
        bytes.extend_from_slice(&((s * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(&path, &bytes).unwrap();
    (path.clone(), ClipWav::adopt_existing(path))
}

/// A cancel writes no transcript row and puts nothing on the clipboard.
///
/// Asserted at the level the guarantee actually lives: the cancel path writes a
/// FAILED-dictation (recovery) row and NEVER an entry in `transcripts`. A
/// transcript row is also the only thing the paste path is ever handed — the
/// worker's `Cancelled` arm returns before both `copy_and_maybe_paste` and
/// `insert_transcript_at` — so "no row" and "nothing pasted" are the same
/// assertion made at the one place a test can hold still.
#[test]
fn cancel_mid_recording_writes_no_row_and_pastes_nothing() {
    let tmp = std::env::temp_dir().join(format!("y3d-norow-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let db = Database::open(tmp.join("history.db")).unwrap();

    let before = db.list_transcripts(1000, None).unwrap().len();

    let (_p, mut clip) = take_wav(&tmp.join("recordings"), "cancelled-take");
    let row = keep_cancelled_take(&db, &tmp.join("recovery"), &mut clip, 900.0, None)
        .expect("a cancelled take is preserved");

    let after = db.list_transcripts(1000, None).unwrap().len();
    assert_eq!(
        before, after,
        "a cancelled take wrote a TRANSCRIPT row — it must write only a recovery row"
    );
    // The row it DOES write is the recovery one, and it says "cancelled"
    // rather than borrowing a failure's wording.
    assert_eq!(row.error, CANCELLED_TAKE_REASON);
    assert!(
        db.list_failed_dictations()
            .unwrap()
            .iter()
            .any(|f| f.id == row.id),
        "the cancelled take is not retrievable from History"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// A cancel during a decode stops it at the NEXT CHUNK BOUNDARY — not at the
/// end of the take, and not by killing the engine.
///
/// The two halves that matter:
///   * it stops EARLY (fewer than `CHUNKS` boundaries crossed), which is the
///     fifteen-minute wait being deleted;
///   * the manager reports [`CANCELLED_BY_USER`], so the caller can tell this
///     from a decode failure and keep the audio instead of writing a hole.
///
/// It also proves the engine SURVIVES: a second decode on the same manager
/// succeeds afterwards. Killing the engine process would have been the easy
/// implementation, and it would take a running meeting down with it.
#[test]
fn cancel_mid_decode_stops_at_the_next_chunk_boundary() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let chunks_done = Arc::new(AtomicUsize::new(0));
    let m = Arc::new(manager(cancelled.clone(), chunks_done.clone()));
    // The manager refuses to load a model file that is not there, so the stub
    // gets a real (empty) one — the loader above never reads it.
    let model_dir = std::env::temp_dir().join(format!("y3d-model-{}", std::process::id()));
    std::fs::create_dir_all(&model_dir).unwrap();
    let model_path = model_dir.join("stub.bin");
    std::fs::write(&model_path, b"stub").unwrap();
    m.load("stub", &model_path).expect("stub loads");

    let decoder = {
        let m = m.clone();
        std::thread::spawn(move || m.transcribe(vec![0.1; 16_000], None, None))
    };

    // Let the decode get properly under way — several boundaries in, so the
    // cancel lands MID-decode rather than before it starts.
    std::thread::sleep(CHUNK_DECODE * 4);
    assert!(
        m.cancel_in_flight_for_user(),
        "there was an in-flight decode and the cancel did not reach it"
    );

    let out = decoder.join().unwrap();
    assert_eq!(
        out.as_deref().map_err(|e| e.as_str()),
        Err(CANCELLED_BY_USER),
        "a user-cancelled decode must report its own reason, not a decode failure"
    );
    let crossed = chunks_done.load(Ordering::SeqCst);
    assert!(
        crossed < CHUNKS,
        "the decode ran to completion anyway: {crossed}/{CHUNKS} chunk boundaries crossed"
    );

    // The engine is still usable — nothing was killed or unloaded.
    let again = m.transcribe(vec![0.1; 16_000], None, None);
    assert!(
        again.is_ok(),
        "the engine did not survive the cancel: {again:?} — a meeting sharing it would have died"
    );
}

/// The cancelled clip lands in the RECOVERY dir, not the bin.
#[test]
fn cancelled_clip_lands_in_the_recovery_dir() {
    let tmp = std::env::temp_dir().join(format!("y3d-recov-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let db = Database::open(tmp.join("history.db")).unwrap();

    let recordings = tmp.join("recordings");
    let recovery = tmp.join("recovery");
    let (original, mut clip) = take_wav(&recordings, "fifteen-minute-take");
    let original_len = std::fs::metadata(&original).unwrap().len();

    let row = keep_cancelled_take(&db, &recovery, &mut clip, 900.0, None)
        .expect("a cancelled take must be preserved");
    // Dropping the guard is what used to unlink the only copy. It must not now.
    drop(clip);

    let kept = Path::new(&row.wav_path);
    assert!(
        kept.starts_with(&recovery),
        "cancelled clip is not under the recovery dir: {}",
        kept.display()
    );
    assert!(
        kept.exists(),
        "the cancelled clip was deleted: {}",
        kept.display()
    );
    assert_eq!(
        std::fs::metadata(kept).unwrap().len(),
        original_len,
        "the cancelled clip was truncated on its way to recovery"
    );
    assert!(
        !original.exists(),
        "the clip was COPIED rather than moved — two copies is a leak, not a save"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// Three distinct constants, compared as constants.
///
/// `transcription.rs:74-77` already warns that "a string compared against a
/// literal in another module is a bug waiting for someone to reword the
/// message". This is that warning made executable for the third member of the
/// family: reusing `PREEMPTED_FOR_DICTATION` for a user cancel would make a
/// cancelled dictation look to a meeting driver like work worth re-decoding.
#[test]
fn cancel_reason_is_distinguishable_from_preempted_and_abandoned() {
    let all = [
        PREEMPTED_FOR_DICTATION,
        ABANDONED_FOR_EXIT,
        CANCELLED_BY_USER,
    ];
    for (i, a) in all.iter().enumerate() {
        for b in all.iter().skip(i + 1) {
            assert_ne!(a, b, "two cancel reasons are the same string: {a:?}");
        }
        assert!(!a.is_empty());
    }
    // And not merely different strings — different MEANINGS, which is what the
    // callers switch on. A substring relationship would let a naive
    // `contains()` check in a future caller conflate two of them.
    assert!(!CANCELLED_BY_USER.contains(PREEMPTED_FOR_DICTATION));
    assert!(!PREEMPTED_FOR_DICTATION.contains(CANCELLED_BY_USER));
    assert!(!CANCELLED_BY_USER.contains(ABANDONED_FOR_EXIT));
    assert!(!ABANDONED_FOR_EXIT.contains(CANCELLED_BY_USER));
}

/// Yap does not own Escape while it is idle.
///
/// `RegisterEventHotKey` is system-wide and exclusive: a bare Escape registered
/// at launch would break Escape in every other app on the Mac for as long as
/// Yap runs. `registered_while_idle()` is the list `lib.rs` registers from at
/// startup, so this reads the same table the app does rather than eyeballing
/// the register call sites.
#[test]
fn escape_is_not_a_global_shortcut_while_idle() {
    assert!(
        !shortcuts::registered_while_idle()
            .iter()
            .any(|b| b.id == shortcuts::CANCEL.id),
        "the cancel key is registered while idle — Escape would be eaten system-wide"
    );
    assert!(
        matches!(shortcuts::CANCEL.scope, Scope::WhileTakeActive),
        "the cancel key must be scoped to a take"
    );
    assert!(
        shortcuts::registered_while_take_active()
            .iter()
            .any(|b| b.id == shortcuts::CANCEL.id),
        "the cancel key is scoped to a take but never appears in that list"
    );
    // Nothing else may be take-scoped by accident: every OTHER binding is a
    // ⌃⌘/⌘⇧ chord that is safe to hold for the life of the process, and
    // demoting one to take-scope would silently stop it working.
    for b in shortcuts::registered_while_take_active() {
        assert_eq!(
            b.id,
            shortcuts::CANCEL.id,
            "{} became take-scoped — it will stop firing outside a take",
            b.id
        );
    }
    // The table is still collision-free with the new chord in it.
    assert_eq!(shortcuts::first_collision(), None);
}

/// How many windows the plan below splits a 60 s take into.
const PLANNED_WINDOWS: usize = 12;

/// A stub that counts WINDOWS, not inner boundaries, and has no cancel hook at
/// all.
///
/// The missing hook is the point. `Transcriber::cancel_handle` returning `None`
/// is the honest shape of a native decode that cannot be interrupted: the only
/// place such a take can be stopped is BETWEEN windows. If the loop's boundary
/// check is the thing doing the work, this engine proves it; if the test leaned
/// on a cancel hook it would be re-proving the engine path that
/// `cancel_mid_decode_stops_at_the_next_chunk_boundary` already covers.
struct WindowCountingEngine {
    windows_decoded: Arc<AtomicUsize>,
}

impl Transcriber for WindowCountingEngine {
    fn transcribe(
        &mut self,
        _samples: &[f32],
        _language: Option<&str>,
        _bias: Option<&str>,
    ) -> Result<String, String> {
        std::thread::sleep(Duration::from_millis(30));
        let n = self.windows_decoded.fetch_add(1, Ordering::SeqCst);
        Ok(format!("window {n}"))
    }
}

/// Twelve five-second windows — the same geometry the Y3-B tests use, small
/// enough that this is milliseconds rather than minutes.
fn twelve_window_plan() -> DictationChunking {
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

/// THE REGRESSION THIS EXISTS FOR. A cancel must stop Y3-B's REAL windowed
/// dictation loop, not just the single decode inside one window.
///
/// `InFlight::discard` is consumed per decode — `*slot = None` on the way out —
/// so without the sticky latch a cancel can only ever spend itself on ONE
/// window. The loop catches that window's `Err`, logs "dictation chunk N
/// failed", and decodes every remaining window anyway: the user cancels at
/// minute four of fifteen and the engine keeps going to minute fifteen, which
/// is precisely the bug this item was opened for, merely moved one layer down.
#[test]
fn cancel_stops_the_real_windowed_loop_not_just_one_window() {
    let windows_decoded = Arc::new(AtomicUsize::new(0));
    let m = Arc::new({
        let windows_decoded = windows_decoded.clone();
        TranscriptionManager::with_loader(
            Arc::new(move |_p: &Path| {
                Ok(Box::new(WindowCountingEngine {
                    windows_decoded: windows_decoded.clone(),
                }) as Box<dyn Transcriber>)
            }),
            Duration::from_secs(3600),
            Duration::from_secs(3600),
            Duration::from_secs(30),
        )
    });
    let model_dir = std::env::temp_dir().join(format!("y3d-win-{}", std::process::id()));
    std::fs::create_dir_all(&model_dir).unwrap();
    let model_path = model_dir.join("stub.bin");
    std::fs::write(&model_path, b"stub").unwrap();
    m.load("stub", &model_path).expect("stub loads");
    m.begin_user_take();

    // 60 s at 16 kHz -> twelve windows under the plan above.
    let samples = vec![0.1f32; DICTATION_CHUNK_SAMPLE_RATE as usize * 60];
    let decoder = {
        let m = m.clone();
        std::thread::spawn(move || {
            m.transcribe_take_with(samples, None, None, &twelve_window_plan())
        })
    };

    // Let a couple of windows go by, so this is a cancel MID-take.
    std::thread::sleep(Duration::from_millis(90));
    m.cancel_in_flight_for_user();
    // The latch is raised whether or not there was a hook to fire — this engine
    // has none, and the boundary check is the only thing that can stop it.
    assert!(
        m.user_cancel_requested(),
        "the cancel did not raise the latch the chunk boundary reads"
    );

    let out = decoder.join().unwrap();
    assert_eq!(
        out.as_ref()
            .map(|t| t.text.as_str())
            .map_err(|e| e.as_str()),
        Err(CANCELLED_BY_USER),
        "a cancelled windowed take must report its own reason — and must NOT come \
         back as an Ok(degraded) take carrying the text the user just rejected"
    );
    let decoded = windows_decoded.load(Ordering::SeqCst);
    assert!(
        decoded < PLANNED_WINDOWS,
        "the windowed loop ran to completion anyway: {decoded}/{PLANNED_WINDOWS} windows decoded"
    );

    // The latch is per-take, not per-process: the NEXT take still works. A
    // sticky flag nobody clears is a cancel button that disables dictation.
    m.begin_user_take();
    assert!(
        !m.user_cancel_requested(),
        "arming a new take did not clear the previous take's latch"
    );
    windows_decoded.store(0, Ordering::SeqCst);
    let again = m.transcribe_take_with(
        vec![0.1f32; DICTATION_CHUNK_SAMPLE_RATE as usize * 60],
        None,
        None,
        &twelve_window_plan(),
    );
    assert!(
        again.is_ok(),
        "the take after a cancelled one failed: {again:?} — the latch was never cleared"
    );
    assert_eq!(
        windows_decoded.load(Ordering::SeqCst),
        PLANNED_WINDOWS,
        "the take after a cancelled one was truncated by the previous take's latch"
    );
}
