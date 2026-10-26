//! YV99 · error-matrix row 16 — "sleep / lid close mid-meeting".
//!
//! Required behaviour: `NSWorkspaceWillSleepNotification` finalizes the journal
//! and marks the meeting `paused_by_sleep`; on wake, Yap OFFERS resume as a new
//! segment rather than pretending the gap did not happen.
//!
//! This row is why YV91's power assertion is not enough on its own:
//! `kIOPMAssertionTypePreventUserIdleSystemSleep` cannot stop a lid close or a
//! user-initiated sleep. The two are complements. The assertion buys the idle
//! case; this state machine has to be honest about the case it cannot buy.
//!
//! Everything below is a sequence of notifications fed to a pure state machine,
//! because the alternative instrument is closing a laptop lid by hand and the
//! interesting cases (a duplicate notification, a wake with no preceding sleep,
//! a sleep after the user already stopped) are ones a lid cannot produce on
//! demand.
//!
//! **The observer exists since Y1-B, and the first tests below drive it.**
//! `power::SleepWakeObserver` registers `NSWorkspaceWillSleepNotification` and
//! `NSWorkspaceDidWakeNotification` on the workspace notification centre and
//! publishes ONE [`SleepEvent`] to a `power::SleepWakeHub`, which fans it out
//! to the three consumers `lib.rs` subscribes: dictation, meetings (this row's
//! [`SleepPolicy`], via `SleepPolicy::drive`) and PERM-E's permission re-read.
//! The tests post the real notifications on the real centre — the same objects
//! macOS posts — so they exercise the shipping registration, not a stand-in.
//!
//! **What these tests do NOT prove.** They do not close a lid: macOS posting
//! the notification on a real sleep is AppKit's contract, not something an
//! in-process test can trigger. The meeting closure in `lib.rs` delegates the
//! whole meeting-side action to `MeetingController::on_sleep_wake`, which
//! `will_sleep_stops_a_real_meeting_with_paused_by_sleep` drives against a
//! real controller (fix-239). The dictation closure needs a Tauri host
//! (`cancel_recording`, the global Escape shortcut), so its half is checked
//! against the source, not run.

mod support;

use std::sync::{Arc, Mutex};

use wilson_voice_lib::meeting_control::{MeetingController, MeetingStatus, SleepStep, StatusSink};
use wilson_voice_lib::meeting_energy::MeetingDiagnostics;
use wilson_voice_lib::meeting_matrix::{
    Coverage, SleepAction, SleepEvent, SleepPolicy, PAUSED_BY_SLEEP, ROWS,
};
use wilson_voice_lib::permissions::{self, WatchReason};
use wilson_voice_lib::power::{
    self, SleepWakeHub, SleepWakeObserver, CONSUMER_DICTATION, CONSUMER_MEETINGS,
    CONSUMER_PERMISSIONS, SLEEP_WAKE_CONSUMERS,
};

/// Every test that posts on the shared workspace centre takes this, so one
/// test's post never lands in another test's observer.
static CENTRE: Mutex<()> = Mutex::new(());

fn centre_lock() -> std::sync::MutexGuard<'static, ()> {
    CENTRE.lock().unwrap_or_else(|p| p.into_inner())
}

/// Post one of the two notifications on `NSWorkspace.sharedWorkspace
/// .notificationCenter` — the centre macOS itself posts them on.
#[cfg(target_os = "macos")]
fn post(event: SleepEvent) {
    use objc2_app_kit::{
        NSWorkspace, NSWorkspaceDidWakeNotification, NSWorkspaceWillSleepNotification,
    };
    let centre = NSWorkspace::sharedWorkspace().notificationCenter();
    // SAFETY: framework constant names; no sender object.
    unsafe {
        let name = match event {
            SleepEvent::WillSleep => NSWorkspaceWillSleepNotification,
            SleepEvent::DidWake => NSWorkspaceDidWakeNotification,
        };
        centre.postNotificationName_object(name, None);
    }
}

fn src(file: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The source with comment lines and string literals removed — a comment
/// naming a call site is not a call site (that is how row 16 was misread).
fn code_of(body: &str) -> String {
    let mut out = String::new();
    for line in body.lines() {
        let t = line.trim_start();
        if t.starts_with("//") || t.starts_with('*') || t.starts_with("/*") {
            continue;
        }
        let mut in_string = false;
        let mut escaped = false;
        let mut prev_slash = false;
        for c in t.chars() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    in_string = false;
                }
                continue;
            }
            if c == '/' && prev_slash {
                out.pop();
                break;
            }
            prev_slash = c == '/';
            if c == '"' {
                in_string = true;
            } else {
                out.push(c);
            }
        }
        out.push('\n');
    }
    out
}

/// The row and the code agree: row 16 is a `Test` row anchored to the shipping
/// observer in `power.rs`, and its cell no longer says NOT WIRED.
#[test]
fn row16_is_no_longer_policy_only() {
    let row = ROWS.iter().find(|r| r.id == "16").expect("row 16");
    assert!(
        !matches!(row.coverage, Coverage::PolicyOnly { .. }),
        "row 16 still published as PolicyOnly: {:?}",
        row.coverage
    );
    assert_eq!(
        row.coverage,
        Coverage::Test {
            test: "matrix_row16_sleep_wake.rs",
            subject: "SleepWakeObserver",
            subject_module: "power.rs",
        }
    );
    let cell = row.coverage.cell();
    assert!(!cell.contains("NOT WIRED"), "{cell}");
    // …and the code backs the cell: power.rs registers both notifications as
    // CODE, not in a comment.
    let power_code = code_of(&src("power.rs"));
    for name in [
        "NSWorkspaceWillSleepNotification",
        "NSWorkspaceDidWakeNotification",
        "addObserverForName_object_queue_usingBlock",
        "removeObserver",
    ] {
        assert!(
            power_code.contains(name),
            "power.rs does not call `{name}` as code"
        );
    }
}

/// ONE observer, registered in `power.rs` and nowhere else, fans each event
/// out to dictation, meetings and permissions.
#[cfg(target_os = "macos")]
#[test]
fn one_observer_fans_out_to_dictation_meetings_and_permissions() {
    let _centre = centre_lock();

    // The three consumers, each calling the same decision function its
    // production closure in lib.rs calls.
    let dictation: Arc<Mutex<Vec<bool>>> = Arc::default();
    let meetings: Arc<Mutex<Vec<SleepAction>>> = Arc::default();
    let perms: Arc<Mutex<Vec<Option<WatchReason>>>> = Arc::default();
    let meeting_recording = Arc::new(Mutex::new(true));

    let hub = Arc::new(SleepWakeHub::new());
    {
        let d = Arc::clone(&dictation);
        hub.subscribe(CONSUMER_DICTATION, move |e| {
            d.lock()
                .unwrap()
                .push(power::dictation_should_park_take(e, true));
        });
        let m = Arc::clone(&meetings);
        let rec = Arc::clone(&meeting_recording);
        let policy = Mutex::new(SleepPolicy::new());
        hub.subscribe(CONSUMER_MEETINGS, move |e| {
            let now = *rec.lock().unwrap();
            let action = policy.lock().unwrap().drive(now, e);
            if action == SleepAction::FinalizeAndPause {
                // what lib.rs does: controller.stop(PAUSED_BY_SLEEP)
                *rec.lock().unwrap() = false;
            }
            m.lock().unwrap().push(action);
        });
        let p = Arc::clone(&perms);
        hub.subscribe(CONSUMER_PERMISSIONS, move |e| {
            p.lock().unwrap().push(permissions::on_sleep_wake(e));
        });
        // A duplicate consumer is refused rather than doubling its actions.
        hub.subscribe(CONSUMER_PERMISSIONS, |_| {
            panic!("second permissions consumer ran")
        });
    }
    assert_eq!(hub.consumers(), SLEEP_WAKE_CONSUMERS.to_vec());

    let observer = SleepWakeObserver::register(Arc::clone(&hub));
    assert_eq!(observer.registered(), 2, "will-sleep and did-wake");

    post(SleepEvent::WillSleep);
    post(SleepEvent::DidWake);

    assert_eq!(
        *dictation.lock().unwrap(),
        vec![true, false],
        "park on sleep only"
    );
    assert_eq!(
        *meetings.lock().unwrap(),
        vec![
            SleepAction::FinalizeAndPause,
            SleepAction::OfferResumeAsNewSegment
        ]
    );
    assert!(
        !*meeting_recording.lock().unwrap(),
        "meeting stopped {PAUSED_BY_SLEEP}"
    );
    assert_eq!(*perms.lock().unwrap(), vec![None, Some(WatchReason::Wake)]);
    observer.unregister();

    // The production wiring: lib.rs subscribes exactly these three to the one
    // hub and installs it once; no other file registers the notifications.
    let lib = code_of(&src("lib.rs"));
    for consumer in [
        "CONSUMER_DICTATION",
        "CONSUMER_MEETINGS",
        "CONSUMER_PERMISSIONS",
    ] {
        assert!(
            lib.contains(&format!("hub.subscribe(power::{consumer}")),
            "lib.rs does not subscribe power::{consumer}"
        );
    }
    assert!(lib.contains("controller.on_sleep_wake(&mut policy, event)"));
    assert!(lib.contains("permissions::on_sleep_wake(event)"));
    assert!(lib.contains("power::dictation_should_park_take(event, in_flight)"));
    assert_eq!(
        lib.matches("power::install_sleep_wake_observer(").count(),
        1
    );

    let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut stack = vec![src_dir];
    let mut scanned = 0;
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read src").flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") || path.ends_with("power.rs") {
                continue;
            }
            scanned += 1;
            let code = code_of(&std::fs::read_to_string(&path).unwrap_or_default());
            for name in [
                "NSWorkspaceWillSleepNotification",
                "NSWorkspaceDidWakeNotification",
            ] {
                assert!(
                    !code.contains(name),
                    "{} registers `{name}` — power.rs owns the ONE observer",
                    path.display()
                );
            }
        }
    }
    assert!(scanned > 10, "src scan found almost nothing");
}

/// An observer that outlives the app is a crash. After unregister, a posted
/// notification reaches nobody; a second install is refused while one is live.
#[cfg(target_os = "macos")]
#[test]
fn the_observer_is_unregistered_on_shutdown() {
    let _centre = centre_lock();

    let heard: Arc<Mutex<Vec<SleepEvent>>> = Arc::default();
    let hub = Arc::new(SleepWakeHub::new());
    let h = Arc::clone(&heard);
    hub.subscribe(CONSUMER_MEETINGS, move |e| h.lock().unwrap().push(e));

    assert!(!power::sleep_wake_observer_installed());
    assert!(power::install_sleep_wake_observer(Arc::clone(&hub)));
    assert!(power::sleep_wake_observer_installed());
    assert!(
        !power::install_sleep_wake_observer(Arc::new(SleepWakeHub::new())),
        "never a second observer"
    );

    post(SleepEvent::WillSleep);
    assert_eq!(*heard.lock().unwrap(), vec![SleepEvent::WillSleep]);

    assert!(
        power::unregister_sleep_wake_observer(),
        "shutdown removes it"
    );
    assert!(!power::sleep_wake_observer_installed());
    post(SleepEvent::DidWake);
    post(SleepEvent::WillSleep);
    assert_eq!(
        *heard.lock().unwrap(),
        vec![SleepEvent::WillSleep],
        "a removed observer must hear nothing"
    );
    assert!(!power::unregister_sleep_wake_observer(), "idempotent");

    // Dropping a locally registered observer removes it too.
    let local = SleepWakeObserver::register(Arc::clone(&hub));
    drop(local);
    post(SleepEvent::WillSleep);
    assert_eq!(heard.lock().unwrap().len(), 1, "Drop unregisters");

    // And the app calls it: RunEvent::Exit unregisters.
    let lib = code_of(&src("lib.rs"));
    let exit = lib
        .find("tauri::RunEvent::Exit")
        .expect("lib.rs handles RunEvent::Exit");
    assert!(
        lib[exit..].contains("power::unregister_sleep_wake_observer()"),
        "RunEvent::Exit does not unregister the sleep/wake observer"
    );
}

/// `drive` syncs the policy to the controller before deciding.
#[test]
fn drive_follows_the_controller_not_a_second_copy_of_its_state() {
    let mut p = SleepPolicy::new();
    // Idle policy, controller recording → the meeting is finalized on sleep.
    assert_eq!(
        p.drive(true, SleepEvent::WillSleep),
        SleepAction::FinalizeAndPause
    );
    assert_eq!(
        p.drive(false, SleepEvent::DidWake),
        SleepAction::OfferResumeAsNewSegment
    );
    // A new meeting started while the offer was outstanding supersedes it.
    assert_eq!(
        p.drive(true, SleepEvent::WillSleep),
        SleepAction::FinalizeAndPause
    );
    // The user stopped the meeting before sleeping → nothing to do.
    let mut q = SleepPolicy::new();
    q.meeting_started();
    assert_eq!(q.drive(false, SleepEvent::WillSleep), SleepAction::Ignore);
}

fn recording() -> SleepPolicy {
    let mut p = SleepPolicy::new();
    p.meeting_started();
    p
}

#[test]
fn sleep_finalizes_and_marks_paused_by_sleep() {
    let mut p = recording();
    assert!(p.is_recording());

    assert_eq!(
        p.observe(SleepEvent::WillSleep),
        SleepAction::FinalizeAndPause
    );
    assert!(
        !p.is_recording(),
        "capture must stop before the machine goes down"
    );
    assert!(
        p.paused_by_sleep(),
        "the meeting row records why it has a seam"
    );
    assert_eq!(PAUSED_BY_SLEEP, "paused_by_sleep");
}

#[test]
fn wake_offers_resume_as_a_new_segment_never_a_pretended_continuity() {
    let mut p = recording();
    p.observe(SleepEvent::WillSleep);

    assert_eq!(
        p.observe(SleepEvent::DidWake),
        SleepAction::OfferResumeAsNewSegment,
        "the plan's own words: offer resume-as-new-segment, not a pretended-continuous gap"
    );
    assert!(p.awaiting_resume());
    assert!(
        !p.is_recording(),
        "waking must not silently resume capture — it is an offer, not a decision"
    );

    assert!(p.resume_accepted());
    assert!(p.is_recording());
    assert!(
        !p.paused_by_sleep(),
        "the seam is closed once the user resumes"
    );
}

#[test]
fn declining_the_offer_ends_the_meeting_cleanly() {
    let mut p = recording();
    p.observe(SleepEvent::WillSleep);
    p.observe(SleepEvent::DidWake);

    assert!(p.resume_declined());
    assert!(!p.is_recording());
    assert!(!p.awaiting_resume());
    // A second answer to a prompt that is gone changes nothing.
    assert!(!p.resume_accepted());
    assert!(!p.is_recording());
}

/// Dark wakes and repeated notifications are normal macOS behaviour. A second
/// prompt stacked on the first — or a second finalize of a journal that is
/// already closed — is the bug this asserts against.
#[test]
fn duplicate_notifications_are_idempotent() {
    let mut p = recording();

    assert_eq!(
        p.observe(SleepEvent::WillSleep),
        SleepAction::FinalizeAndPause
    );
    assert_eq!(
        p.observe(SleepEvent::WillSleep),
        SleepAction::Ignore,
        "the journal is already finalized; finalizing twice is a bug"
    );

    assert_eq!(
        p.observe(SleepEvent::DidWake),
        SleepAction::OfferResumeAsNewSegment
    );
    assert_eq!(
        p.observe(SleepEvent::DidWake),
        SleepAction::Ignore,
        "one outstanding prompt, however many wakes"
    );
    assert_eq!(
        p.observe(SleepEvent::WillSleep),
        SleepAction::Ignore,
        "sleeping again with a prompt outstanding must not stack a second one"
    );
}

/// The case that must not become a silent continuous recording: we woke still
/// believing we were capturing, so the will-sleep notification never arrived or
/// arrived too late to act on. There IS a gap in the audio either way.
#[test]
fn a_wake_with_no_preceding_sleep_never_pretends_the_gap_did_not_happen() {
    let mut p = recording();

    assert_eq!(
        p.observe(SleepEvent::DidWake),
        SleepAction::FinalizeMissedSleepAndOfferResume,
        "finalize what is on disk AND offer resume — never treat the two sides as continuous"
    );
    assert!(!p.is_recording());
    assert!(p.awaiting_resume());
}

#[test]
fn sleep_while_idle_does_nothing_at_all() {
    let mut p = SleepPolicy::new();
    assert_eq!(p.observe(SleepEvent::WillSleep), SleepAction::Ignore);
    assert_eq!(p.observe(SleepEvent::DidWake), SleepAction::Ignore);
    assert!(!p.paused_by_sleep());
    assert!(!p.awaiting_resume());
}

/// A meeting the user already stopped must never come back from a resume
/// prompt — that would be the app arguing with them.
#[test]
fn a_user_stop_cancels_any_pending_resume() {
    let mut p = recording();
    p.observe(SleepEvent::WillSleep);
    p.observe(SleepEvent::DidWake);
    assert!(p.awaiting_resume());

    p.meeting_stopped();
    assert!(!p.awaiting_resume());
    assert!(!p.resume_accepted());
    assert_eq!(p.observe(SleepEvent::DidWake), SleepAction::Ignore);
}

/// The full lid-close arc, as one sequence, because the row is about the arc and
/// not about any single transition: record → close the lid → open it → resume →
/// close it again → open it again → stop.
#[test]
fn two_lid_closes_in_one_meeting_produce_two_offers_and_no_lost_state() {
    let mut p = recording();

    for round in 1..=2 {
        assert_eq!(
            p.observe(SleepEvent::WillSleep),
            SleepAction::FinalizeAndPause,
            "round {round}"
        );
        assert_eq!(
            p.observe(SleepEvent::DidWake),
            SleepAction::OfferResumeAsNewSegment,
            "round {round}"
        );
        assert!(p.resume_accepted(), "round {round}");
        assert!(p.is_recording(), "round {round}");
    }

    p.meeting_stopped();
    assert!(!p.is_recording());
}

/// fix-239 — the finalize half of row 16, driven end to end: the REAL observer
/// hears a posted will-sleep, the hub calls `MeetingController::on_sleep_wake`
/// (the one call the `lib.rs` meeting closure makes), and a REAL recording
/// meeting is stopped with the `paused_by_sleep` reason, read back from the
/// meeting row's diagnostics blob. The wake that follows reports the offer
/// (row 16b) and stops nothing.
#[cfg(target_os = "macos")]
#[test]
fn will_sleep_stops_a_real_meeting_with_paused_by_sleep() {
    let _guard = support::exclusive();
    let _centre = centre_lock();
    support::install_fake_engine();
    support::set_fake_mode(support::FAKE_OK);
    let dir = support::temp_dir("y1b-row16-finalize");
    let db = Arc::new(support::open_db(&dir));
    let sink: StatusSink = Arc::new(|_: &MeetingStatus| {});
    let controller = Arc::new(MeetingController::new(Arc::clone(&db), sink));
    let id = controller
        .start(&dir, Some("Lid closes".into()))
        .expect("start");
    assert!(controller.is_recording());

    let steps: Arc<Mutex<Vec<SleepStep>>> = Arc::default();
    let hub = Arc::new(SleepWakeHub::new());
    {
        let c = Arc::clone(&controller);
        let st = Arc::clone(&steps);
        let policy = Mutex::new(SleepPolicy::new());
        hub.subscribe(CONSUMER_MEETINGS, move |e| {
            let mut policy = policy.lock().unwrap();
            st.lock().unwrap().push(c.on_sleep_wake(&mut policy, e));
        });
    }
    let observer = SleepWakeObserver::register(Arc::clone(&hub));

    post(SleepEvent::WillSleep);
    assert!(!controller.is_recording(), "will-sleep stopped the meeting");
    post(SleepEvent::DidWake);
    observer.unregister();

    assert_eq!(
        *steps.lock().unwrap(),
        vec![SleepStep::Finalized, SleepStep::ResumeOffered]
    );
    let row = db.get_meeting(&id).unwrap().expect("meeting row");
    let blob = row
        .diagnostics
        .expect("the stopped meeting carries diagnostics");
    let d: MeetingDiagnostics = serde_json::from_str(&blob).expect("valid JSON");
    assert_eq!(d.stop_reason.as_deref(), Some(PAUSED_BY_SLEEP));

    // And `lib.rs` hands the meeting to exactly this method.
    let lib = code_of(&src("lib.rs"));
    assert!(lib.contains("controller.on_sleep_wake(&mut policy, event)"));
}

/// fix-239 — a take armed Escape as a system-wide shortcut when it started.
/// `cancel_recording` does not release it (only `cancel_take` and the end of
/// the transcribe worker do), and a sleep-cancel spawns no worker, so the
/// dictation consumer must release it itself or the first Escape after wake is
/// eaten in every app. Checked against the source: the closure needs a Tauri
/// host to run.
#[test]
fn the_sleep_cancel_gives_escape_back() {
    let lib = code_of(&src("lib.rs"));
    let start = lib
        .find("hub.subscribe(power::CONSUMER_DICTATION")
        .expect("lib.rs subscribes the dictation consumer");
    let end = start
        + lib[start..]
            .find("hub.subscribe(power::CONSUMER_MEETINGS")
            .expect("the meetings consumer follows it");
    let dictation = &lib[start..end];
    let cancel = dictation
        .find("cancel_recording(&dict_app, &dict_state);")
        .expect("the sleep consumer cancels the take");
    let release = dictation
        .find("unregister_cancel_shortcut(&dict_app);")
        .expect("the sleep consumer must release the Escape grab");
    assert!(
        release > cancel,
        "Escape is released after the take is cancelled"
    );
}
