//! Y10-A — multi-language dictation: a language SET, auto-detect held to it,
//! driven off the PROBED model capability, and the English-shaped cleanup
//! stages skipped for a non-English take.
//!
//! The decode tests drive the real `TranscriptionManager` and the real
//! `languages::decode_in_set` with a scripted engine (no GGUF download), so
//! what they prove is the production orchestration, not a re-implementation.
//! The meeting path is untouched (see `meeting_english_only_gate.rs`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use wilson_voice_lib::dictation::{self, CleanupLevel, DictationMode, Style};
use wilson_voice_lib::languages::{
    self, CleanupStage, DecodeMode, LanguagePrefs, LanguageSupport, Resolution,
};
use wilson_voice_lib::transcribe_progress::NoObserver;
use wilson_voice_lib::transcription::{Transcriber, TranscriptionManager};

/// A scripted engine: reports `support` as its probed capabilities, and when
/// it is run WITHOUT a hint "detects" `detects`. Every hint it is handed is
/// recorded, in order.
struct ScriptedEngine {
    support: Option<LanguageSupport>,
    detects: Option<String>,
    last: Option<String>,
    hints: Arc<Mutex<Vec<Option<String>>>>,
}

impl Transcriber for ScriptedEngine {
    fn transcribe(
        &mut self,
        _samples: &[f32],
        language: Option<&str>,
        _bias_prompt: Option<&str>,
    ) -> Result<String, String> {
        self.hints
            .lock()
            .unwrap()
            .push(language.map(str::to_string));
        self.last = match language {
            None => self.detects.clone(),
            Some(_) => None,
        };
        Ok(format!("decoded as {}", language.unwrap_or("auto")))
    }

    fn language_support(&self) -> Option<LanguageSupport> {
        self.support.clone()
    }

    fn last_detected_language(&self) -> Option<String> {
        self.last.clone()
    }
}

fn model_file(tag: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("yap-languages-{tag}-{}.gguf", std::process::id()));
    std::fs::write(&path, b"stub").unwrap();
    path
}

fn manager(
    support: Option<LanguageSupport>,
    detects: Option<&str>,
) -> (TranscriptionManager, Arc<Mutex<Vec<Option<String>>>>) {
    let hints = Arc::new(Mutex::new(Vec::new()));
    let h = hints.clone();
    let detects = detects.map(str::to_string);
    let m = TranscriptionManager::with_loader(
        Arc::new(move |_p: &Path| {
            Ok(Box::new(ScriptedEngine {
                support: support.clone(),
                detects: detects.clone(),
                last: None,
                hints: h.clone(),
            }) as Box<dyn Transcriber>)
        }),
        Duration::from_secs(300),
        Duration::from_secs(60),
        Duration::from_secs(30),
    );
    (m, hints)
}

fn whisper_like() -> LanguageSupport {
    // What a multilingual model probes as: detection on, ~99 languages listed.
    let mut languages: Vec<String> = ["en", "es", "fr", "de", "it", "ht", "pt"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    languages.extend((0..92).map(|i| format!("x{i}")));
    LanguageSupport {
        supports_detect: true,
        languages,
    }
}

/// Multilingual list, but no language identification: it can decode Spanish
/// when told to, it just cannot tell which language it is hearing.
fn no_detect() -> LanguageSupport {
    LanguageSupport {
        supports_detect: false,
        languages: vec!["en".into(), "es".into()],
    }
}

fn english_only() -> LanguageSupport {
    LanguageSupport {
        supports_detect: false,
        languages: vec!["en".into()],
    }
}

fn en_es_auto() -> LanguagePrefs {
    LanguagePrefs::from_settings("en", &["en".into(), "es".into()], true)
}

fn decode(m: &TranscriptionManager, path: &Path, prefs: &LanguagePrefs) -> languages::LanguageTake {
    m.load("stub", path).expect("load");
    languages::decode_in_set(
        m,
        "stub",
        path,
        vec![0.1; 16_000],
        prefs,
        None,
        &mut NoObserver,
    )
    .expect("decode")
}

#[test]
fn a_model_without_detect_support_falls_back_and_says_so() {
    // The plan, from the probe alone.
    let plan = languages::plan_decode(&en_es_auto(), &no_detect());
    assert_eq!(plan.mode, DecodeMode::FallbackNoDetect);
    assert_eq!(plan.hint.as_deref(), Some("en"));
    let notice = plan.notice.clone().expect("a fallback must say so");
    assert!(notice.contains("can't detect"), "{notice}");
    assert!(notice.contains("English"), "{notice}");

    // The real orchestration: the engine is handed the primary, never `None`.
    let path = model_file("nodetect");
    let (m, hints) = manager(Some(no_detect()), Some("es"));
    let take = decode(&m, &path, &en_es_auto());
    assert_eq!(*hints.lock().unwrap(), vec![Some("en".to_string())]);
    assert_eq!(take.language, "en");
    assert_eq!(take.mode, DecodeMode::FallbackNoDetect);
    assert!(take
        .notice
        .as_deref()
        .unwrap_or("")
        .contains("can't detect"));
    assert_eq!(take.take.text, "decoded as en");

    // An engine that reports NOTHING is not assumed to detect either.
    let (m, hints) = manager(None, Some("es"));
    let take = decode(&m, &path, &en_es_auto());
    assert_eq!(*hints.lock().unwrap(), vec![Some("en".to_string())]);
    assert_eq!(take.mode, DecodeMode::FallbackNoDetect);

    // The picker's view carries the same notice, and the probe's verdict.
    let opts = languages::language_options(&en_es_auto(), Some(&no_detect()));
    assert_eq!(opts.supports_detect, Some(false));
    assert_eq!(opts.mode, DecodeMode::FallbackNoDetect);
    assert!(opts.notice.unwrap().contains("can't detect"));
    // It offers only what the model can transcribe, and an English-only model
    // asked for Spanish says that too.
    let opts = languages::language_options(&en_es_auto(), Some(&english_only()));
    assert!(opts.notice.unwrap().contains("can't transcribe Spanish"));
    assert_eq!(
        opts.offered
            .iter()
            .map(|c| c.code.as_str())
            .collect::<Vec<_>>(),
        vec!["en"]
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn auto_detect_only_chooses_within_the_set() {
    let path = model_file("auto");

    // Detected inside the set: accepted, one decode, no hint.
    let (m, hints) = manager(Some(whisper_like()), Some("es"));
    let take = decode(&m, &path, &en_es_auto());
    assert_eq!(take.mode, DecodeMode::AutoWithinSet);
    assert_eq!(take.language, "es");
    assert!(!take.redecoded);
    assert_eq!(*hints.lock().unwrap(), vec![None]);

    // Detected OUTSIDE the set (Italian for an en/es speaker): never accepted —
    // re-decoded with the primary as the hint.
    let (m, hints) = manager(Some(whisper_like()), Some("it"));
    let take = decode(&m, &path, &en_es_auto());
    assert_eq!(take.language, "en");
    assert!(take.redecoded);
    assert_eq!(*hints.lock().unwrap(), vec![None, Some("en".to_string())]);
    assert_eq!(take.take.text, "decoded as en");

    // No detection reported at all: also held to the set.
    let plan = languages::plan_decode(&en_es_auto(), &whisper_like());
    assert_eq!(
        languages::resolve_detected(&plan, None),
        Resolution::Redecode("en".into())
    );
    assert_eq!(plan.candidates, vec!["en".to_string(), "es".to_string()]);

    // 99 advertised languages are NOT 99 choices: the picker offers its short
    // list, and the set is capped.
    let opts = languages::language_options(&en_es_auto(), Some(&whisper_like()));
    assert!(opts.offered.len() <= languages::PICKER_LANGUAGES.len());
    assert!(opts.offered.iter().all(|c| !c.code.starts_with('x')));
    let many: Vec<String> = ["en", "es", "fr", "de", "it", "pt"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let capped = LanguagePrefs::from_settings("fr", &many, true);
    assert_eq!(capped.set.len(), languages::MAX_LANGUAGE_SET);
    assert_eq!(
        capped.set[0], "fr",
        "the primary is always in the set, first"
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn english_shaped_stages_are_skipped_for_a_non_english_take() {
    // The per-stage statement.
    for stage in [
        CleanupStage::Backtrack,
        CleanupStage::ListFormat,
        CleanupStage::SpokenMarks,
        CleanupStage::EmailShape,
        CleanupStage::Structure,
        CleanupStage::Polish,
    ] {
        assert!(!stage.language_agnostic(), "{stage:?} is English-shaped");
        assert!(!stage.runs_for("de"), "{stage:?} must not run on German");
        assert!(stage.runs_for("en"));
    }
    for stage in [
        CleanupStage::Dictionary,
        CleanupStage::Paragraphs,
        CleanupStage::TrailingPeriod,
    ] {
        assert!(stage.language_agnostic(), "{stage:?}");
        assert!(stage.runs_for("de"));
    }
    assert_eq!(CleanupStage::ALL.len(), 9);

    // The pipeline itself. The spoken-punctuation table, the backtrack filler
    // list and the polish model are all English; a German take that happens
    // to contain "comma" or "um" keeps them verbatim.
    let raw = "um hallo comma wie geht es dir period";
    let run = |lang: &str, polish_calls: &Mutex<u32>, dict_calls: &Mutex<u32>| {
        dictation::run_cleanup_for_language(
            raw,
            CleanupLevel::High,
            DictationMode::Email,
            wilson_voice_lib::structure::StructureFormat::Off,
            Style::Default,
            &[],
            lang,
            |t| {
                *dict_calls.lock().unwrap() += 1;
                t.to_string()
            },
            |t| {
                *polish_calls.lock().unwrap() += 1;
                Ok(t.to_string())
            },
        )
    };

    // Control: English DOES run the English stages on this exact text, so
    // the German assertions below are not vacuous.
    let (p, d) = (Mutex::new(0), Mutex::new(0));
    let (en_text, en_trace) = run("en", &p, &d);
    assert!(
        en_text.contains(','),
        "spoken comma converted for English: {en_text}"
    );
    assert!(
        !en_text.starts_with("um"),
        "filler dropped for English: {en_text}"
    );
    assert_eq!(*p.lock().unwrap(), 1, "polish ran for English");
    assert_ne!(en_trace.llm_skip_reason, Some("non_english_take"));

    let (p, d) = (Mutex::new(0), Mutex::new(0));
    let (de_text, de_trace) = run("de", &p, &d);
    assert_eq!(de_text, raw, "no English rule touched the German take");
    assert_eq!(*p.lock().unwrap(), 0, "the English polish never saw German");
    assert_eq!(de_trace.llm_skip_reason, Some("non_english_take"));
    assert_eq!(*d.lock().unwrap(), 1, "the user's dictionary still applies");
}

#[test]
fn the_default_remains_english_for_an_existing_install() {
    // A settings file written before the language set existed.
    let dir = std::env::temp_dir().join(format!("yap-lang-settings-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    std::fs::write(&path, r#"{"language":"en","autoPaste":true}"#).unwrap();
    let settings = wilson_voice_lib::load_settings(&path);
    assert!(settings.language_set.is_empty());
    let prefs = settings.language_prefs();
    assert_eq!(prefs.primary, "en");
    assert_eq!(prefs.set, vec!["en".to_string()]);
    // Even on a model that CAN detect, a one-language set decodes English.
    let plan = languages::plan_decode(&prefs, &whisper_like());
    assert_eq!(plan.mode, DecodeMode::Single);
    assert_eq!(plan.hint.as_deref(), Some("en"));
    assert!(plan.notice.is_none());

    // A fresh install is English too.
    let fresh = wilson_voice_lib::AppSettings::default().language_prefs();
    assert_eq!(fresh.primary, "en");
    assert_eq!(fresh.set, vec!["en".to_string()]);

    // An existing non-English install keeps its one language — it is not
    // silently widened into an auto-detect.
    std::fs::write(&path, r#"{"language":"fr"}"#).unwrap();
    let fr = wilson_voice_lib::load_settings(&path).language_prefs();
    assert_eq!(fr.set, vec!["fr".to_string()]);
    assert_eq!(
        languages::plan_decode(&fr, &whisper_like()).hint.as_deref(),
        Some("fr")
    );
    let _ = std::fs::remove_dir_all(dir);
}
