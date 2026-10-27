//! Y9-A — the named transform library (`src/transforms.rs`).
//!
//! Every model-backed case runs the REAL validated stage
//! (`polish::polish_stage_chunked` → `validate_polish`) against an injected
//! `PolishClient`, the same seam `polish.rs`'s own tests use, so no model and no
//! sidecar are needed and every failure path is forced on purpose.
//!
//! The golden fixtures live in `tests/fixtures/transforms/<id>.jsonl`, one file
//! per table row (see `tests/fixtures/README.md`).

use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::Value;
use wilson_voice_lib::command_mode;
use wilson_voice_lib::db::Database;
use wilson_voice_lib::dictation::{DictationMode, Style};
use wilson_voice_lib::polish::{PolishClient, PolishConfig, PolishError};
use wilson_voice_lib::polish_protocol::PolishRequest;
use wilson_voice_lib::transforms::{
    self, TransformKind, TransformRun, TransformStatus, TRANSFORMS,
};

/// The stage ON: a model id is configured; the client is injected, so no file
/// is ever read.
fn on() -> PolishConfig {
    PolishConfig {
        model: "qwen2.5-1.5b-instruct-q4_k_m".to_string(),
        deadline_ms: 1200,
        style: Style::Default,
        preferences: Vec::new(),
        instruction: None,
    }
}

/// A selection long enough for the stage to take (>= MIN_POLISH_WORDS).
const SELECTION: &str = "so um can you send over the quote when you get a chance thanks";

/// Answers with a fixed string and records every request it was handed.
struct ScriptedClient {
    answer: String,
    seen: Mutex<Vec<PolishRequest>>,
}

impl ScriptedClient {
    fn new(answer: &str) -> Self {
        Self {
            answer: answer.to_string(),
            seen: Mutex::new(Vec::new()),
        }
    }
    fn calls(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}

impl PolishClient for ScriptedClient {
    fn rewrite(&self, req: &PolishRequest) -> Result<String, PolishError> {
        self.seen.lock().unwrap().push(req.clone());
        Ok(self.answer.clone())
    }
}

/// Answers a perfectly good rewrite — long after the deadline.
struct SleepingClient;
impl PolishClient for SleepingClient {
    fn rewrite(&self, req: &PolishRequest) -> Result<String, PolishError> {
        std::thread::sleep(Duration::from_millis(req.deadline_ms + 150));
        Ok(req.text.to_uppercase())
    }
}

/// The sidecar's own deadline answer (`{"ok":false,"err":"deadline"}`).
struct SidecarDeadlineClient;
impl PolishClient for SidecarDeadlineClient {
    fn rewrite(&self, _req: &PolishRequest) -> Result<String, PolishError> {
        Err(PolishError::Deadline)
    }
}

struct PanickingClient;
impl PolishClient for PanickingClient {
    fn rewrite(&self, _req: &PolishRequest) -> Result<String, PolishError> {
        panic!("client exploded mid-rewrite");
    }
}

fn model_rows() -> impl Iterator<Item = &'static transforms::TransformDef> {
    TRANSFORMS.iter().filter(|t| t.kind == TransformKind::Model)
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/transforms")
}

fn fixture(id: &str) -> Vec<Value> {
    let path = fixtures_dir().join(format!("{id}.jsonl"));
    let body = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("no golden fixture for transform `{id}` at {path:?}: {e}"));
    body.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            serde_json::from_str(l).unwrap_or_else(|e| panic!("{id}.jsonl: bad line {l}: {e}"))
        })
        .collect()
}

fn field<'a>(case: &'a Value, key: &str, id: &str) -> &'a str {
    case[key]
        .as_str()
        .unwrap_or_else(|| panic!("{id}.jsonl: a case is missing `{key}`: {case}"))
}

#[test]
fn every_table_row_has_a_label_and_a_prompt() {
    let mut ids = std::collections::BTreeSet::new();
    for t in TRANSFORMS {
        assert!(!t.id.trim().is_empty(), "a row has no id");
        assert!(ids.insert(t.id), "duplicate transform id `{}`", t.id);
        assert!(!t.label.trim().is_empty(), "`{}` has no label", t.id);
        assert!(
            !t.prompt.trim().is_empty(),
            "`{}` has an empty prompt",
            t.id
        );
        assert!(!t.done.trim().is_empty(), "`{}` has no success toast", t.id);
        assert_eq!(transforms::find(t.id), Some(t));
    }
    // The four P1 #7 built-ins are model rows, the nine YV49 edits are
    // deterministic rows, and they are all in the ONE table.
    for (id, label) in [
        ("polish", "Polish"),
        ("prompt_engineer", "Prompt Engineer"),
        ("concise", "Concise"),
        ("formal", "Formal"),
    ] {
        let row = transforms::find(id).unwrap_or_else(|| panic!("built-in `{id}` missing"));
        assert_eq!(row.label, label);
        assert_eq!(row.kind, TransformKind::Model);
        assert!(!row.phrases.is_empty(), "`{id}` cannot be spoken");
    }
    let deterministic = TRANSFORMS
        .iter()
        .filter(|t| t.kind == TransformKind::Deterministic)
        .count();
    assert_eq!(deterministic, 9, "the nine YV49 command-mode edits");
    assert_eq!(TRANSFORMS.len(), 13);
}

/// Every YV49 command lands on a deterministic row of the table — the mapping
/// is total, so command mode records its runs under the table's ids.
#[test]
fn every_command_mode_edit_is_a_table_row() {
    for phrase in [
        "delete that",
        "make it a list",
        "make it uppercase",
        "make it lowercase",
        "make it title case",
        "wrap in quotes",
        "replace cat with dog",
        "add punctuation",
        "make it one paragraph",
    ] {
        let command = command_mode::parse_command(phrase)
            .unwrap_or_else(|| panic!("`{phrase}` is a YV49 command"));
        let id = transforms::id_for_command(&command);
        let row = transforms::find(id).unwrap_or_else(|| panic!("`{id}` has no row"));
        assert_eq!(row.kind, TransformKind::Deterministic, "`{id}`");
    }
}

/// One golden file per row, and no file that names no row.
#[test]
fn a_golden_fixture_per_transform() {
    for t in TRANSFORMS {
        let cases = fixture(t.id);
        assert!(!cases.is_empty(), "`{}`'s fixture is empty", t.id);
        for case in &cases {
            let name = field(case, "case", t.id);
            let input = field(case, "input", t.id);
            let want_status = TransformStatus::from_tag(field(case, "expected_status", t.id))
                .unwrap_or_else(|| panic!("{}.jsonl `{name}`: unknown status", t.id));
            let want_text = field(case, "expected_text", t.id);
            let got = match t.kind {
                TransformKind::Model => {
                    let client = ScriptedClient::new(field(case, "model_output", t.id));
                    let got =
                        transforms::run_model(t, input, DictationMode::Document, &on(), &client);
                    // The row's prompt is what reached the sidecar, verbatim.
                    let seen = client.seen.lock().unwrap();
                    assert!(
                        !seen.is_empty(),
                        "{} `{name}`: the model was never asked",
                        t.id
                    );
                    for req in seen.iter() {
                        assert_eq!(
                            req.instruction.as_deref(),
                            Some(t.prompt),
                            "{} `{name}`",
                            t.id
                        );
                        assert_eq!(
                            req.deadline_ms,
                            on().deadline_ms,
                            "{} `{name}`: same deadline",
                            t.id
                        );
                    }
                    got
                }
                TransformKind::Deterministic => {
                    let spoken = field(case, "instruction", t.id);
                    let command = command_mode::parse_command(spoken).unwrap_or_else(|| {
                        panic!("{} `{name}`: `{spoken}` is not a command", t.id)
                    });
                    assert_eq!(
                        transforms::id_for_command(&command),
                        t.id,
                        "{} `{name}`",
                        t.id
                    );
                    transforms::run_command(&command, input)
                }
            };
            assert_eq!(got.transform_id, t.id);
            assert_eq!(got.status, want_status, "{} `{name}`: status", t.id);
            assert_eq!(got.text, want_text, "{} `{name}`: text", t.id);
            if want_status != TransformStatus::Succeeded {
                assert_eq!(
                    got.text.as_bytes(),
                    input.as_bytes(),
                    "{} `{name}`: never lose text",
                    t.id
                );
            }
        }
    }
    for entry in std::fs::read_dir(fixtures_dir()).unwrap() {
        let path = entry.unwrap().path();
        assert_eq!(
            path.extension().and_then(|e| e.to_str()),
            Some("jsonl"),
            "{path:?}"
        );
        let stem = path.file_stem().unwrap().to_str().unwrap();
        assert!(
            transforms::find(stem).is_some(),
            "fixture {path:?} names no table row"
        );
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(
            body.is_ascii(),
            "{path:?}: fixtures are ASCII, invented text only"
        );
    }
}

#[test]
fn a_forced_timeout_leaves_the_input_byte_identical() {
    let short = PolishConfig {
        deadline_ms: 20,
        ..on()
    };
    for t in model_rows() {
        // The parent's clock: the answer arrives after the deadline.
        let got = transforms::run_model(
            t,
            SELECTION,
            DictationMode::Document,
            &short,
            &SleepingClient,
        );
        assert_eq!(got.status, TransformStatus::Timeout, "`{}`", t.id);
        assert_eq!(got.reason, Some("deadline"));
        assert_eq!(got.text.as_bytes(), SELECTION.as_bytes(), "`{}`", t.id);
        // The sidecar's own deadline answer is a timeout too, not an error.
        let got = transforms::run_model(
            t,
            SELECTION,
            DictationMode::Document,
            &on(),
            &SidecarDeadlineClient,
        );
        assert_eq!(got.status, TransformStatus::Timeout, "`{}`", t.id);
        assert_eq!(got.text.as_bytes(), SELECTION.as_bytes(), "`{}`", t.id);
    }
}

#[test]
fn a_garbage_client_yields_rejected_not_error() {
    for garbage in [
        "<|im_start|>assistant\nSure, here is the rewrite",
        "",
        "Certainly! Call 555 0199 for details.",
        "quote quote quote quote quote quote quote quote quote quote quote quote quote quote quote \
         quote quote quote quote quote quote quote quote quote quote quote quote quote quote quote \
         quote quote quote quote quote quote quote quote quote quote quote quote quote quote quote",
    ] {
        for t in model_rows() {
            let client = ScriptedClient::new(garbage);
            let got = transforms::run_model(t, SELECTION, DictationMode::Document, &on(), &client);
            assert_eq!(got.status, TransformStatus::Rejected, "`{}` on {garbage:?}", t.id);
            assert_ne!(got.status, TransformStatus::Error);
            assert_eq!(got.text.as_bytes(), SELECTION.as_bytes(), "`{}`", t.id);
        }
    }
}

#[test]
fn a_panicking_client_yields_error_and_the_input_byte_identical() {
    for t in model_rows() {
        let got = transforms::run_model(
            t,
            SELECTION,
            DictationMode::Document,
            &on(),
            &PanickingClient,
        );
        assert_eq!(got.status, TransformStatus::Error, "`{}`", t.id);
        assert_eq!(got.reason, Some("client_panic"));
        assert_eq!(got.text.as_bytes(), SELECTION.as_bytes(), "`{}`", t.id);
    }
}

#[test]
fn no_changes_when_the_model_returns_the_input() {
    for t in model_rows() {
        let client = ScriptedClient::new(SELECTION);
        let got = transforms::run_model(t, SELECTION, DictationMode::Document, &on(), &client);
        assert_eq!(got.status, TransformStatus::NoChanges, "`{}`", t.id);
        assert_eq!(got.text.as_bytes(), SELECTION.as_bytes());
        assert_eq!(client.calls(), 1);
    }
}

/// The failures that never reach the model still say so.
#[test]
fn no_model_no_selection_and_code_are_observable() {
    let t = transforms::find("formal").unwrap();
    let client = ScriptedClient::new("Never asked.");
    let off = PolishConfig::default();
    let got = transforms::run_model(t, SELECTION, DictationMode::Document, &off, &client);
    assert_eq!(
        (got.status, got.reason),
        (TransformStatus::Error, Some("no_model"))
    );
    let got = transforms::run_model(t, "   ", DictationMode::Document, &on(), &client);
    assert_eq!(
        (got.status, got.reason),
        (TransformStatus::NotEditable, Some("no_selection"))
    );
    assert_eq!(got.text, "   ");
    // V8: code never reaches a model, transform or not.
    let got = transforms::run_model(t, SELECTION, DictationMode::Code, &on(), &client);
    assert_eq!(
        (got.status, got.reason),
        (TransformStatus::Error, Some("not_eligible"))
    );
    assert_eq!(got.text.as_bytes(), SELECTION.as_bytes());
    assert_eq!(client.calls(), 0, "nothing above may reach the model");
}

/// A transform is all-or-nothing: one rejected chunk keeps the WHOLE selection,
/// where dictation would have kept the other chunks' rewrites.
#[test]
fn a_partly_rejected_long_selection_is_kept_whole() {
    struct FirstChunkOnly(Mutex<usize>);
    impl PolishClient for FirstChunkOnly {
        fn rewrite(&self, req: &PolishRequest) -> Result<String, PolishError> {
            let mut n = self.0.lock().unwrap();
            *n += 1;
            if *n == 1 {
                // A real, valid edit: dictation would KEEP this chunk.
                Ok(req.text.replacen("the quarterly", "The quarterly", 1))
            } else {
                Ok(String::new())
            }
        }
    }
    let paragraph = "the quarterly planning review covers hiring budgets roadmap risks and vendor \
                     contracts for every team in detail. ";
    let selection = paragraph.repeat(14);
    let t = transforms::find("polish").unwrap();
    let got = transforms::run_model(
        t,
        &selection,
        DictationMode::Document,
        &on(),
        &FirstChunkOnly(Mutex::new(0)),
    );
    assert_eq!(got.status, TransformStatus::Rejected);
    assert_eq!(got.text.as_bytes(), selection.as_bytes());
    // Control: the same client through the dictation stage keeps chunk 1's
    // edit — so the assertion above is not vacuous.
    let dictation = wilson_voice_lib::polish::polish_stage_chunked(
        &selection,
        DictationMode::Document,
        &on(),
        &FirstChunkOnly(Mutex::new(0)),
    );
    assert!(
        dictation.chunks.len() >= 2,
        "the selection must span chunks"
    );
    assert!(
        dictation.text.starts_with("The quarterly"),
        "dictation keeps the accepted chunk"
    );
}

#[test]
fn spoken_names_pick_model_rows_and_never_shadow_a_command() {
    assert_eq!(
        transforms::match_spoken("Make it formal.").map(|t| t.id),
        Some("formal")
    );
    assert_eq!(
        transforms::match_spoken("please polish it").map(|t| t.id),
        Some("polish")
    );
    assert_eq!(
        transforms::match_spoken("Prompt engineer this").map(|t| t.id),
        Some("prompt_engineer")
    );
    assert_eq!(
        transforms::match_spoken("make it concise").map(|t| t.id),
        Some("concise")
    );
    // Exact, never fuzzy: a near miss edits nothing.
    assert_eq!(
        transforms::match_spoken("make it formal and also shorter"),
        None
    );
    assert_eq!(transforms::match_spoken(""), None);
    for t in model_rows() {
        for phrase in t.phrases {
            assert!(
                command_mode::parse_command(phrase).is_none(),
                "`{phrase}` is also a deterministic command"
            );
            assert_eq!(transforms::match_spoken(phrase).map(|r| r.id), Some(t.id));
        }
    }
}

#[test]
fn the_status_enum_is_the_six_wispr_shaped_tags() {
    let tags: Vec<&str> = TransformStatus::ALL.iter().map(|s| s.tag()).collect();
    assert_eq!(
        tags,
        [
            "succeeded",
            "no_changes",
            "timeout",
            "rejected",
            "error",
            "not_editable"
        ]
    );
    for s in TransformStatus::ALL {
        assert_eq!(TransformStatus::from_tag(s.tag()), Some(s));
        assert_eq!(
            serde_json::to_value(s).unwrap(),
            Value::String(s.tag().into())
        );
    }
    // `rejected` is the validator's verdict and stays apart from `error`.
    assert_eq!(
        transforms::status_for_reason("validator"),
        TransformStatus::Rejected
    );
    assert_eq!(
        transforms::status_for_reason("aggregate"),
        TransformStatus::Rejected
    );
    assert_eq!(
        transforms::status_for_reason("v3_retention"),
        TransformStatus::Rejected
    );
    assert_eq!(
        transforms::status_for_reason("client_deadline"),
        TransformStatus::Timeout
    );
    for err in [
        "client_panic",
        "no_sidecar",
        "protocol",
        "max_out",
        "not_eligible",
    ] {
        assert_eq!(
            transforms::status_for_reason(err),
            TransformStatus::Error,
            "{err}"
        );
    }
}

/// Each invocation is a row: tags and counts, never the text.
#[test]
fn every_invocation_is_persisted_with_its_status() {
    let db = Database::open_in_memory().expect("in-memory db");
    let t = transforms::find("formal").unwrap();
    let short = PolishConfig {
        deadline_ms: 20,
        ..on()
    };
    let timed_out = transforms::run_model(
        t,
        SELECTION,
        DictationMode::Document,
        &short,
        &SleepingClient,
    );
    let rejected = transforms::run_model(
        t,
        SELECTION,
        DictationMode::Document,
        &on(),
        &ScriptedClient::new(""),
    );
    db.record_transform_run(&TransformRun::of(&timed_out, SELECTION))
        .unwrap();
    db.record_transform_run(&TransformRun::of(&rejected, SELECTION))
        .unwrap();
    let rows = db.list_transform_runs(10).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].status, TransformStatus::Rejected, "newest first");
    assert_eq!(rows[1].status, TransformStatus::Timeout);
    assert_eq!(rows[1].transform_id, "formal");
    assert_eq!(rows[1].reason.as_deref(), Some("deadline"));
    assert_eq!(rows[1].input_chars, SELECTION.chars().count() as i64);
    let dump = format!("{rows:?}");
    assert!(
        !dump.contains("quote"),
        "a run row must never carry the selection"
    );
}

/// A default shortcut is optional, but one that is set must parse and must not
/// steal a key Yap already registers.
#[test]
fn every_default_shortcut_parses_and_collides_with_nothing() {
    use tauri_plugin_global_shortcut::Shortcut;
    for t in TRANSFORMS {
        if let Some(keys) = t.shortcut {
            let parsed = Shortcut::from_str(keys)
                .unwrap_or_else(|e| panic!("`{}` shortcut {keys:?} does not parse: {e}", t.id));
            for other in TRANSFORMS.iter().filter(|o| o.id != t.id) {
                if let Some(theirs) = other.shortcut {
                    assert_ne!(
                        Shortcut::from_str(theirs).ok(),
                        Some(parsed),
                        "{} vs {}",
                        t.id,
                        other.id
                    );
                }
            }
        }
    }
}
