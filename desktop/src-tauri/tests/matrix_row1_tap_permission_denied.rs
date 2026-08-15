//! Matrix row 1 — **system-audio (tap) permission denied at start**.
//!
//! Required behaviour (plan §6): the meeting still records, mic-only, badged
//! "system audio unavailable", never aborted, with one deep link to the right
//! Settings pane.
//!
//! Published as `PolicyOnly` with **no owning PR** — #125 (YV102) was named
//! here until it merged, and what merged was the pre-warm, not this row — and
//! this file is what makes that publication checkable from three ends:
//!
//!   * the **decision** — can this app tell a denied tap from a granted one
//!     that had nothing to record? — is driven here against the shipping
//!     discriminator YV104 merged, including the case where getting it wrong
//!     means accusing a user's privacy settings of something that never
//!     happened;
//!   * the **absence** — `syscapture::start_system_tap`, the thing that would
//!     open the tap a denial could be observed on, has no caller in `src/` — is
//!     asserted here too, so the row cannot keep saying "not wired" after
//!     somebody starts a tap inside a meeting;
//!   * the **half that did ship** — the deep link, and the fact that its anchor
//!     is the one verified on the target OS rather than the plan's candidate.
//!
//! **Why there is no tap in this file.** A real `AudioHardwareCreateProcessTap`
//! needs the TCC grant and a 14.4 Mac, and CI has neither. That is not a
//! weakening: the thing row 1 is about is a *decision made from observations*,
//! and the observations (`TapLiveness`, `TapEnvironment`) are plain data. A
//! denial is replayed here as what it actually looks like — an IOProc that
//! fires on cadence and delivers buffers of exact zeros, forever, while
//! something is audibly playing — through the same `fold_block` the drain path
//! uses, so the numbers under test are the numbers the app would compute.

use std::time::Duration;

use wilson_voice_lib::meeting::WATCHDOG_INTERVAL;
use wilson_voice_lib::meeting_matrix::{Coverage, ROWS};
use wilson_voice_lib::rtring::CaptureAnchor;
use wilson_voice_lib::syscapture::{
    fold_block, GhostWatchdog, TapEnvironment, TapLiveness, TapVerdict, TapWatchdogAction,
    MAX_TAP_REBUILDS_PER_MEETING,
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

/// **The absence half, re-aimed by YV102 at the thing that is actually
/// missing.**
///
/// This test used to assert `syscapture::prewarm_tap` was absent, and #125
/// shipped it: the Settings step calls it, so that assertion went red exactly
/// as designed. The instruction it printed — *promote the row* — is the wrong
/// move here, and writing down why is the point of this comment.
///
/// Row 1 is a denial **at start**. The pre-warm asks the same question minutes
/// earlier, from Settings, with no meeting running; it cannot badge a meeting
/// that has not begun, and a user who never opens that step is exactly the user
/// row 1 describes. The call that would put this row in effect is
/// `syscapture::start_system_tap` — a tap inside a real meeting — and it has no
/// caller anywhere in `src/`. Same missing wiring row 2 names, and no item in
/// the 22-B backlog owns it, which is why the cell below is unowned.
///
/// The tripwire is therefore not deleted, it is pointed at the truth: the day
/// something opens a tap in a meeting, this goes red and row 1 becomes a `Test`
/// row with a real surface to drive.
#[test]
fn nothing_opens_a_tap_in_a_meeting_so_row_1_is_still_not_wired() {
    // `syscapture.rs` DEFINES `start_system_tap`; a definition is not a call
    // site (the rule `callsite.rs` and `matrix_coverage.rs` both state).
    let found = callsite::call_sites("start_system_tap", &["syscapture.rs"]);
    assert!(
        found.is_empty(),
        "{}",
        callsite::promote_the_row("1", "start_system_tap", &found)
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

/// …and the published cell says so: no owner, and the call site it is waiting
/// for is the one this file just proved absent.
#[test]
fn the_published_cell_admits_that_nobody_owns_the_missing_wiring() {
    let row = ROWS.iter().find(|r| r.id == "1").expect("row 1");
    assert_eq!(
        row.coverage,
        Coverage::PolicyOnly {
            test: "matrix_row1_tap_permission_denied.rs",
            wiring_pr: None,
            absent_call_site: "start_system_tap",
        }
    );
    let cell = row.coverage.cell();
    assert!(cell.contains("NOT WIRED"), "{cell}");
    assert!(cell.contains("start_system_tap"), "{cell}");
    assert!(
        !cell.contains("#125"),
        "#125 merged; a cell still naming it as the pending owner reads as progress and \
         describes none: {cell}"
    );
}
