//! yap24-PILL2 — the backend says which stage a dictation take is in.
//!
//! Before this module the pill INFERRED a take's phase from `status.busy` and
//! the presence or absence of a `transcript` event, so the two longest waits
//! of a take — the LLM polish (bounded by the Y4-E chunked deadline) and the
//! paste-receipt wait — both read as one undifferentiated "working" phase, and
//! `pill/live.ts`'s `polishing` / `pasting` phases were declared and never
//! produced.
//!
//! ONE event, [`crate::TAKE_STAGE_EVENT`] (`"take_stage"`), payload
//! `{ stage, takeId, words? }`, emitted at the real boundaries of the pipeline:
//!
//! | stage                 | emitted by                                    |
//! |-----------------------|-----------------------------------------------|
//! | `decoding`            | `lib.rs`, immediately before `transcribe_native` |
//! | `polishing`           | [`TakeStages::polishing`] — the polish closure, so only when the LLM stage actually runs |
//! | `pasting`             | [`TakeStages::around_paste`], from `paste::insert_take` (the single insertion chooser) |
//! | `paste_confirmed` / `paste_not_confirmed` | the same [`TakeStages::around_paste`] call |
//! | `done` / `empty` / `error` / `cancelled` | `lib.rs::terminal_stage`, once, after the take's closure returns |
//!
//! Every one of them goes through [`TakeStages::emit`], the ONE emit helper,
//! which also refuses a stage the take already reported and anything after a
//! terminal stage — so no stage can be emitted twice or from two places.
//!
//! A not-confirmed paste does NOT get its own pill sentence: `paste.rs` emits
//! `insert-copy-card` on that path and the Copy card owns the message
//! (yap-insert-bridge-2026-10-26). The stage is reported so the pill can stop
//! saying "pasting"; nothing more.

use serde::Serialize;
use std::sync::Mutex;
use std::time::Instant;

/// One boundary in a dictation take. Serialized `snake_case`, the exact strings
/// `TakeStageName` in `desktop/src/pill/live.ts` accepts (held equal by
/// `take_stage_names_match_the_pill`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TakeStage {
    Decoding,
    Polishing,
    Pasting,
    PasteConfirmed,
    PasteNotConfirmed,
    Done,
    Empty,
    Error,
    Cancelled,
}

impl TakeStage {
    pub const ALL: [TakeStage; 9] = [
        TakeStage::Decoding,
        TakeStage::Polishing,
        TakeStage::Pasting,
        TakeStage::PasteConfirmed,
        TakeStage::PasteNotConfirmed,
        TakeStage::Done,
        TakeStage::Empty,
        TakeStage::Error,
        TakeStage::Cancelled,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            TakeStage::Decoding => "decoding",
            TakeStage::Polishing => "polishing",
            TakeStage::Pasting => "pasting",
            TakeStage::PasteConfirmed => "paste_confirmed",
            TakeStage::PasteNotConfirmed => "paste_not_confirmed",
            TakeStage::Done => "done",
            TakeStage::Empty => "empty",
            TakeStage::Error => "error",
            TakeStage::Cancelled => "cancelled",
        }
    }

    /// The take is over: nothing may follow a terminal stage.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            TakeStage::Done | TakeStage::Empty | TakeStage::Error | TakeStage::Cancelled
        )
    }
}

/// The `take_stage` payload. `takeId` is the take's clip stem (the same id the
/// recovery row and the chunk persistence key on); `null` only for a take that
/// failed before capture produced a clip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TakeStagePayload {
    pub stage: TakeStage,
    pub take_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub words: Option<u32>,
}

/// Where a stage goes. The app's [`tauri::AppHandle`] in production; a
/// recorder in the tests.
pub trait StageSink: Send + Sync {
    fn send(&self, payload: &TakeStagePayload) -> Result<(), String>;
}

impl<R: tauri::Runtime> StageSink for tauri::AppHandle<R> {
    fn send(&self, payload: &TakeStagePayload) -> Result<(), String> {
        use tauri::Emitter;
        self.emit(crate::TAKE_STAGE_EVENT, payload)
            .map_err(|e| e.to_string())
    }
}

struct Inner {
    take_id: Option<String>,
    emitted: Vec<TakeStage>,
    last_at: Instant,
}

/// One take's stage reporter. Created when the take's worker thread starts,
/// borrowed by every boundary that reports, dropped with the take.
pub struct TakeStages<'a> {
    sink: &'a dyn StageSink,
    started: Instant,
    inner: Mutex<Inner>,
}

impl<'a> TakeStages<'a> {
    pub fn new(sink: &'a dyn StageSink) -> Self {
        let now = Instant::now();
        Self {
            sink,
            started: now,
            inner: Mutex::new(Inner {
                take_id: None,
                emitted: Vec::new(),
                last_at: now,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The take's id, once capture has produced its clip.
    pub fn set_take_id(&self, take_id: Option<&str>) {
        self.lock().take_id = take_id.map(str::to_string);
    }

    /// The stages this take has reported, in order.
    pub fn emitted(&self) -> Vec<TakeStage> {
        self.lock().emitted.clone()
    }

    /// THE emit helper. Returns whether the stage was sent: a stage the take
    /// already reported, or anything after a terminal stage, is refused (and
    /// logged) rather than sent twice. Logs one structured line per stage with
    /// the time since the previous stage and since the take started, which is
    /// the per-stage duration the latency log never had.
    pub fn emit(&self, stage: TakeStage, words: Option<u32>) -> bool {
        let payload = {
            let mut inner = self.lock();
            if inner.emitted.contains(&stage) || inner.emitted.iter().any(|s| s.is_terminal()) {
                log::warn!(
                    "take_stage refused stage={} reason=already_reported prior={}",
                    stage.as_str(),
                    inner.emitted.len()
                );
                return false;
            }
            let now = Instant::now();
            let since_prev_ms = now.duration_since(inner.last_at).as_millis();
            inner.last_at = now;
            inner.emitted.push(stage);
            log::info!(
                "take_stage emit stage={} take={} since_prev_ms={} since_start_ms={}",
                stage.as_str(),
                inner.take_id.as_deref().unwrap_or("-"),
                since_prev_ms,
                now.duration_since(self.started).as_millis()
            );
            TakeStagePayload {
                stage,
                take_id: inner.take_id.clone(),
                words,
            }
        };
        if let Err(e) = self.sink.send(&payload) {
            log::warn!("take_stage emit_failed stage={} error={e}", stage.as_str());
        }
        true
    }

    /// Wrap the polish closure handed to `dictation::run_cleanup_*`: the
    /// cleanup pipeline calls it ONLY when the LLM stage runs for this take
    /// (level High, an eligible language), so `polishing` is reported exactly
    /// when polish actually starts and never for a polish-off take.
    pub fn polishing<'s, F>(
        &'s self,
        polish: F,
    ) -> impl Fn(&str) -> Result<String, &'static str> + 's
    where
        F: Fn(&str) -> Result<String, &'static str> + 's,
    {
        move |text| {
            self.emit(TakeStage::Polishing, None);
            polish(text)
        }
    }

    /// The paste boundary, called from `paste::insert_take` around the one
    /// insertion it makes. A copy-only take (`want_paste == false`: auto-paste
    /// off, a stale take, a Scratchpad note) never types, so it reports no
    /// paste stage at all.
    pub fn around_paste<T>(
        &self,
        want_paste: bool,
        insert: impl FnOnce() -> T,
        pasted: impl Fn(&T) -> bool,
    ) -> T {
        if !want_paste {
            return insert();
        }
        self.emit(TakeStage::Pasting, None);
        let out = insert();
        self.emit(
            if pasted(&out) {
                TakeStage::PasteConfirmed
            } else {
                TakeStage::PasteNotConfirmed
            },
            None,
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictation::{self, CleanupLevel, DictationMode};

    #[derive(Default)]
    struct Recorder(Mutex<Vec<TakeStagePayload>>);

    impl StageSink for Recorder {
        fn send(&self, payload: &TakeStagePayload) -> Result<(), String> {
            self.0.lock().unwrap().push(payload.clone());
            Ok(())
        }
    }

    impl Recorder {
        fn stages(&self) -> Vec<TakeStage> {
            self.0.lock().unwrap().iter().map(|p| p.stage).collect()
        }
    }

    const RAW: &str = "so the plan is we ship the pill today and then we test it tomorrow morning";

    /// The pipeline's own shape, in order: decode starts, the cleanup pipeline
    /// runs with the stage-wrapped polish closure the app hands it, the paste
    /// boundary runs, then the one terminal stage. Everything here is the real
    /// code the app runs EXCEPT the decode, the polish sidecar and the
    /// keystroke, which are stubbed (`polish_ok`, `pasted`).
    fn run_take(rec: &Recorder, level: CleanupLevel, want_paste: bool, pasted: bool) {
        let stages = TakeStages::new(rec);
        stages.set_take_id(Some("take-123"));
        stages.emit(TakeStage::Decoding, None);
        let polish_ok = |t: &str| -> Result<String, &'static str> { Ok(format!("{t}.")) };
        let (text, _trace) = dictation::run_cleanup_for_language(
            RAW,
            level,
            DictationMode::Plain,
            crate::structure::StructureFormat::Off,
            Default::default(),
            &[],
            "en",
            |t| t.to_string(),
            stages.polishing(polish_ok),
        );
        assert!(!text.is_empty());
        let ok = stages.around_paste(want_paste, || pasted, |p| *p);
        assert_eq!(ok, pasted);
        stages.emit(TakeStage::Done, Some(16));
    }

    #[test]
    fn take_stage_sequence_polish_on() {
        let rec = Recorder::default();
        run_take(&rec, CleanupLevel::High, true, true);
        assert_eq!(
            rec.stages(),
            vec![
                TakeStage::Decoding,
                TakeStage::Polishing,
                TakeStage::Pasting,
                TakeStage::PasteConfirmed,
                TakeStage::Done,
            ]
        );
    }

    #[test]
    fn take_stage_sequence_polish_off_never_says_polishing() {
        for level in [
            CleanupLevel::None,
            CleanupLevel::Light,
            CleanupLevel::Medium,
        ] {
            let rec = Recorder::default();
            run_take(&rec, level, true, true);
            assert_eq!(
                rec.stages(),
                vec![
                    TakeStage::Decoding,
                    TakeStage::Pasting,
                    TakeStage::PasteConfirmed,
                    TakeStage::Done,
                ],
                "level {level:?}"
            );
        }
    }

    #[test]
    fn take_stage_paste_not_confirmed_is_reported_then_done() {
        let rec = Recorder::default();
        run_take(&rec, CleanupLevel::High, true, false);
        assert_eq!(
            rec.stages(),
            vec![
                TakeStage::Decoding,
                TakeStage::Polishing,
                TakeStage::Pasting,
                TakeStage::PasteNotConfirmed,
                TakeStage::Done,
            ]
        );
    }

    #[test]
    fn take_stage_copy_only_take_reports_no_paste_stage() {
        let rec = Recorder::default();
        run_take(&rec, CleanupLevel::Medium, false, false);
        assert_eq!(rec.stages(), vec![TakeStage::Decoding, TakeStage::Done]);
    }

    #[test]
    fn take_stage_never_twice_and_nothing_after_terminal() {
        let rec = Recorder::default();
        let stages = TakeStages::new(&rec);
        assert!(stages.emit(TakeStage::Decoding, None));
        assert!(!stages.emit(TakeStage::Decoding, None));
        assert!(stages.emit(TakeStage::Error, None));
        assert!(!stages.emit(TakeStage::Done, Some(3)));
        assert!(!stages.emit(TakeStage::Pasting, None));
        assert_eq!(rec.stages(), vec![TakeStage::Decoding, TakeStage::Error]);
        assert_eq!(stages.emitted(), rec.stages());
    }

    #[test]
    fn take_stage_payload_wire_shape() {
        let p = TakeStagePayload {
            stage: TakeStage::PasteNotConfirmed,
            take_id: Some("abc".into()),
            words: None,
        };
        assert_eq!(
            serde_json::to_value(&p).unwrap(),
            serde_json::json!({ "stage": "paste_not_confirmed", "takeId": "abc" })
        );
        let d = TakeStagePayload {
            stage: TakeStage::Done,
            take_id: None,
            words: Some(7),
        };
        assert_eq!(
            serde_json::to_value(&d).unwrap(),
            serde_json::json!({ "stage": "done", "takeId": null, "words": 7 })
        );
        for s in TakeStage::ALL {
            assert_eq!(
                serde_json::to_value(s).unwrap(),
                serde_json::json!(s.as_str())
            );
        }
    }

    /// The pill's half of the wire: `live.ts` names the same event and accepts
    /// exactly these stage strings.
    #[test]
    fn take_stage_names_match_the_pill() {
        let live = include_str!("../../src/pill/live.ts");
        assert!(
            live.contains(&format!(
                "export const TAKE_STAGE_EVENT = \"{}\";",
                crate::TAKE_STAGE_EVENT
            )),
            "live.ts TAKE_STAGE_EVENT drifted from lib.rs"
        );
        for s in TakeStage::ALL {
            assert!(
                live.contains(&format!("\"{}\"", s.as_str())),
                "live.ts TakeStageName is missing {}",
                s.as_str()
            );
        }
    }
}
