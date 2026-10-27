//! yap24-NT2 needs-human sweep — both review seats BLOCKED 24ba9f9 because the
//! pinned meeting model was recorded as `asr_model` while
//! `WarmEngineChunkAsr::transcribe_timed` decoded on WHATEVER model sat in the
//! warm slot (it loaded only when nothing was resident). A user whose dictation
//! model X is warm got a meeting decoded by X and labelled as the pinned model,
//! and a dictation mid-meeting spliced two models into one transcript.
//!
//! These run against the real `TranscriptionManager` with a stub loader whose
//! engine answers with the file it was loaded from, so the transcript itself
//! says which model decoded it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use wilson_voice_lib::meeting_asr::{ChunkAsr, WarmEngineChunkAsr};
use wilson_voice_lib::transcription::{Transcriber, TranscriptionManager, NO_ENGINE_LOADED};

struct NamedEngine(String);

impl Transcriber for NamedEngine {
    fn transcribe(
        &mut self,
        _samples: &[f32],
        _language: Option<&str>,
        _bias: Option<&str>,
    ) -> Result<String, String> {
        Ok(self.0.clone())
    }
}

fn manager() -> TranscriptionManager {
    TranscriptionManager::with_loader(
        Arc::new(|p: &Path| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            Ok(Box::new(NamedEngine(name)) as Box<dyn Transcriber>)
        }),
        Duration::from_secs(3600),
        Duration::from_secs(3600),
        Duration::from_secs(10),
    )
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn a_meeting_chunk_decodes_on_the_pinned_model_not_the_warm_dictation_model() {
    let m = manager();
    let dictation = root().join("Cargo.toml");
    let pinned = root().join("build.rs");
    // The user's dictation model is warm, as arm_asr_engine leaves it.
    m.load("dictation-x", &dictation)
        .expect("dictation model loads");

    let asr = WarmEngineChunkAsr::new(m.clone(), "meeting-pinned", &pinned, None, None);
    let out = asr.transcribe_timed(&[0.1; 1600]).expect("chunk decodes");
    assert_eq!(
        out.text, "build.rs",
        "the chunk must be decoded by the pinned model"
    );
    assert_eq!(m.loaded_model().as_deref(), Some("meeting-pinned"));

    // A dictation mid-meeting swaps the slot back to its model; the next chunk
    // reloads the pinned one instead of decoding on the dictation model.
    m.load("dictation-x", &dictation)
        .expect("dictation swaps the slot");
    let out = asr
        .transcribe_timed(&[0.1; 1600])
        .expect("next chunk decodes");
    assert_eq!(
        out.text, "build.rs",
        "a later chunk must not splice in the dictation model"
    );
}

#[test]
fn a_pinned_decode_refuses_another_resident_model_and_leaves_it_in_place() {
    let m = manager();
    m.load("dictation-x", &root().join("Cargo.toml"))
        .expect("loads");
    let err = m
        .transcribe_timed_on("meeting-pinned", vec![0.1; 1600], None, None)
        .unwrap_err();
    assert_eq!(err, NO_ENGINE_LOADED);
    // The dictation's engine is handed back untouched, not dropped.
    assert_eq!(m.loaded_model().as_deref(), Some("dictation-x"));
    assert_eq!(
        m.transcribe(vec![0.1; 1600], None, None).unwrap(),
        "Cargo.toml"
    );
}
