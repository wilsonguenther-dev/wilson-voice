//! Matrix row 1 — **system-audio (tap) permission denied at start**.
//!
//! Required behaviour (plan §6): the meeting still records, mic-only, badged
//! "system audio unavailable", never aborted, with one deep link to the right
//! Settings pane.
//!
//! **Published as `Test` since YV110, and the promotion was forced rather than
//! chosen.** This file used to assert that `syscapture::start_system_tap` had
//! no caller — the row's own tripwire, aimed at the one thing that was missing.
//! YV110 gave it one (`meeting_control::open_meeting_tap`, from
//! `SessionEngine::start`), the assertion went red with its own
//! promote-the-row instruction, and the row was promoted. So the file now
//! drives four things instead of three:
//!
//!   * the **decision at T-0** — `syscapture::track_b_plan`, the shipping
//!     function a meeting asks whether it may attach Track B, and the sentence
//!     it hands back when the answer is no. This is the row's subject: what row
//!     1 requires is that a denial costs the second track and nothing else;
//!   * the **discriminator** — can this app tell a denied tap from a granted
//!     one that had nothing to record? — driven against the shipping fold YV104
//!     merged, including the case where getting it wrong means accusing a
//!     user's privacy settings of something that never happened;
//!   * the **caller**, now asserted present rather than absent, in the same
//!     scan that used to assert the opposite;
//!   * the **deep link**, and the fact that its anchor is the one verified on
//!     the target OS rather than the plan's candidate.
//!
//! **Why there is no tap in this file.** A real `AudioHardwareCreateProcessTap`
//! needs the TCC grant and a 14.4 Mac, and CI has neither. That is not a
//! weakening: the thing row 1 is about is a *decision made from observations*,
//! and the observations (`TapLiveness`, `TapEnvironment`) are plain data. A
//! denial is replayed here as what it actually looks like — an IOProc that
//! fires on cadence and delivers buffers of exact zeros, forever, while
//! something is audibly playing — through the same `fold_block` the drain path
//! uses, so the numbers under test are the numbers the app would compute. The
//! end-to-end wiring over a fake CoreAudio platform is
//! `tests/meeting_track_b_wiring.rs`; the real-Zoom demo is manual and lives in
//! `docs/MEETING-DEMO.md`.

use std::time::Duration;

use wilson_voice_lib::meeting::WATCHDOG_INTERVAL;
use wilson_voice_lib::meeting_matrix::{Coverage, ROWS};
use wilson_voice_lib::meetings::{SetupVerdict, SystemAudioSetup};
use wilson_voice_lib::os_version_gate::{system_audio_gate, OsVersion, SystemAudioGate};
use wilson_voice_lib::rtring::CaptureAnchor;
use wilson_voice_lib::syscapture::{
    fold_block, track_b_plan, GhostWatchdog, TapEnvironment, TapLiveness, TapVerdict,
    TapWatchdogAction, TrackBPlan, LOOKS_DENIED_MESSAGE, MAX_TAP_REBUILDS_PER_MEETING,
};

#[path = "support/callsite.rs"]
mod callsite;

const RATE: u32 = 48_000;
/// One 10 ms tap block at 48 kHz, the cadence an IOProc actually delivers at.
const BLOCK_FRAMES: usize = 480;

/// A tap that is firing perfectly and delivering nothing but exact zeros — the
/// only thing a denied tap ever produces, and (after minutes of good audio) the
/// only thing OS-4's ghost produces either.
fn silent_block() -> Vec<f32> {
    vec![0.0; BLOCK_FRAMES]
}

fn anchor(index: u64) -> CaptureAnchor {
    CaptureAnchor {
        host_ns: index * 10_000_000,
        sample_index: index * BLOCK_FRAMES as u64,
        frames: BLOCK_FRAMES as u32,
        sample_rate: RATE,
        lost_frames: 0,
    }
}

/// Run a meeting whose tap delivered **nothing but zeros from sample 0**, at
/// the shipping 60 s watchdog interval, until the ghost watchdog stops asking
/// for rebuilds. Returns every action it took.
///
/// `output_active` is the second bit the verdict needs:
/// `kAudioProcessPropertyIsRunningOutput` folded over the process list — was
/// anything on this Mac observably producing audio while the tap stayed quiet?
fn drive_never_delivered(output_active: Option<bool>, ticks: usize) -> Vec<TapWatchdogAction> {
    let mut live = TapLiveness::started();
    let mut last_nonzero = Duration::ZERO;
    let mut ghost = GhostWatchdog::new();
    let env = TapEnvironment {
        system_output_active: output_active,
        ..TapEnvironment::default()
    };
    let mut actions = Vec::new();

    for tick in 1..=ticks {
        let elapsed = WATCHDOG_INTERVAL * tick as u32;
        // Every 10 ms block of the last 60 s arrived, and every one was silent.
        let blocks = WATCHDOG_INTERVAL.as_millis() as u64 / 10;
        let base = (tick as u64 - 1) * blocks;
        for i in 0..blocks {
            fold_block(
                &mut live,
                &silent_block(),
                &[anchor(base + i)],
                elapsed,
                &mut last_nonzero,
            );
        }
        let action = ghost.tick(elapsed, live, env, live.frames_delivered);
        actions.push(action);
        // The caller of a rebuild reports back; a rebuild that is never closed
        // is a different failure (YV104's in-flight timeout) and not this row's.
        if action.is_rebuild() {
            ghost.finish_rebuild(
                wilson_voice_lib::syscapture::TapRebuildOutcome::Succeeded,
                elapsed,
            );
        }
    }
    actions
}

/// The row's own claim about what a denial looks like: never a non-zero sample,
/// and something was playing the whole time it stayed quiet.
#[test]
fn a_tap_that_never_delivered_while_audio_was_playing_reads_as_permission_denied() {
    let actions = drive_never_delivered(Some(true), 12);

    let degrade = actions
        .iter()
        .find(|a| a.is_degrade())
        .expect("a tap that never delivers must eventually degrade rather than retry forever");
    assert_eq!(
        degrade.verdict(),
        Some(TapVerdict::PermissionLikelyDenied),
        "never a non-zero sample, with output observably active during the silence, is the one \
         shape a TCC denial has — and the only shape this module is allowed to call one"
    );
    assert!(degrade.verdict().unwrap().blames_permission());
    assert!(
        degrade.verdict().unwrap().banner().contains("microphone"),
        "row 1's behaviour is mic-only recording that CONTINUES, so the banner has to say the mic \
         track is still running: {}",
        degrade.verdict().unwrap().banner()
    );
}

/// The failure mode that makes this row dangerous rather than merely missing.
///
/// The same all-zero tap, in a meeting where nothing was ever playing, is the
/// ordinary in-person meeting: a granted tap with nothing to record. Reading
/// that as a denial accuses the user's privacy settings of something that never
/// happened and sends them to a Settings pane that is already correct.
#[test]
fn a_granted_tap_with_nothing_playing_is_never_badged_denied() {
    // The probe answered, and it said nothing is producing output. There is
    // nothing wrong here at all: no rebuild, no badge, no verdict, for twelve
    // minutes of a perfectly ordinary in-person meeting.
    let quiet = drive_never_delivered(Some(false), 12);
    assert!(
        quiet.iter().all(|a| *a == TapWatchdogAction::Continue),
        "a quiet room must not cost a tap teardown or a badge: {quiet:#?}"
    );

    // The probe could not be read. The silence IS actionable — an unreadable
    // probe must not become evidence in either direction — so the rebuild
    // ladder runs, and the meeting still ends up with an honest verdict rather
    // than an accusation.
    let unknown = drive_never_delivered(None, 12);
    let degrade = unknown
        .iter()
        .find(|a| a.is_degrade())
        .expect("an unreadable probe leaves the silence unexplained, so it degrades");
    let verdict = degrade.verdict().expect("a degrade carries a verdict");
    assert_eq!(verdict, TapVerdict::NoSystemAudioObserved);
    assert!(
        !verdict.blames_permission(),
        "with nothing ever observed playing, the app has no evidence of a denial and must not \
         imply one: {verdict:?}"
    );
    assert!(
        !verdict.banner().contains("permission"),
        "the banner for {verdict:?} must not mention permission: {}",
        verdict.banner()
    );
}

/// Row 1 is a verdict at the FAR end of the ladder, never the first answer.
///
/// This is row 1 inheriting YV104's rule rather than restating it: the plan's
/// original §2.1 heuristic would have badged "denied" the moment silence passed
/// a threshold, and that is precisely the reading OS-4's ghost also produces.
/// Every action before the budget is spent must be a rebuild.
#[test]
fn no_permission_verdict_is_ever_reached_before_the_rebuild_budget_is_spent() {
    let actions = drive_never_delivered(Some(true), 12);
    let first_degrade = actions
        .iter()
        .position(|a| a.is_degrade())
        .expect("degrade never happened");
    let rebuilds_first = actions[..first_degrade]
        .iter()
        .filter(|a| a.is_rebuild())
        .count();

    assert_eq!(
        rebuilds_first, MAX_TAP_REBUILDS_PER_MEETING as usize,
        "the full 7-step rebuild is the first answer to silence and gets its whole budget before \
         anything is badged: {actions:#?}"
    );
    for action in &actions[..first_degrade] {
        assert_eq!(
            action.verdict(),
            None,
            "a permission verdict appeared before the budget was spent"
        );
    }
}

/// The tap is lost; the meeting is not. Row 1's "never abort", as a property of
/// the type rather than of a caller's care.
#[test]
fn nothing_this_row_can_produce_ends_the_meeting() {
    let actions = drive_never_delivered(Some(true), 12);
    for action in actions {
        match action {
            TapWatchdogAction::Continue
            | TapWatchdogAction::RebuildFull { .. }
            | TapWatchdogAction::RebuildSettled { .. }
            | TapWatchdogAction::DegradeTrackLost { .. } => {}
        }
    }
    // The match above is exhaustive on purpose: it is the compile-time half of
    // "there is no stop variant, and there never will be". If a future edit
    // adds one, this file stops compiling and its author has to come here.
}

/// **The row's behaviour, as the shipping app now performs it.**
///
/// A meeting asks exactly one question about system audio before it starts, and
/// this is it. A denial answers `MicOnly` with a sentence — never an error,
/// never a refusal, never silence — and that is the whole of row 1: the meeting
/// goes ahead with the microphone and says what it is missing.
#[test]
fn a_denied_tap_costs_the_second_track_and_nothing_else() {
    let denied = SystemAudioSetup::from_row(Some(SystemAudioSetup::encode(
        SetupVerdict::LooksDenied,
        "2026-08-15T00:00:00Z",
    )));
    let plan = track_b_plan(SystemAudioGate::Available, &denied);
    assert_eq!(
        plan,
        TrackBPlan::MicOnly {
            badge: LOOKS_DENIED_MESSAGE
        },
        "row 1 is a mic-only MEETING, not a refused one"
    );
    assert_eq!(plan.tracks(), 1, "one track, and the meeting still runs");
    let badge = plan.badge().expect("a mic-only meeting must say why");
    assert!(
        badge.contains("System Settings"),
        "row 1 ends with `one deep link to the right Settings pane`, so the sentence has to name \
         the recovery: {badge}"
    );
    assert!(
        !badge.contains("cannot record") && !badge.contains("unavailable."),
        "the meeting is NOT unavailable — it is recording: {badge}"
    );

    // And the same row on a Mac too old for the API says the OTHER sentence:
    // no setting on that machine can change it, so sending the user to Settings
    // would be a dead end (that half is row 12's, and the split is deliberate).
    assert_eq!(
        track_b_plan(system_audio_gate(OsVersion::new(13, 6, 0)), &denied),
        TrackBPlan::MicOnly {
            badge: wilson_voice_lib::os_version_gate::SYSTEM_AUDIO_REQUIREMENT
        }
    );
}

/// **The tripwire, inverted.** It used to assert `start_system_tap` had no
/// caller; the day it got one is the day this row became real, so it now
/// asserts the caller is still there.
///
/// Deleting the check when it fired would have been the failure mode
/// `matrix_coverage.rs` warns about in as many words. Keeping it pointed the
/// other way costs nothing and catches the regression that matters now: a
/// refactor that quietly drops the call would leave this row published as
/// `Test` describing a meeting that opens no tap.
#[test]
fn a_meeting_really_does_open_a_tap_now() {
    let callers = callsite::call_sites("start_system_tap", &["syscapture.rs"]);
    assert!(
        !callers.is_empty(),
        "nothing in `src/` calls `start_system_tap` any more. Row 1 is published as `Test`, which \
         claims a meeting opens a tap that a denial can be observed on — if the caller is really \
         gone, this row goes back to `Coverage::PolicyOnly` rather than staying green."
    );
}

/// The half of row 1 that DID ship with #125, asserted so the row's cell is not
/// read as "none of this exists".
///
/// Row 1's required behaviour ends with *"one deep link to the right Settings
/// pane"*, and OS-10 is explicit that a wrong anchor is worse than no link at
/// all: TCC never re-asks, so this link is the entire recovery, and landing a
/// denied user at the top of System Settings is a dead end. The anchor is
/// declared once, in `permissions::SYSTEM_AUDIO_PANE`, and resolves to the
/// enumerated-and-verified `Privacy_AudioCapture` — never the plan's candidate
/// `Privacy_SystemAudio`, which does not exist on the target OS.
#[test]
fn the_deep_link_half_of_the_row_ships_and_points_at_the_verified_anchor() {
    let permissions = std::fs::read_to_string(callsite::src_dir().join("permissions.rs"))
        .expect("read src/permissions.rs");
    // Comments do not count, in either direction — this module's docs quote the
    // candidate anchor precisely to record that it does not exist, and a scan
    // that read comments would call that evidence of shipping it. Same rule
    // `callsite.rs` states for call sites.
    let code: String = permissions
        .lines()
        .map(|line| {
            let t = line.trim_start();
            if t.starts_with("//") || t.starts_with('*') || t.starts_with("/*") {
                String::new()
            } else {
                format!("{line}\n")
            }
        })
        .collect();
    assert!(
        code.contains("Privacy_AudioCapture"),
        "the deep link must use the anchor verified on the target OS"
    );
    assert!(
        !code.contains("Privacy_SystemAudio"),
        "`Privacy_SystemAudio` is the plan's candidate anchor and it does not exist — an \
         unrecognised anchor opens the top of System Settings, which OS-10 calls a worse dead end \
         than shipping no link"
    );
    assert_eq!(
        wilson_voice_lib::permissions::SYSTEM_AUDIO_PANE,
        "SystemAudio",
        "the pane key the UI passes to `open_privacy_settings` is declared here and nowhere else"
    );
}

/// …and the published cell says what the app now does, naming the decision this
/// file drives.
#[test]
fn the_published_cell_names_the_decision_the_app_now_makes() {
    let row = ROWS.iter().find(|r| r.id == "1").expect("row 1");
    assert_eq!(
        row.coverage,
        Coverage::Test {
            test: "matrix_row1_tap_permission_denied.rs",
            subject: "track_b_plan",
            subject_module: "syscapture.rs",
        }
    );
    let cell = row.coverage.cell();
    assert!(!cell.contains("NOT WIRED"), "{cell}");
}
