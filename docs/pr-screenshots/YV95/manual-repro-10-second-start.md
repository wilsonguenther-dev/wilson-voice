# YV95 — "a user who has never read a changelog can start and stop a meeting within 10 seconds"

The plan's own acceptance line for this item (finding #6 / 22d). It is a UX
claim about a person, so it is a manual repro by construction. This file records
what was automated, what was NOT run, and the exact procedure — so the claim is
never stronger than the evidence.

## What was NOT run

**The timed, end-to-end repro on a running app was not performed for this
branch, and no "n seconds" figure is claimed in the PR.** Starting a meeting for
real needs a capture engine, and the capture engine is YV91 (PR #108 —
`meeting::MeetingSession`), which is not on `main` yet. On this branch alone the
entry points are deliberately DISABLED with the reason in the label
(`meeting_control::NO_ENGINE_MESSAGE`), because a Record button that silently
does nothing is finding #6 all over again in nicer clothes.

Once #108 has landed, the wiring is a single line at the marked spot in
`lib.rs::setup` (`install_capture_engine(...)`), and the repro below becomes
runnable. It belongs with YV99's phase-closing on-camera demo, which records a
full 10-minute meeting anyway.

## The gate that makes that promise binding

An unrun acceptance criterion plus a comment saying "someone will wire this
later" is how a phase merge gate stops gating. Finding #6 is explicitly a merge
gate, and four disabled controls do not close it — merging as-is would otherwise
leave `main` with a permanently dead Record button and nothing forcing YV91 to
add the one line.

So the promise is a test, not a note:

    cargo test --test capture_engine_is_installed

`the_record_button_cannot_stay_dead_once_the_capture_module_lands` reads `src/`
and fails the build the first time `src/meeting.rs` (YV91's `MeetingSession`)
exists without a non-comment `meeting_control::install_capture_engine(` call in
`lib.rs`. Whichever of #108 / #112 merges second cannot go green without doing
the wiring, and its failure message names this file as the next step. Falsified
before shipping: with a stub `src/meeting.rs` in the tree the test fails with
that message; with the file absent it passes.

The companion test `with_no_engine_installed_every_entry_point_says_why` pins
the other half — while nothing is installed, `capture_available()` is false,
`MeetingStatus.unavailableReason` is `NO_ENGINE_MESSAGE` verbatim, and
`start()` refuses rather than opening a row. That is what makes the disabled
state honest rather than a false capability claim.

## What WAS verified, mechanically

`cargo test --test meeting_manual_start_stop` (10 tests) drives the same control
plane every entry point calls, against a synthetic capture engine:

* one toggle opens a `recording` row with the preflight diagnostics already on
  it; a second toggle closes it out with a duration, a WAV path that exists on
  disk, and `state='transcribing'`;
* the elapsed clock ticks once per interval, and the label the pill renders is
  byte-identical to the one the main window renders;
* a second press cannot open a second meeting; stopping nothing is an error, not
  a phantom row; a refused start leaves NO row behind; a capture that lands no
  audio is `partial` with the reason attached; quitting finalizes a live meeting.

## The procedure, for whoever runs it

Prerequisite: a build with a capture engine installed, Microphone granted, and a
tester who has not been shown the feature.

1. Start a stopwatch and hand over the Mac with only this instruction:
   *"Record this meeting."*
2. Time to first bytes captured, by any of the four routes — the tester should
   need exactly one of them:
   * menu-bar icon → **Record a meeting**
   * **⌃⌘M** from anywhere
   * Yap → **Meetings** tab → the big **Record this meeting** button
   * (while recording) the pill's ■ control, or the banner's **Stop meeting**
3. Confirm the pill shows a red dot and a running `hh:mm:ss`, and the menu-bar
   item now reads **Stop meeting**.
4. Ask them to stop it. Stop the clock when the row appears in the Meetings list.
5. Record both numbers below. The bar is **≤ 10 s for start**, and the row must
   be in the list without a manual refresh.

| Date | Tester (no prior exposure?) | Route taken | Start (s) | Stop → row visible (s) |
|---|---|---|---|---|
| _blocked on #108 — enforced by `cargo test --test capture_engine_is_installed`_ | | | | |

This row stays empty on purpose. It is not "we forgot": there is nothing on this
branch that can be timed, because there is nothing behind the button, and a
fabricated number here would be worse than a blank. The gate above is what makes
the blank temporary.

## Why four routes and not one

Finding #6 lists all four as the item's content, and they exist for different
moments: the tray is discoverable, the hotkey is fast, the pill is where your
eyes already are once recording, and the empty state is the only thing on screen
the first time you ever open the tab. All four call the SAME backend toggle
(`toggle_meeting_recording` → `MeetingController::toggle`), so there is one
behaviour to learn and one to test.
