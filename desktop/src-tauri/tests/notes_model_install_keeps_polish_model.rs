//! yap24-NT4 R3 — installing (or resolving) the notes model leaves the
//! dictation `polish_model` setting byte-unchanged.
//!
//! The panel's fear (2026-09-26): summaries and dictation polish both read
//! `polish_model`, and a one-click install that WROTE it would silently turn
//! on LLM polish for every dictation. The notes model is fetched through
//! `summary_jobs::ensure_notes_model`, the same function the app's notes
//! worker (`AppNotes::fetch_model` in lib.rs) calls; this test drives it both
//! directly and through a real `SummaryJobs` worker, reading the settings —
//! in memory and on disk — before and after.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use wilson_voice_lib::summarize::DEFAULT_SUMMARY_MODEL;
use wilson_voice_lib::summary_jobs::{
    ensure_notes_model, CancelToken, NotesBackend, NotesError, NotesPhase, NotesStatus, SummaryJobs,
};
use wilson_voice_lib::{load_settings, AppSettings};

const OVERRIDES: &[&str] = &["", "qwen2.5-0.5b-instruct-q4_k_m", "not-a-model"];

/// A scratch dir under the system temp dir, removed when dropped.
struct Scratch(std::path::PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn settings_file(polish_model: &str) -> (Scratch, std::path::PathBuf, Vec<u8>) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "yap-nt4-r3-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let dir = Scratch(dir);
    let path = dir.0.join("settings.json");
    let s = AppSettings { polish_model: polish_model.to_string(), ..AppSettings::default() };
    std::fs::write(&path, serde_json::to_vec_pretty(&s).unwrap()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    (dir, path, bytes)
}

fn polish_bytes(s: &Mutex<AppSettings>) -> Vec<u8> {
    s.lock().unwrap().polish_model.as_bytes().to_vec()
}

fn whole_bytes(s: &Mutex<AppSettings>) -> Vec<u8> {
    serde_json::to_vec(&*s.lock().unwrap()).unwrap()
}

#[test]
fn installing_the_notes_model_leaves_polish_model_byte_unchanged() {
    for &chosen in OVERRIDES {
        let (_dir, path, file_before) = settings_file(chosen);
        let settings = Mutex::new(load_settings(&path));
        let polish_before = polish_bytes(&settings);
        let whole_before = whole_bytes(&settings);
        assert_eq!(polish_before, chosen.as_bytes(), "fixture loaded as written");

        let on_disk: Mutex<HashSet<String>> = Mutex::new(HashSet::new());
        let mut downloads = Vec::new();
        let used = ensure_notes_model(
            &|| settings.lock().unwrap().polish_model.clone(),
            &|id| on_disk.lock().unwrap().contains(id),
            &mut |id| {
                downloads.push(id.to_string());
                on_disk.lock().unwrap().insert(id.to_string());
                Ok(())
            },
        )
        .expect("the notes model installs");
        assert_eq!(used, DEFAULT_SUMMARY_MODEL, "override {chosen:?}");
        assert_eq!(downloads, vec![DEFAULT_SUMMARY_MODEL.to_string()]);

        // Selecting it again (now on disk) neither downloads nor writes.
        let again = ensure_notes_model(
            &|| settings.lock().unwrap().polish_model.clone(),
            &|id| on_disk.lock().unwrap().contains(id),
            &mut |_| panic!("an installed notes model is never fetched again"),
        )
        .unwrap();
        assert_eq!(again, DEFAULT_SUMMARY_MODEL);

        assert_eq!(polish_bytes(&settings), polish_before, "polish_model changed for {chosen:?}");
        assert_eq!(whole_bytes(&settings), whole_before, "settings changed for {chosen:?}");
        assert_eq!(std::fs::read(&path).unwrap(), file_before, "settings.json changed for {chosen:?}");
    }
}

#[test]
fn an_installed_polish_override_is_used_and_still_not_written() {
    let (_dir, path, file_before) = settings_file("qwen2.5-0.5b-instruct-q4_k_m");
    let settings = Mutex::new(load_settings(&path));
    let before = polish_bytes(&settings);
    let used = ensure_notes_model(
        &|| settings.lock().unwrap().polish_model.clone(),
        &|id| id == "qwen2.5-0.5b-instruct-q4_k_m",
        &mut |_| panic!("an override on disk needs no download"),
    )
    .unwrap();
    assert_eq!(used, "qwen2.5-0.5b-instruct-q4_k_m");
    assert_eq!(polish_bytes(&settings), before);
    assert_eq!(std::fs::read(&path).unwrap(), file_before);
}

#[test]
fn a_failed_install_also_leaves_polish_model_alone() {
    let (_dir, path, file_before) = settings_file("");
    let settings = Mutex::new(load_settings(&path));
    let before = whole_bytes(&settings);
    let err = ensure_notes_model(
        &|| settings.lock().unwrap().polish_model.clone(),
        &|_| false,
        &mut |_| Err(NotesError::Offline),
    )
    .unwrap_err();
    assert!(matches!(err, NotesError::Offline));
    assert_eq!(whole_bytes(&settings), before);
    assert_eq!(std::fs::read(&path).unwrap(), file_before);
}

/// The same install, as the notes worker runs it: model missing → fetch →
/// summarize, with the settings shared the way `AppState.settings` is.
struct Backend {
    settings: Arc<Mutex<AppSettings>>,
    on_disk: Mutex<HashSet<String>>,
}

impl NotesBackend for Backend {
    fn model_ready(&self) -> bool {
        let chosen = self.settings.lock().unwrap().polish_model.clone();
        matches!(
            wilson_voice_lib::summarize::plan_summary_model(&chosen, &|id| self
                .on_disk
                .lock()
                .unwrap()
                .contains(id)),
            wilson_voice_lib::summarize::SummaryModelPlan::Use(_)
        )
    }
    fn fetch_model(&self, progress: &mut dyn FnMut(u8), _c: &CancelToken) -> Result<(), NotesError> {
        ensure_notes_model(
            &|| self.settings.lock().unwrap().polish_model.clone(),
            &|id| self.on_disk.lock().unwrap().contains(id),
            &mut |id| {
                progress(100);
                self.on_disk.lock().unwrap().insert(id.to_string());
                Ok(())
            },
        )
        .map(|_| ())
    }
    fn summarize(&self, _id: &str, _c: &CancelToken) -> Result<(), NotesError> {
        Ok(())
    }
}

#[test]
fn the_notes_worker_install_leaves_polish_model_byte_unchanged() {
    let (_dir, path, file_before) = settings_file("");
    let settings = Arc::new(Mutex::new(load_settings(&path)));
    let before = whole_bytes(&settings);
    let backend = Arc::new(Backend { settings: settings.clone(), on_disk: Mutex::new(HashSet::new()) });
    let jobs = SummaryJobs::spawn(backend.clone(), Arc::new(|_: &NotesStatus| {})).unwrap();
    assert!(jobs.submit("m1"));
    let end = jobs.wait_for("m1", Duration::from_secs(5)).expect("a status");
    assert_eq!(end.phase, NotesPhase::Ready);
    assert!(backend.on_disk.lock().unwrap().contains(DEFAULT_SUMMARY_MODEL));
    assert_eq!(whole_bytes(&settings), before);
    assert_eq!(settings.lock().unwrap().polish_model, "");
    assert_eq!(std::fs::read(&path).unwrap(), file_before);
}
