//! Y9-A — the transform library: ONE table of named transforms over a
//! selection, and an observable status for every run of one.
//!
//! Two kinds of row live in the same table so there is exactly one list:
//!
//! * **Deterministic** rows are YV49's command-mode edits (`command_mode.rs`):
//!   pure Rust, no model, matched by `command_mode::parse_command`. The table
//!   names them; `command_mode::apply` still does the work.
//! * **Model** rows (*Polish*, *Prompt Engineer*, *Concise*, *Formal*) run
//!   through the existing `yap-polish` sidecar via [`polish::polish_stage_chunked`]
//!   — the SAME chunker, the SAME per-request deadline, the SAME
//!   [`polish::validate_polish`] gate as a dictation rewrite. The only thing a
//!   row adds is its `prompt`, carried to the sidecar as the request's
//!   `instruction`. There is no second path to the pasteboard: a transform that
//!   is not [`TransformStatus::Succeeded`] hands back the selection byte for
//!   byte, and the caller pastes nothing.
//!
//! A new transform is a row, not a code path.
//!
//! The status enum is Wispr-shaped (`succeeded | no_changes | timeout |
//! rejected | error | not_editable`) and is persisted per invocation
//! (`transform_runs`, migration 7) so a failure is a row anybody can count,
//! never a silent no-op. `rejected` is Yap's own: it is what `validate_polish`
//! returning `None` means, kept apart from `error` so the validator's work is
//! visible rather than folded into "something broke".
//!
//! Hygiene (YV20/M2): a run records the transform id, the status, a closed-set
//! reason TAG and two character counts. Never the selection, never the model's
//! answer.

use serde::{Deserialize, Serialize};

use crate::command_mode::{self, Command};
use crate::dictation::DictationMode;
use crate::polish::{self, PolishClient, PolishConfig};

/// Whether a row runs locally in Rust or asks the polish model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransformKind {
    /// A YV49 command-mode edit — pure, instant, no model.
    Deterministic,
    /// A rewrite by the `yap-polish` sidecar, under `validate_polish`.
    Model,
}

/// One named transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TransformDef {
    /// Stable id: the fixture file name, the `transform_runs.transform_id`
    /// value, and what a per-transform shortcut binds to.
    pub id: &'static str,
    /// What the UI calls it.
    pub label: &'static str,
    /// For a model row, the instruction the sidecar is given verbatim. For a
    /// deterministic row, the rule it applies, in the same words — so every row
    /// says what it does, and a reviewer can diff the two kinds side by side.
    pub prompt: &'static str,
    /// Deterministic or model-backed.
    pub kind: TransformKind,
    /// Toast on success ("Made it formal").
    pub done: &'static str,
    /// Spoken command-mode phrases that pick a MODEL row, matched EXACTLY after
    /// `command_mode::normalized_instruction` — the same no-guessing rule as
    /// the deterministic table. Empty for deterministic rows: `parse_command`
    /// owns their phrases.
    pub phrases: &'static [&'static str],
    /// Default global shortcut, in `tauri_plugin_global_shortcut` syntax.
    /// `None` for every built-in today: binding one is the user's call and goes
    /// through the hotkey table (Y8-A); a default here is validated by
    /// `every_default_shortcut_parses_and_collides_with_nothing`.
    pub shortcut: Option<&'static str>,
}

/// The transform library. The UI, the shortcut table and command mode all read
/// THIS list.
pub const TRANSFORMS: &[TransformDef] = &[
    // ── model-backed (P1 #7 built-ins) ──────────────────────────────────────
    TransformDef {
        id: "polish",
        label: "Polish",
        prompt: "Fix grammar, punctuation, capitalization and run-on sentences, and remove filler \
                 words and false starts. Keep the wording, tone and meaning.",
        kind: TransformKind::Model,
        done: "Polished it",
        phrases: &[
            "polish",
            "polish it",
            "polish this",
            "polish that",
            "clean it up",
            "clean this up",
        ],
        shortcut: None,
    },
    TransformDef {
        id: "prompt_engineer",
        label: "Prompt Engineer",
        prompt: "Rewrite it as a clear, specific prompt for an AI assistant: state the goal, the \
                 context and the constraints it already contains, in plain imperative sentences. \
                 Do not answer it and do not add requirements it does not state.",
        kind: TransformKind::Model,
        done: "Turned it into a prompt",
        phrases: &[
            "prompt engineer",
            "prompt engineer it",
            "prompt engineer this",
            "make it a prompt",
            "turn it into a prompt",
        ],
        shortcut: None,
    },
    TransformDef {
        id: "concise",
        label: "Concise",
        prompt: "Make it more concise: cut redundant words and repetition, keep every fact, name \
                 and number.",
        kind: TransformKind::Model,
        done: "Made it concise",
        phrases: &[
            "concise",
            "make it concise",
            "make it more concise",
            "make it shorter",
            "tighten it up",
        ],
        shortcut: None,
    },
    TransformDef {
        id: "formal",
        label: "Formal",
        prompt: "Rewrite it in a formal, professional register: no slang or contractions, full \
                 sentences, terminal punctuation. Keep the meaning.",
        kind: TransformKind::Model,
        done: "Made it formal",
        phrases: &[
            "formal",
            "make it formal",
            "make it more formal",
            "make it professional",
        ],
        shortcut: None,
    },
    // ── deterministic (YV49 command mode, registered in the same table) ─────
    TransformDef {
        id: "delete",
        label: "Delete",
        prompt: "Remove the selection.",
        kind: TransformKind::Deterministic,
        done: "Deleted the selection",
        phrases: &[],
        shortcut: None,
    },
    TransformDef {
        id: "bullet_list",
        label: "Bullet list",
        prompt: "Put each line or clause of the selection on its own bulleted line.",
        kind: TransformKind::Deterministic,
        done: "Made it a list",
        phrases: &[],
        shortcut: None,
    },
    TransformDef {
        id: "upper_case",
        label: "Uppercase",
        prompt: "Uppercase every letter of the selection.",
        kind: TransformKind::Deterministic,
        done: "Made it uppercase",
        phrases: &[],
        shortcut: None,
    },
    TransformDef {
        id: "lower_case",
        label: "Lowercase",
        prompt: "Lowercase every letter of the selection.",
        kind: TransformKind::Deterministic,
        done: "Made it lowercase",
        phrases: &[],
        shortcut: None,
    },
    TransformDef {
        id: "title_case",
        label: "Title case",
        prompt: "Capitalize the first letter of every word of the selection.",
        kind: TransformKind::Deterministic,
        done: "Made it title case",
        phrases: &[],
        shortcut: None,
    },
    TransformDef {
        id: "quote",
        label: "Quote",
        prompt: "Wrap the selection in double quotes.",
        kind: TransformKind::Deterministic,
        done: "Wrapped it in quotes",
        phrases: &[],
        shortcut: None,
    },
    TransformDef {
        id: "replace",
        label: "Replace",
        prompt: "Replace every occurrence of the spoken phrase X with the spoken phrase Y.",
        kind: TransformKind::Deterministic,
        done: "Replaced the phrase",
        phrases: &[],
        shortcut: None,
    },
    TransformDef {
        id: "punctuate",
        label: "Punctuate",
        prompt: "Capitalize the first word and end the selection with terminal punctuation.",
        kind: TransformKind::Deterministic,
        done: "Added punctuation",
        phrases: &[],
        shortcut: None,
    },
    TransformDef {
        id: "one_paragraph",
        label: "One paragraph",
        prompt: "Join the selection's lines and list items into one paragraph.",
        kind: TransformKind::Deterministic,
        done: "Made it one paragraph",
        phrases: &[],
        shortcut: None,
    },
];

/// The row with this id.
pub fn find(id: &str) -> Option<&'static TransformDef> {
    TRANSFORMS.iter().find(|t| t.id == id)
}

/// The table row a YV49 command belongs to. Total over [`Command`], so a new
/// command variant does not compile until it has a row.
pub fn id_for_command(command: &Command) -> &'static str {
    match command {
        Command::Delete => "delete",
        Command::BulletList => "bullet_list",
        Command::UpperCase => "upper_case",
        Command::LowerCase => "lower_case",
        Command::TitleCase => "title_case",
        Command::Quote => "quote",
        Command::Replace { .. } => "replace",
        Command::Punctuate => "punctuate",
        Command::OneParagraph => "one_paragraph",
    }
}

/// The MODEL row a spoken command-mode instruction names, matched exactly (no
/// fuzzy matching — a guess edits text the user cannot get back). Checked only
/// AFTER `command_mode::parse_command` declined, so a deterministic command can
/// never be shadowed by a model row.
pub fn match_spoken(instruction: &str) -> Option<&'static TransformDef> {
    let normalized = command_mode::normalized_instruction(instruction);
    if normalized.is_empty() {
        return None;
    }
    TRANSFORMS
        .iter()
        .filter(|t| t.kind == TransformKind::Model)
        .find(|t| t.phrases.contains(&normalized.as_str()))
}

/// What happened to one invocation. Persisted as its snake_case tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransformStatus {
    /// The transform's output replaced the selection.
    Succeeded,
    /// It ran and the answer equals the selection — nothing to paste.
    NoChanges,
    /// The model missed the deadline (the parent's clock, or the sidecar's own).
    Timeout,
    /// The model answered and `validate_polish` (or the aggregate gate) refused
    /// the answer. Yap-specific, and deliberately NOT `error`.
    Rejected,
    /// No model, no sidecar, a panic, a protocol fault, a token overrun, or a
    /// selection the stage will not take.
    Error,
    /// Nothing selected, or the target refused the edit.
    NotEditable,
}

impl TransformStatus {
    /// Every status, in the order the docs list them.
    pub const ALL: [TransformStatus; 6] = [
        TransformStatus::Succeeded,
        TransformStatus::NoChanges,
        TransformStatus::Timeout,
        TransformStatus::Rejected,
        TransformStatus::Error,
        TransformStatus::NotEditable,
    ];

    /// The persisted tag.
    pub fn tag(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::NoChanges => "no_changes",
            Self::Timeout => "timeout",
            Self::Rejected => "rejected",
            Self::Error => "error",
            Self::NotEditable => "not_editable",
        }
    }

    /// Parse a persisted tag.
    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.tag() == tag)
    }
}

/// The status a polish reason tag means. The tags are the closed set
/// `polish::ChunkOutcome::reason` documents.
pub fn status_for_reason(reason: &str) -> TransformStatus {
    match reason {
        "deadline" | "client_deadline" => TransformStatus::Timeout,
        // `polish_attempt` reports every `validate_polish` refusal as
        // "validator"; the assembled-document gate as "aggregate".
        "validator" | "aggregate" => TransformStatus::Rejected,
        r if r.starts_with("v") && r.as_bytes().get(1).is_some_and(u8::is_ascii_digit) => {
            TransformStatus::Rejected
        }
        _ => TransformStatus::Error,
    }
}

/// One invocation's result. `text` is the edit to paste when `status` is
/// [`TransformStatus::Succeeded`], and the selection BYTE FOR BYTE otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformResult {
    pub transform_id: &'static str,
    pub status: TransformStatus,
    /// The closed-set reason tag for anything but success. Never text.
    pub reason: Option<&'static str>,
    pub text: String,
}

impl TransformResult {
    fn kept(
        def: &TransformDef,
        status: TransformStatus,
        reason: &'static str,
        selection: &str,
    ) -> Self {
        Self {
            transform_id: def.id,
            status,
            reason: Some(reason),
            text: selection.to_string(),
        }
    }

    /// The toast for this result. Every non-success says the text is unchanged,
    /// because it is.
    pub fn message(&self) -> String {
        let label = find(self.transform_id).map_or(self.transform_id, |t| t.label);
        match self.status {
            TransformStatus::Succeeded => find(self.transform_id)
                .map_or(label, |t| t.done)
                .to_string(),
            TransformStatus::NoChanges => format!("{label}: nothing to change"),
            TransformStatus::Timeout => format!("{label} took too long — your text is unchanged"),
            TransformStatus::Rejected => {
                format!("{label}: the rewrite failed Yap's safety check — your text is unchanged")
            }
            TransformStatus::Error if self.reason == Some("no_model") => {
                format!("{label} needs a polish model — download one in Settings")
            }
            TransformStatus::Error => format!("{label} failed — your text is unchanged"),
            TransformStatus::NotEditable => format!("{label}: couldn't edit this text"),
        }
    }
}

/// Run a MODEL row over `selection` against `client` — the injectable seam the
/// tests drive. Never-lose-text: every outcome except `Succeeded` returns the
/// selection unchanged.
///
/// All-or-nothing over chunks: `polish_stage_chunked` keeps a dictation's
/// rejected chunk as rules text and rewrites the rest, which is right for
/// dictation and wrong for a transform — half a selection made formal is not
/// "formal". So a transform succeeds only when EVERY chunk was accepted and the
/// assembled document passed the aggregate gate; otherwise the first chunk's
/// reason decides the status and the selection is kept whole.
pub fn run_model(
    def: &TransformDef,
    selection: &str,
    mode: DictationMode,
    cfg: &PolishConfig,
    client: &dyn PolishClient,
) -> TransformResult {
    if def.kind != TransformKind::Model {
        return TransformResult::kept(
            def,
            TransformStatus::Error,
            "not_a_model_transform",
            selection,
        );
    }
    if selection.trim().is_empty() {
        return TransformResult::kept(def, TransformStatus::NotEditable, "no_selection", selection);
    }
    if cfg.model.trim().is_empty() {
        return TransformResult::kept(def, TransformStatus::Error, "no_model", selection);
    }
    let cfg = PolishConfig {
        instruction: Some(def.prompt.to_string()),
        ..cfg.clone()
    };
    let outcome = polish::polish_stage_chunked(selection, mode, &cfg, client);
    let first_failure = outcome
        .aggregate_reason
        .or_else(|| outcome.chunks.iter().find_map(|c| c.reason));
    if let Some(reason) = first_failure {
        return TransformResult::kept(def, status_for_reason(reason), reason, selection);
    }
    if outcome.chunks.is_empty() {
        return TransformResult::kept(def, TransformStatus::Error, "not_eligible", selection);
    }
    if outcome.text.trim() == selection.trim() {
        return TransformResult::kept(def, TransformStatus::NoChanges, "no_changes", selection);
    }
    TransformResult {
        transform_id: def.id,
        status: TransformStatus::Succeeded,
        reason: None,
        text: outcome.text,
    }
}

/// Run a MODEL row against the app's own sidecar for the configured polish
/// model. `Error` / `no_model` when none is installed.
pub fn run_installed(
    def: &TransformDef,
    selection: &str,
    mode: DictationMode,
    cfg: &PolishConfig,
) -> TransformResult {
    match polish::installed_client(cfg) {
        Some(client) => run_model(def, selection, mode, cfg, &client),
        None if selection.trim().is_empty() => {
            TransformResult::kept(def, TransformStatus::NotEditable, "no_selection", selection)
        }
        None => TransformResult::kept(def, TransformStatus::Error, "no_model", selection),
    }
}

/// Run a DETERMINISTIC row (a parsed YV49 command) over `selection`. Pure.
pub fn run_command(command: &Command, selection: &str) -> TransformResult {
    let id = id_for_command(command);
    let edited = command_mode::apply(command, selection);
    if edited == selection {
        return TransformResult {
            transform_id: id,
            status: TransformStatus::NoChanges,
            reason: Some("no_changes"),
            text: selection.to_string(),
        };
    }
    TransformResult {
        transform_id: id,
        status: TransformStatus::Succeeded,
        reason: None,
        text: edited,
    }
}

/// One persisted invocation (`transform_runs`). Counts and tags only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TransformRun {
    pub transform_id: String,
    pub status: TransformStatus,
    pub reason: Option<String>,
    pub input_chars: i64,
    pub output_chars: i64,
}

impl TransformRun {
    /// The row for a result over `selection`.
    pub fn of(result: &TransformResult, selection: &str) -> Self {
        Self {
            transform_id: result.transform_id.to_string(),
            status: result.status,
            reason: result.reason.map(str::to_string),
            input_chars: selection.chars().count() as i64,
            output_chars: result.text.chars().count() as i64,
        }
    }
}

/// Migration 7 — the per-invocation status log. Appended to the YV94 ladder in
/// `db::run_migrations`; never edit it once shipped.
pub const MIGRATION_8_TRANSFORM_RUNS: &str = "
CREATE TABLE IF NOT EXISTS transform_runs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  created_at TEXT NOT NULL,
  transform_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN
    ('succeeded','no_changes','timeout','rejected','error','not_editable')),
  reason TEXT,
  input_chars INTEGER NOT NULL DEFAULT 0,
  output_chars INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_transform_runs_created ON transform_runs(created_at DESC);
";
