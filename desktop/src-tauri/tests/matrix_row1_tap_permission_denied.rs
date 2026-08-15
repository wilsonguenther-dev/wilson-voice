//! Matrix row 1 — **system-audio (tap) permission denied at start**.
//!
//! Required behaviour (plan §6): the meeting still records, mic-only, badged
//! "system audio unavailable", never aborted, with one deep link to the right
//! Settings pane.
//!
//! Published as `PolicyOnly` with **#125 (YV102)** named as the owner of its
//! wiring, and this file is what makes that publication checkable from both
//! ends:
//!
//!   * the **decision** — can this app tell a denied tap from a granted one
//!     that had nothing to record? — is driven here against the shipping
//!     discriminator YV104 merged, including the case where getting it wrong
//!     means accusing a user's privacy settings of something that never
//!     happened;
//!   * the **absence** — `syscapture::prewarm_tap`, the thing that would ask
//!     the question at start, does not exist in `src/` — is asserted here too,
//!     so the row cannot keep saying "not wired" after YV102 merges.
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

/// **The absence half.** Row 1's wiring is `syscapture::prewarm_tap`, which
/// lands with #125 (YV102) and is in no file in `src/` today — which is why the
/// app cannot ask this question at start, and why the row is not `Test`.
#[test]
fn prewarm_tap_is_still_absent_so_row_1_is_not_wired() {
    let found = callsite::call_sites("prewarm_tap", &[]);
    assert!(
        found.is_empty(),
        "{}",
        callsite::promote_the_row("1", "prewarm_tap", &found)
    );
}

/// …and the published cell says so, naming the PR that owns it, so a reader of
/// the rendered table gets the same answer this test does.
#[test]
fn the_published_cell_names_the_owner_of_the_missing_wiring() {
    let row = ROWS.iter().find(|r| r.id == "1").expect("row 1");
    assert_eq!(
        row.coverage,
        Coverage::PolicyOnly {
            test: "matrix_row1_tap_permission_denied.rs",
            wiring_pr: Some("#125 (YV102)"),
            absent_call_site: "prewarm_tap",
        }
    );
    let cell = row.coverage.cell();
    assert!(cell.contains("Policy only"), "{cell}");
    assert!(cell.contains("#125 (YV102)"), "{cell}");
    assert!(cell.contains("prewarm_tap"), "{cell}");
}
