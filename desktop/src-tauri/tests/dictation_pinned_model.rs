//! yap24-NT2 fix round 2 — a long dictation is never decoded on the meeting's
//! pinned model.
//!
//! The reviewer's probe, kept as a test: the SHIPPED meeting decoder
//! (`WarmEngineChunkAsr` on the pinned model, standing down while
//! `EngineDemand::dictation_pending`) polls every 2 ms beside the SHIPPED
//! dictation entry point lib.rs calls (`transcribe_take_observed_on`, default
//! chunking, a 660 s take — 22 windows). Before the fix 20 of 22 windows ran
//! on the pinned model in every trial; now none may, and the meeting's own
//! chunks stay on its pinned model.
//!
//! Lives in `tests/` rather than beside the manager because its 2 ms poll is a
//! test-only timer, and the YV81 census in `long_take_budget.rs` counts every
//! sub-second `Duration` in the take-path modules.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use wilson_voice_lib::meeting_asr::{ChunkAsr, EngineDemand, WarmEngineChunkAsr};
use wilson_voice_lib::transcribe_progress::NoObserver;
use wilson_voice_lib::transcription::{
    Transcriber, TranscriptionManager, DICTATION_CHUNK_SAMPLE_RATE,
};

/// A meeting chunk is this many samples; no dictation window is.
const MEETING_CHUNK: usize = 777;

type Log = Arc<Mutex<Vec<(String, usize)>>>;

/// Logs which MODEL (its file name) decoded each call and how many samples it
/// was handed.
struct NamedEngine {
    name: String,
    log: Log,
}

impl Transcriber for NamedEngine {
    fn transcribe(
        &mut self,
        samples: &[f32],
        _language: Option<&str>,
        _bias_prompt: Option<&str>,
    ) -> Result<String, String> {
        self.log.lock().push((self.name.clone(), samples.len()));
        Ok(format!("{} said these words", self.name))
    }
}

fn model_file(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, b"stub gguf").expect("write stub model");
    path
}

fn manager(log: Log) -> TranscriptionManager {
    type Loader = Arc<dyn Fn(&Path) -> Result<Box<dyn Transcriber>, String> + Send + Sync>;
    let loader: Loader = Arc::new(move |path: &Path| {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(Box::new(NamedEngine {
            name,
            log: log.clone(),
        }) as Box<dyn Transcriber>)
    });
    TranscriptionManager::with_loader(
        loader,
        Duration::from_secs(600),
        Duration::from_secs(600),
        Duration::from_secs(10),
    )
}

#[test]
fn a_meeting_job_never_gets_a_long_dictation_decoded_on_its_pinned_model() {
    let dir = std::env::temp_dir().join(format!("yap-nt2-pin-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let dict = model_file(&dir, "dict");
    let pinned = model_file(&dir, "pinned");
    let take_seconds = 660usize;

    for (trial, gap_ms) in [0u64, 0, 2, 2, 2].into_iter().enumerate() {
        let log: Log = Arc::new(Mutex::new(Vec::new()));
        let m = manager(log.clone());
        let stop = Arc::new(AtomicBool::new(false));
        let meeting = {
            let m = m.clone();
            let stop = stop.clone();
            let asr = WarmEngineChunkAsr::new(m.clone(), "pinned", &pinned, None, None);
            std::thread::spawn(move || {
                while !stop.load(Ordering::Acquire) {
                    if !m.dictation_pending() {
                        let _ = asr.transcribe_timed(&vec![0.2f32; MEETING_CHUNK]);
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
            })
        };
        // lib.rs's order: recording on, recording off at stop_and_transcribe,
        // then transcribe_native loads the user's model and decodes.
        m.set_live_recording(true);
        std::thread::sleep(Duration::from_millis(20));
        m.set_live_recording(false);
        std::thread::sleep(Duration::from_millis(gap_ms));
        m.load("dict", &dict).expect("dict loads");
        let take = m
            .transcribe_take_observed_on(
                "dict",
                &dict,
                vec![0.1f32; DICTATION_CHUNK_SAMPLE_RATE as usize * take_seconds],
                None,
                None,
                &mut NoObserver,
            )
            .expect("take decodes");
        stop.store(true, Ordering::Release);
        meeting.join().expect("meeting thread");

        let log = log.lock().clone();
        let dictation: Vec<&(String, usize)> = log
            .iter()
            .filter(|(_, len)| *len != MEETING_CHUNK)
            .collect();
        let dictation_on_pinned = dictation.iter().filter(|(n, _)| n == "pinned").count();
        let meeting_on_dict = log
            .iter()
            .filter(|(n, len)| *len == MEETING_CHUNK && n == "dict")
            .count();
        println!(
            "trial {trial} gap_ms={gap_ms}: windows={} dictation_decodes={} dictation_on_PINNED={dictation_on_pinned} meeting_on_DICT={meeting_on_dict}",
            take.chunks_total,
            dictation.len()
        );
        assert!(take.windowed, "a {take_seconds} s take must window");
        assert_eq!(
            take.chunks_failed, 0,
            "trial {trial}: a reload cost a window"
        );
        assert_eq!(dictation.len(), take.chunks_total, "trial {trial}: {log:?}");
        assert_eq!(dictation_on_pinned, 0, "trial {trial}: {log:?}");
        assert_eq!(meeting_on_dict, 0, "trial {trial}: {log:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
