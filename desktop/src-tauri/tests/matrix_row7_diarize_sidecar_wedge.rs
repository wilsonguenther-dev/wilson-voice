//! YV132 · matrix row 7 — the diarize sidecar OOMs, panics or wedges.
//!
//! ```sh
//! cargo test --test matrix_row7_diarize_sidecar_wedge
//! ```
//!
//! The published behaviour is two claims joined by an "and": *deadline + kill +
//! restart budget 1*, **and** *on give-up, a plain single-speaker transcript on
//! a meeting that is `complete` with `diarization_failed` — never `failed`.*
//! They live in two places on `main`, and this file drives both rather than
//! picking whichever one is easier to assert:
//!
//!   * the budget half is `diarize::DiarizePool`, shipping since YV121, driven
//!     here against **stub processes** — a `/bin/sh` script that never speaks, a
//!     script that announces itself and dies — so a wedge and a crash are real
//!     process events rather than a mocked error value. Zero model bytes, zero
//!     onnxruntime, no staged binary;
//!   * the give-up half is `meeting_matrix::diarize_sidecar_degrade`. Until
//!     yap-1106-diarize-wire nothing in the app called it; now
//!     `meeting_diarize::diarize_meeting` does, on every finished meeting, and
//!     the row is `Test` (see `a_crashed_sidecar_leaves_the_meeting_complete_with_plain_labels`).
//!
//! **What this file must never become.** `tests/diarize_sidecar_pool.rs` already
//! proves the pool's four policies in detail, and a row-7 test that re-asserted
//! them and stopped there would be the row-5 defect again — a green cell whose
//! test demonstrates something adjacent to the row. Row 7's subject is *what the
//! meeting is afterwards*, so every assertion below ends at a meeting: its
//! state, its marker, its transcript.

use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use wilson_voice_lib::diarize::{DiarizeError, DiarizeLauncher, DiarizePool, DiarizeState};
use wilson_voice_lib::diarize_protocol::{ERR_MODEL_LOAD_FAILED, ERR_MODEL_NOT_FOUND};
use wilson_voice_lib::meeting_matrix::{
    diarize_sidecar_degrade, DegradeReason, SpeakerLabels, DIARIZATION_FAILED,
};
use wilson_voice_lib::meetings::MeetingState;

/// A stub sidecar: `/bin/sh` running `script`, counting how many times it was
/// launched — which is how "one restart per session" is observed rather than
/// asserted.
fn counted_stub(script: &'static str, launches: Arc<AtomicUsize>) -> DiarizeLauncher {
    Box::new(move || {
        launches.fetch_add(1, Ordering::Relaxed);
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg(script);
        Ok(command)
    })
}

/// Never announces readiness and never exits — a child that is not starting.
/// The closest a stub gets to an OOM-thrashing ONNX session build.
const WEDGED_STUB: &str = "sleep 30\n";

/// Announces readiness and immediately exits — a child that panicked after the
/// handshake, which is what an OOM kill looks like from the parent's side.
const DYING_STUB: &str = r#"printf '{"type":"ready","version":"stub"}\n'"#;

fn model_pair() -> (std::path::PathBuf, std::path::PathBuf) {
    (
        std::path::PathBuf::from("/nonexistent/segmentation.onnx"),
        std::path::PathBuf::from("/nonexistent/embedding.onnx"),
    )
}

/// A wedged child is killed at the readiness budget, and the meeting that was
/// waiting on it is **complete with a plain transcript**, not failed.
#[test]
fn a_wedged_sidecar_is_killed_and_the_meeting_still_completes() {
    let launches = Arc::new(AtomicUsize::new(0));
    let budget = Duration::from_millis(300);
    let pool = DiarizePool::new(counted_stub(WEDGED_STUB, Arc::clone(&launches)), budget);
    let (segmentation, embedding) = model_pair();

    let started = Instant::now();
    let err = pool
        .load_models(&segmentation, &embedding)
        .expect_err("a child that never speaks cannot load models");
    let waited = started.elapsed();

    // The deadline half: bounded, and bounded by the budget rather than by the
    // 30 s the stub would otherwise sleep for.
    assert_eq!(err, DiarizeError::Unavailable);
    assert!(
        waited >= budget && waited < Duration::from_secs(5),
        "waited {waited:?} against a {budget:?} readiness budget"
    );
    // The kill half: the child is gone, not left running behind a failed job.
    assert!(
        !pool.is_warm(),
        "the wedged child was killed, not abandoned"
    );
    assert_eq!(pool.status().state, DiarizeState::Failed);
    assert_eq!(pool.status().reason, Some("ready_timeout"));

    // And the meeting. This is the row's actual subject.
    let labels = diarize_sidecar_degrade(&err);
    assert_eq!(
        labels,
        SpeakerLabels::Plain(DegradeReason::Sidecar {
            tag: "unavailable".to_string()
        })
    );
    assert!(labels.is_plain(), "a plain single-speaker transcript");
    assert_eq!(labels.marker(), Some(DIARIZATION_FAILED));
    assert_eq!(
        labels.meeting_state_after(MeetingState::Complete),
        MeetingState::Complete,
        "the user still gets their notes: a wedged diarizer never fails a meeting"
    );
}

/// A child that dies is respawned exactly once; the second death gives up — and
/// the give-up is still a complete meeting.
#[test]
fn the_restart_budget_is_one_and_giving_up_is_still_a_complete_meeting() {
    let launches = Arc::new(AtomicUsize::new(0));
    let pool = DiarizePool::new(
        counted_stub(DYING_STUB, Arc::clone(&launches)),
        Duration::from_secs(10),
    );
    let (segmentation, embedding) = model_pair();

    // Bounded, so a pool that never gives up fails this test instead of hanging
    // the suite.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut last = Err(DiarizeError::Protocol);
    while Instant::now() < deadline && pool.status().state != DiarizeState::Failed {
        last = pool.load_models(&segmentation, &embedding);
    }
    assert_eq!(pool.status().state, DiarizeState::Failed);
    assert_eq!(pool.status().reason, Some("died"));
    assert_eq!(
        launches.load(Ordering::Relaxed),
        2,
        "one spawn plus exactly one restart, per session"
    );
    assert!(last.is_err(), "the last attempt did not produce segments");

    let labels = diarize_sidecar_degrade(&last.unwrap_err());
    assert_eq!(labels.marker(), Some(DIARIZATION_FAILED));
    for outcome in [MeetingState::Complete, MeetingState::Partial] {
        assert_eq!(
            labels.meeting_state_after(outcome),
            outcome,
            "diarization giving up does not move a meeting's state in either direction"
        );
    }
}

/// **Every** way the sidecar can fail to answer degrades the same way. Table
/// driven over the whole error set, because the failure mode this row is really
/// guarding against is one variant that somebody handled differently — and it
/// would be the rarest one, found by a user and not by a test.
#[test]
fn no_sidecar_failure_can_produce_a_failed_meeting() {
    let errors = [
        DiarizeError::Unavailable,
        DiarizeError::Deadline,
        DiarizeError::Protocol,
        // The refusal tags are spent from the protocol's own constants rather
        // than typed out. YV122 RETIRED `no_backend` — the condition it named
        // (a sidecar with no inference crate compiled in) stopped existing when
        // sherpa-onnx landed — and this table went on driving over it, which is
        // a test asserting behaviour for an answer the child can no longer
        // give. Naming the constants means the next retirement is a compile
        // error here instead of a quietly decorative row.
        DiarizeError::Refused(ERR_MODEL_NOT_FOUND.to_string()),
        DiarizeError::Refused(ERR_MODEL_LOAD_FAILED.to_string()),
    ];

    for err in &errors {
        let labels = diarize_sidecar_degrade(err);
        assert!(labels.is_plain(), "{err:?} did not degrade");
        assert_eq!(labels.marker(), Some(DIARIZATION_FAILED), "{err:?}");
        assert_eq!(
            labels.reason().map(|r| r.log_tag()),
            Some("sidecar"),
            "{err:?} is row 7's failure and must log as one"
        );
        for state in [
            MeetingState::Complete,
            MeetingState::Partial,
            MeetingState::Failed,
        ] {
            assert_eq!(
                labels.meeting_state_after(state),
                state,
                "{err:?} changed a meeting's state, which is not diarization's to change"
            );
        }
        // The tag is the POOL's own word, not a second vocabulary — so a log
        // line and `DiarizeStatus` cannot disagree about which failure it was.
        assert_eq!(
            labels.reason(),
            Some(&DegradeReason::Sidecar {
                tag: err.tag().to_string()
            }),
            "{err:?}"
        );
    }

    // Non-vacuous: the five errors do not all collapse to one indistinguishable
    // outcome. Two refusals with different tags stay different.
    assert_ne!(
        diarize_sidecar_degrade(&DiarizeError::Refused(ERR_MODEL_NOT_FOUND.to_string())),
        diarize_sidecar_degrade(&DiarizeError::Refused(ERR_MODEL_LOAD_FAILED.to_string()))
    );
    assert_ne!(
        diarize_sidecar_degrade(&DiarizeError::Deadline),
        diarize_sidecar_degrade(&DiarizeError::Protocol)
    );
}

/// The log line a human reads when this happens says which failure it was, in
/// one line, with no path and no sentence from the child in it.
#[test]
fn the_give_up_logs_as_a_sidecar_failure_and_says_the_transcript_survived() {
    let line = diarize_sidecar_degrade(&DiarizeError::Deadline)
        .reason()
        .expect("a degrade carries a reason")
        .log_line();
    assert!(line.contains(DIARIZATION_FAILED), "{line}");
    assert!(line.contains("reason=sidecar"), "{line}");
    assert!(line.contains("detail=deadline"), "{line}");
    assert!(
        line.contains("transcript is intact"),
        "the line a human reads has to say what SURVIVED: {line}"
    );
}

/// yap-1106-diarize-wire promoted row 7 to `Test`: the shipping surface is
/// `meeting_diarize::diarize_meeting`, which `run_meeting_job` calls on every
/// finished meeting. A sidecar that CRASHES on its first request (and again on
/// its one restart) leaves the meeting `complete`, the transcript with its
/// plain labels (no `cluster_index` written), the degrade marker, and the
/// `diarize result=failed` log line.
#[test]
fn a_crashed_sidecar_leaves_the_meeting_complete_with_plain_labels() {
    use wilson_voice_lib::meeting_diarize::{diarize_meeting, DiarizeBackend, DiarizeResult};
    use wilson_voice_lib::meetings::{render_transcript, MeetingKind, NewMeetingSegment};

    let dir = std::env::temp_dir().join(format!(
        "yap-row7-crash-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let db = wilson_voice_lib::db::Database::open(dir.join("wilson_voice.db")).unwrap();
    let meeting = db
        .create_meeting_with_kind("Crash", "manual", MeetingKind::InPerson)
        .unwrap();
    db.append_meeting_segments(
        &meeting.id,
        &[
            NewMeetingSegment::new(0.0, 4.0, "first voice"),
            NewMeetingSegment::new(4.0, 8.0, "second voice"),
        ],
    )
    .unwrap();
    let wav = dir.join("mic.wav");
    std::fs::write(&wav, b"RIFF....WAVEfake").unwrap();
    db.finish_meeting(&meeting.id, 8.0, Some(&wav)).unwrap();
    db.set_meeting_state(&meeting.id, MeetingState::Complete, None)
        .unwrap();

    let launches = Arc::new(AtomicUsize::new(0));
    let crash: DiarizeLauncher = {
        let launches = launches.clone();
        Box::new(move || {
            launches.fetch_add(1, Ordering::Relaxed);
            let mut command = std::process::Command::new("/bin/sh");
            command.arg("-c").arg(concat!(
                r#"printf '{"type":"ready","version":"stub"}\n'"#,
                "\nread -r line\nexit 3\n"
            ));
            Ok::<std::process::Command, DiarizeError>(command)
        })
    };
    let pool = Arc::new(DiarizePool::new(crash, Duration::from_secs(10)));
    let outcome = diarize_meeting(&db, &meeting.id, &dir, &DiarizeBackend::Pool(pool.clone()));
    pool.shutdown();

    assert!(
        matches!(outcome.result, DiarizeResult::Failed(_)),
        "{:?}",
        outcome.result
    );
    assert!(outcome.labels.is_plain());
    assert_eq!(outcome.labels.marker(), Some(DIARIZATION_FAILED));
    assert!(outcome
        .log_line(&meeting.id)
        .starts_with("diarize result=failed clusters=0 speech_s=0.0 wall_ms="));
    assert!(launches.load(Ordering::Relaxed) >= 1);

    let after = db.get_meeting(&meeting.id).unwrap().unwrap();
    assert_eq!(after.state, MeetingState::Complete.as_str());
    let rows = db.list_meeting_segments(&meeting.id).unwrap();
    assert!(rows.iter().all(|r| r.cluster_index.is_none()));
    let labels: Vec<String> = render_transcript(&rows, MeetingKind::InPerson)
        .iter()
        .map(|l| l.label())
        .collect();
    assert_eq!(labels, vec!["Speaker".to_string(), "Speaker".to_string()]);
    let _ = std::fs::remove_dir_all(&dir);
}
