//! yap-notetaker-notes — PRIVACY: with a local model, the meeting-notes path
//! makes ZERO network calls.
//!
//! Every outbound HTTP request in the app takes its client from
//! `net::http_client`. This test installs `net::DenyAll`, which PANICS on the
//! first request, then runs a meeting through the real notes worker
//! (`SummaryJobs`, the same per-meeting job lock every trigger uses) with a
//! backend whose fetch step is the app's real model-download path and whose
//! summary step is the real `summarize::summarize_segments_with` over a
//! two-track (You / Call) transcript. A local model is on disk, so the job must
//! reach Ready with no egress at all.
//!
//! The positive control proves the seam is not vacuous: the same backend
//! with NO model on disk reaches the deny-all egress through the real
//! download code and panics there. A seam nothing calls would pass the first
//! test and fail this one.

mod support;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

use support::{labels_in, segments_from, StubModel};
use wilson_voice_lib::meetings::{MeetingKind, MeetingSegment};
use wilson_voice_lib::polish_protocol::PolishRequest;
use wilson_voice_lib::summarize::{summarize_segments_with, SummaryError, TrackSpeakers};
use wilson_voice_lib::summary_jobs::{
    classify_download_error, ensure_notes_model, CancelToken, NotesBackend, NotesError, NotesPhase,
    SummaryJobs,
};
use wilson_voice_lib::{models, net};

/// Both tests share the process-global egress and its counter.
static SERIAL: Mutex<()> = Mutex::new(());
static SETUP: Once = Once::new();

fn setup() {
    SETUP.call_once(|| {
        // A fresh data dir, so a notes model already downloaded on this Mac
        // can never make the positive control skip its download.
        let root = std::env::temp_dir().join(format!("yap-stay-local-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("temp data dir");
        std::env::set_var("YAP_DATA_DIR", &root);
        net::install(Arc::new(net::DenyAll));
    });
}

fn call_segments() -> Vec<MeetingSegment> {
    let mut segs = segments_from(&[
        "Okay, this is the launch review for the notetaker.",
        "We agreed the export ships with checkboxes for action items.",
        "Jeisil will send the release notes to the team by Friday.",
        "Wilson to record the demo video next week.",
    ]);
    // Alternate tracks: the call shape whose turns read You / Call.
    for (i, s) in segs.iter_mut().enumerate() {
        s.track = (i % 2) as i64;
    }
    segs
}

fn answer(req: &PolishRequest) -> Result<String, SummaryError> {
    Ok(match req.mode.as_str() {
        "map" => {
            let ids = labels_in(&req.text);
            let id = |i: usize| ids.get(i).cloned().unwrap_or_else(|| ids[0].clone());
            format!(
                r#"{{"narrative":"The team reviewed the notetaker launch. The export ships with checkboxes for action items.","actions":[{{"text":"Jeisil will send the release notes to the team by Friday.","segment":"{}"}}],"decisions":[{{"text":"The export ships with checkboxes for action items.","segment":"{}"}}],"questions":[]}}"#,
                id(2),
                id(1)
            )
        }
        "reduce" => "The team reviewed the notetaker launch.".to_string(),
        "headline" => r#"{"title":"Notetaker launch review","key_points":["The export ships with checkboxes for action items.","Jeisil will send the release notes by Friday.","Wilson will record the demo video next week."]}"#.to_string(),
        other => panic!("unexpected stage {other}"),
    })
}

/// The notes backend with the app's real download path and the real summary.
struct LocalNotes {
    model_on_disk: bool,
    stored: Mutex<Option<String>>,
}

impl NotesBackend for LocalNotes {
    fn model_ready(&self) -> bool {
        self.model_on_disk
    }

    fn fetch_model(
        &self,
        _progress: &mut dyn FnMut(u8),
        cancel: &CancelToken,
    ) -> Result<(), NotesError> {
        let on_disk = self.model_on_disk;
        let mut download = |m: &str| -> Result<(), NotesError> {
            let token = cancel.clone();
            let stop = move || token.is_cancelled();
            tauri::async_runtime::block_on(models::download_polish_model_cancellable(
                m,
                |_, _| {},
                &stop,
            ))
            .map(|_| ())
            .map_err(|e| classify_download_error(&e))
        };
        ensure_notes_model(&String::new, &|_| on_disk, &mut download).map(|_| ())
    }

    fn summarize(&self, _id: &str, _cancel: &CancelToken) -> Result<(), NotesError> {
        let segs = call_segments();
        let model = StubModel::with_headline(answer);
        let speakers = TrackSpeakers::for_meeting(MeetingKind::Virtual, &segs);
        let s = summarize_segments_with(&segs, &model, &speakers)
            .map_err(|e| NotesError::Summary(format!("{e:?}")))?;
        *self.stored.lock().unwrap() = Some(s.markdown);
        Ok(())
    }
}

#[test]
fn local_model_notes_path_makes_zero_network_calls() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    setup();
    let before = net::egress_requests();

    let backend = Arc::new(LocalNotes {
        model_on_disk: true,
        stored: Mutex::new(None),
    });
    let jobs = SummaryJobs::spawn(backend.clone(), Arc::new(|_| {})).expect("notes worker");
    assert!(jobs.submit("m-local"), "the job is queued");
    let status = jobs
        .wait_for("m-local", Duration::from_secs(30))
        .expect("a final status");

    assert_eq!(status.phase, NotesPhase::Ready, "status: {status:?}");
    assert_eq!(
        net::egress_requests(),
        before,
        "the notes path asked for an HTTP client"
    );
    let md = backend
        .stored
        .lock()
        .unwrap()
        .clone()
        .expect("notes stored");
    assert!(md.contains("Jeisil will send the release notes"), "{md}");
    // The summary's attribution reads the call's turns: track 1 is "Call".
    assert!(md.contains("(Call, "), "{md}");
}

#[test]
fn positive_control_a_needed_download_hits_the_deny_all_seam() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    setup();
    let before = net::egress_requests();
    let backend = LocalNotes {
        model_on_disk: false,
        stored: Mutex::new(None),
    };
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        backend.fetch_model(&mut |_| {}, &CancelToken::new())
    }));
    let panic = outcome.expect_err("the deny-all egress must panic on a real download");
    let msg = panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default();
    assert!(
        msg.contains("network egress denied purpose=model-download"),
        "{msg}"
    );
    assert_eq!(net::egress_requests(), before + 1);
}
