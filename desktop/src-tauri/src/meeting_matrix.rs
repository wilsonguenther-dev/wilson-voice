//! YV99 — the 22-A error-handling matrix, as data and as policy.
//!
//! The plan's §6 matrix is seventeen rows of prose. Prose does not fail a build.
//! This module turns the six rows 22-A actually owns (4, 5, 6, 15, 16, 17) plus
//! the two rows finding #3 added (`3a` user quits mid-processing, `3b` an ASR
//! chunk exceeds `TRANSCRIBE_TIMEOUT`) into two things a merge queue can read:
//!
//!   1. **[`ROWS`] — the matrix itself, as a const table**, each row carrying
//!      the name of the test that covers it *and the shipping symbol that test
//!      has to drive*. `tests/matrix_coverage.rs` walks that table against the
//!      filesystem, so "every row has a passing named test or a documented
//!      manual repro" stops being a claim in a PR body and becomes something CI
//!      checks. `docs/yap22a-error-matrix.md` is rendered from the same table
//!      ([`render_markdown`]) and asserted against it, so the document cannot
//!      drift away from the code.
//!   2. **The pure policy for the rows whose *decision* nothing else owns** —
//!      the honest quality note a meeting owes the user when the disk cost it
//!      audio (row `5b`), row 16's sleep/wake state machine, and row 17's
//!      continuation-title rule (row `17b`).
//!
//! A row is a *cell*, not a failure: where one failure's two halves have
//! different truth values, it gets two cells. Row 5 is the case — the bounded
//! queue that keeps the disk off the audio callback is `meeting::MeetingJournal`
//! and is shipping, while the quality note that would tell the user what the
//! drops cost has no caller anywhere. One cell covering both would have to pick
//! a truth value, and either choice is a lie about the other half. So the
//! failure is published as `5` (shipping) and `5b` (policy only). Row 17 is the
//! same shape now that YV91 has merged: the cap runs (`17`), the continuation
//! meeting it should hand off to does not (`17b`).
//!
//! **A `Test` cell names the code it drives, and that is not decoration.** The
//! review that produced this rule found row 5 published as a plain green
//! `cargo test` cell whose test read `record.rs` — the *dictation* journal —
//! while the row's subject is the meeting journal a three-hour recording lives
//! or dies by. Both are bounded queues with a drop counter, so the test passed,
//! looked exactly right, and demonstrated nothing about the row. So
//! [`Coverage::Test`] carries `subject`/`subject_module`, and
//! `matrix_coverage::test_rows_drive_the_shipping_code_they_name` fails unless
//! that symbol is real code in that module AND appears as code in the row's
//! test. A `Test` cell can no longer be aimed at the wrong journal in silence.
//!
//! Everything *here* is pure: no clock, no disk, no CoreAudio, no Tauri handle.
//! That is the point for the rows this module owns. Sleep/wake and a
//! continuation title are miserable to reproduce by hand and trivial to
//! reproduce as a sequence of events fed to a state machine — the same shape
//! `sysaudio::restore_plan` and `input_format`'s handler already use in this
//! codebase. The capture session (YV91) supplies the events and performs the
//! actions; this module decides what the action *is*.
//!
//! **What this module deliberately does NOT do.** It does not open a journal,
//! take a power assertion, register `NSWorkspaceWillSleepNotification`, stop a
//! recording, or start the meeting that carries on past the cap. Those call
//! sites live in the capture session and its UI. A policy with no caller is a
//! policy that is not in effect, which is why [`ROWS`] never marks such a row
//! [`Coverage::Test`]: a row whose *decision* is tested here while its *call
//! site* does not exist anywhere is [`Coverage::PolicyOnly`]. Both variants are
//! tripwired in `tests/matrix_coverage.rs`, in opposite directions — a `Test`
//! row's subject must be present, a `PolicyOnly` row's call site must stay
//! absent — so neither can quietly rot into an implied "the app already does
//! this".
//!
//! ## YV105 — the five tap-scoped rows (1, 2, 3, 12, 14)
//!
//! This table is no longer 22-A's alone. YV105 wires the five rows of the
//! plan's §6 that are **tap-scoped** into the same const, because the
//! alternative — a second table for 22-B — is the parallel-mechanism mistake
//! this module was built to avoid. The rows the plan lists that are still NOT
//! here are still not here for stated reasons (see [`ROWS`]).
//!
//! **Four of the five are `PolicyOnly`, and that is the finding of this item
//! rather than a shortfall in it.** YV101 (the 14.4 gate), YV103 (the
//! output-device state machine) and YV104 (the ghost watchdog) have merged, so
//! their *decisions* ship. The thing that produces the events those decisions
//! consume — the CoreAudio tap itself — has not: YV100 and YV102 are open PRs,
//! `CaptureEnv::tap_liveness` returns `None` for every environment in the tree
//! (`meeting.rs` says so in as many words), and nothing calls
//! `InputFormatWatch::watch_output`, so the output half of that watch is inert.
//! A `Test` cell on row 2 would tell a reader "if the system-audio track dies
//! mid-meeting, Yap keeps your mic track and banners the pill". Today Yap has
//! no system-audio track to lose. The mechanism being merged is not the same
//! claim as the app performing the behaviour, and this table's whole reason for
//! existing is that the second claim is the one people read.
//!
//! Row 12 is the exception and it splits, exactly as rows 5 and 17 did: the
//! gate itself runs (`meeting_asr::meeting_availability_for` is consulted by
//! the shipping `notetaker_status` command, and it is what keeps mic-only
//! recording working down to the macOS 12 floor), while the sentence it
//! produces reaches no surface, because no frontend calls that command yet. Two
//! truth values, two cells.
//!
//! ## YV102 (#125) — what merging the pre-warm actually moved
//!
//! Both of the tripwires YV105 armed for this PR fired, and they resolved in
//! opposite directions. That is the point of arming them per-row rather than
//! promoting a whole phase at once.
//!
//!   * **Row `12b` becomes `Test`.** `App.tsx` now invokes `notetaker_status`
//!     and renders `systemAudioMessage` under a disabled "Set up meeting
//!     recording" step, which is the plan's own wording for row 12 — visible,
//!     disabled, carrying the reason — so the split closes.
//!   * **Row 1 stays `PolicyOnly` and loses its owner.** `prewarm_tap` shipped
//!     and is called, so the old `absent_call_site` is gone; the row's
//!     behaviour did not ship with it. A denial "at start" needs a tap at
//!     start, and `start_system_tap` still has no caller — the same missing
//!     wiring row 2 names, owned by no item in the 22-B backlog. Promoting the
//!     row because its tripwire fired would have published "your meeting keeps
//!     recording when the tap is denied" about a meeting that opens no tap.
//!
//! ## YV110 — the two rows that were waiting for a caller
//!
//! Rows 1 and 2 are `Test` rows now, and the thing that changed is the one
//! thing both cells had been naming for four merges: **a meeting opens a tap.**
//! `meeting_control::SessionEngine::start` asks `syscapture::track_b_plan`
//! whether this Mac and this install attach Track B, calls `start_system_tap`
//! when the answer is yes, and hands the honest sentence to the 1 Hz status
//! emit when the answer is no. `syscapture::TappedEnv` is the first
//! `CaptureEnv` in the tree to answer `Some` to `tap_liveness`, so YV104's
//! ghost ladder runs inside a real session rather than only inside its own
//! tests.
//!
//! Both promotions were *forced* rather than chosen: `matrix_coverage`'s
//! unowned-policy tripwire and each row's own absence test went red on the
//! commit that added the caller, which is exactly what those assertions exist
//! to do. The rows' tests were rewritten to drive the shipping surface, and the
//! unowned inventory dropped from six rows to four.
//!
//! One thing the promoted row 2 deliberately does not claim: the seven
//! CoreAudio calls of a mid-meeting rebuild are decided and logged, not
//! executed — see that row's cell for why, and `syscapture`'s YV110 section for
//! the clock-continuity reason it is a separate item.
//!
//! ## YV132 — the three yap23 rows (7, 8, 13)
//!
//! The `ROWS` doc comment reserved rows 7, 8 and 13 for "yap23, which does not
//! exist yet to fail". It exists now: YV121 shipped `diarize::DiarizePool` (the
//! readiness handshake, the deadline, the one-restart budget, the idle sweep),
//! YV123 shipped the two vendored models and `models::is_diarize_downloaded` /
//! `is_diarize_ready` over them. So the reservation is spent and the three rows
//! are published.
//!
//! **All three are `PolicyOnly`, and that is this item's finding rather than a
//! shortfall in it.** Nothing in the shipping app runs a diarization pass:
//! `diarize::pool()` has no caller, no meeting is ever handed to a sidecar, and
//! no surface anywhere offers speaker detection to turn on or off. A `Test`
//! cell on row 7 would tell a reader "if the diarizer wedges, your meeting
//! still completes with a plain transcript" — about a meeting that never asks a
//! diarizer anything. That is the false-capability claim rows 1 and 2 were held
//! back from for four merges, and the same rule applies to a phase this
//! module's own table is the first to touch.
//!
//! What each row therefore owes the reader is the *decision*, written once and
//! tested, plus a tripwire that fires the day it acquires a caller:
//!
//!   * **Row 7** — [`diarize_sidecar_degrade`]. Every way the sidecar can fail
//!     to answer maps to the same outcome: a plain single-speaker transcript on
//!     a **complete** meeting, marked [`DIARIZATION_FAILED`]. The type is what
//!     enforces the plan's governing principle here — [`SpeakerLabels`] has no
//!     variant that fails or discards a meeting, so "the user still gets their
//!     notes" cannot be lost to a branch somebody forgot to take.
//!   * **Row 8** — [`cluster_sanity`]. A pass that RAN and returned noise
//!     degrades to the same plain transcript, carrying a different reason, so
//!     "the sidecar crashed" and "the sidecar answered with rubbish" stay
//!     diagnosable apart in a log and in a diagnostics row. **This row's
//!     degrade is a requirement on YV126's `diarize::rank_and_floor` (#141,
//!     open), and that requirement is currently unmet** — see the row's cell
//!     and [`cluster_sanity`]'s own docs for the divergence and for the two
//!     assertions that make it impossible to leave unreconciled.
//!   * **Row 13** — [`speaker_detection_gate`]. Models missing or half
//!     downloaded turns the affordance **off, not broken**, with the combined
//!     download size in the sentence — and it cannot touch recording, because
//!     the gate's own type has nothing to say about recording at all.
//!
//! **Not one accuracy number is declared here, and that is load-bearing.**
//! [`cluster_sanity`] takes its floor as an argument; this module holds no
//! opinion about how many seconds or turns a cluster needs. Those numbers are
//! YV126's to MEASURE against `diarize_metrics` on the eval fixtures, and a
//! plausible-looking `const` here would be exactly the vendor-blog threshold
//! this phase was sequenced eval-first to avoid — with the extra twist that the
//! published matrix row would then be where it entered the codebase.
//!
//! **And it does not re-declare a threshold that ships somewhere else.** The
//! 3 h cap and its 2 h 45 m warning are `meeting::MEETING_HARD_CAP` /
//! `meeting::MEETING_CAP_WARN_AT`, enforced by `meeting::watchdog_tick` — the
//! code that actually runs, and what row 17's test now drives directly. A second
//! copy here would be two numbers with no compile-time link between them: change
//! the shipping one and this module's tests stay green while the published
//! matrix row goes quietly wrong. So this module keeps only the half the capture
//! session does *not* build — [`continuation_title`] — and both
//! `tests/matrix_row17_meeting_cap.rs` and `tests/matrix_coverage.rs` fail the
//! build if a cap threshold is ever re-declared here.

// The only two imports this module has, both added by YV132 and both types
// rather than behaviour: the error the sidecar pool already reports
// (`diarize::DiarizeError`, so row 7's mapping is exhaustive over the real set
// and a new variant is a compile error here) and the state a meeting already
// has (`meetings::MeetingState`, so row 7's "complete, never failed" is stated
// in the shipping enum instead of in a second copy of its strings). Everything
// below is still pure: no clock, no disk, no CoreAudio, no Tauri handle.
use crate::diarize::DiarizeError;
use crate::meetings::MeetingState;

// ── The matrix, as data ─────────────────────────────────────────────────────

/// How one matrix row is covered. There are exactly three honest answers — the
/// app does it and a test proves it, a human has to do it by hand, or the
/// decision exists and nothing calls it — and "we thought about it" is not one
/// of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    /// End to end: a named integration test that exists in
    /// `desktop/src-tauri/tests/` on this branch, runs in CI, and drives code
    /// that is **wired into the shipping app**.
    ///
    /// This is the strongest cell in the table and it is the one a reader
    /// interprets as "done", so it is deliberately the hardest to claim: the
    /// cell must name not only the test but the shipping symbol that test has
    /// to touch, and `tests/matrix_coverage.rs` checks both ends of that.
    Test {
        /// The test file, in `desktop/src-tauri/tests/`.
        test: &'static str,
        /// The shipping symbol the row is *about* — the thing that has to be
        /// exercised for the cell to mean anything. Row 5's is
        /// `MeetingJournal`, and naming it is what stops the row being
        /// satisfied by a green test aimed at the dictation journal next door.
        subject: &'static str,
        /// The `src/` file `subject` lives in. Two modules in this tree have a
        /// bounded journal queue with a drop counter; a row that names only the
        /// symbol could still be pointed at the wrong one.
        subject_module: &'static str,
    },
    /// No pure test is possible (it needs two processes, or a lid). The string
    /// is a repo-relative path to the written repro, which a human follows and
    /// records the result of in the PR.
    Manual(&'static str),
    /// The *decision* is implemented and tested here as pure policy; the *call
    /// site that would put it in effect does not exist in the shipping tree*.
    ///
    /// This variant exists because the alternative was a lie. Row 16's
    /// sleep/wake state machine is fully tested — and nothing registers
    /// `NSWorkspaceWillSleepNotification`, so closing a lid mid-meeting still
    /// does whatever it did before. Publishing that row as [`Coverage::Test`]
    /// tells a reader the app finalizes on sleep. It does not. Rows `5b` and
    /// `17b` are the same shape with smaller blast radii: [`quality_note`]
    /// computes the sentence a meeting owes the user when the disk cost it
    /// audio, [`continuation_title`] names the meeting that carries on past the
    /// 3 h cap, and no surface anywhere calls either.
    ///
    /// This is tripwired in the opposite direction to [`Test`](Coverage::Test):
    /// `tests/matrix_coverage.rs` fails the moment `absent_call_site` shows up
    /// as code anywhere in `src/` outside this module, because that is the day
    /// the row became a real `Test` row and must be flipped. That tripwire runs
    /// over the rows with **no owning PR**, which are the ones that can rot
    /// silently; a row whose `wiring_pr` names an open PR is checked by its own
    /// test instead (YV105's five rows each assert their named call site is
    /// still absent), so the claim is checked either way.
    ///
    /// **The pure decision does not have to live in this module.** It did for
    /// all three of 22-A's policy rows, which is why the wording above says
    /// "here" — but YV105's tap rows are decided in the module that owns the
    /// mechanism (`syscapture::ghost_tick`, `input_format::InputFormatWatch`),
    /// and that is the better place for them: a second copy here would be the
    /// same duplicate-threshold trap row 17 was rejected for. What makes those
    /// rows `PolicyOnly` is unchanged and is the only thing this variant has
    /// ever meant — **nothing in the shipping app can reach the decision yet**.
    PolicyOnly {
        /// The test file that drives the pure policy on this branch.
        test: &'static str,
        /// The PR known to be bringing the call site — or `None` when **no open
        /// PR owns it at all**, which is the truth for all three of today's
        /// policy rows and is worth saying out loud instead of leaving a reader
        /// to assume someone has it.
        wiring_pr: Option<&'static str>,
        /// The symbol whose appearance in `src/` means the wiring landed. Kept
        /// as a string rather than a comment precisely so a test can check it.
        absent_call_site: &'static str,
    },
}

/// One row of the 22-A error matrix.
#[derive(Debug, Clone, Copy)]
pub struct MatrixRow {
    /// The row's number in the plan's §6 table (`3a`/`3b` are finding #3's two
    /// additions, which the plan's table never numbered).
    pub id: &'static str,
    /// What goes wrong.
    pub failure: &'static str,
    /// What Yap must do about it.
    pub behavior: &'static str,
    pub coverage: Coverage,
}

/// The `matrix_` prefix every row's test file must carry.
///
/// The phase's acceptance criterion is a sweep over the matrix tests, so a row
/// whose test is named something else is a row that sweep can never reach —
/// which is how "all eight rows green in one run" became unsatisfiable by
/// construction while every individual test was green. Enforced by
/// `matrix_coverage::every_row_names_a_test_the_acceptance_sweep_can_reach`.
pub const REQUIRED_TEST_PREFIX: &str = "matrix_";

/// The sixteen failures of the plan's §6 matrix that 22-A, 22-B and yap23 own,
/// as nineteen cells (rows 5, 12 and 17 each have two halves with different
/// truth values — see the module docs).
///
/// **Rows 7, 8 and 13 joined this table with YV132**, and what earned them the
/// entry is that their subject now exists to fail: YV121 shipped the
/// `yap-diarize` sidecar and its pool, YV123 shipped the two vendored models
/// and the `is_diarize_downloaded` gate over them. All three are
/// [`Coverage::PolicyOnly`] and that is the finding of the item rather than a
/// shortfall in it — see the module docs' YV132 section.
///
/// **Rows 9, 10 and 11 are still NOT here, and each has a different reason —
/// stated so the boundary is a decision rather than an omission:**
///
///   * **9** (an ASR chunk fails) — already general-purpose in the shipped
///     chunker and published as row `3b`, which is the same failure stated with
///     the timeout that causes it. It applies identically to a tap track, so
///     there is nothing tap-specific to add and a second cell would be the same
///     mechanism counted twice.
///   * **10** (summarizer OOM/deadline/invalid JSON) — YV97's own degrade
///     ladder owns it, and that ladder is track-agnostic.
///   * **11** (calendar access revoked) — calendar is yap24 and does not exist.
///
/// Listing any of them here with no mechanism behind it would make the matrix
/// look more covered than it is, which is the one thing this table must never
/// do.
pub const ROWS: &[MatrixRow] = &[
    MatrixRow {
        id: "4",
        failure: "Disk fills during a 2 h recording",
        behavior: "Pre-flight refuses with a clear number; the 60 s watchdog stops cleanly below 1 GB free, finalizes the journal, marks `state='partial'`",
        coverage: Coverage::Test {
            test: "matrix_row4_disk_preflight.rs",
            subject: "required_free_bytes",
            subject_module: "meeting.rs",
        },
    },
    MatrixRow {
        id: "5",
        failure: "Journal write falls behind",
        behavior: "Bounded queue, `try_send`, drops counted, the audio callback never blocks",
        // The subject is the MEETING journal, and saying so in the table is the
        // whole point of the field: an earlier revision published this row
        // behind a test that read `record.rs`, which is the dictation take's
        // journal. Same shape, same counter name, different code — and a
        // three-hour recording is the case where the queue actually matters,
        // because YV91 stops retaining the session in RAM, so for a meeting the
        // journal IS the recording.
        coverage: Coverage::Test {
            test: "matrix_row5_journal_backpressure.rs",
            subject: "MeetingJournal",
            subject_module: "meeting.rs",
        },
    },
    MatrixRow {
        id: "5b",
        failure: "Journal write falls behind — and the meeting never says what it cost",
        behavior: "`dropped > 0` becomes an honest quality note on the meeting detail: how many seconds of audio the disk cost, and that they are missing",
        // NOT `Test`. `quality_note` computes that sentence and has zero callers
        // — no meeting detail renders it, and this branch changes no UI. The
        // never-block half above IS wired (`MeetingJournal::append`), which is
        // why the failure is published as two cells rather than one averaged
        // claim.
        coverage: Coverage::PolicyOnly {
            test: "matrix_row5_journal_backpressure.rs",
            wiring_pr: None,
            absent_call_site: "quality_note",
        },
    },
    MatrixRow {
        id: "6",
        failure: "App killed or crashes mid-meeting",
        behavior: "An orphan `.in_progress.json` + `.spill.pcm` found at startup finalizes into a wav; the meeting row becomes `partial` with a Resume-processing affordance",
        coverage: Coverage::Test {
            test: "matrix_row6_orphan_recovery.rs",
            subject: "recover_orphaned_meetings",
            subject_module: "meeting.rs",
        },
    },
    MatrixRow {
        id: "15",
        failure: "A second Yap instance is launched during a meeting",
        behavior: "`tauri-plugin-single-instance`, registered as the FIRST plugin, hands the argv to the running app and exits the duplicate — no second recorder, no second SQLite writer",
        // The backlog specifies this row's coverage as a manual repro, and it is
        // right: the failure is two live processes, and no in-process test can
        // launch one. `matrix_row15_single_instance.rs` exists and is green, but
        // all three of its assertions read `lib.rs` and `Cargo.toml` as TEXT —
        // that is a guard against the plugin being reordered out of first
        // position, not a demonstration that a second launch is survivable.
        // Publishing a source grep in the table's strongest cell is the same
        // defect as publishing an uncalled policy in it.
        coverage: Coverage::Manual("docs/yap22a-phase-demo.md"),
    },
    MatrixRow {
        id: "16",
        failure: "Sleep or lid close mid-meeting",
        behavior: "Finalize the journal, mark `paused_by_sleep`; on wake OFFER resume as a new segment rather than pretending the gap did not happen",
        // NOT `Test`. The state machine below is complete and tested; the
        // observer that would feed it is in no branch — `power.rs` mentions
        // `NSWorkspaceWillSleepNotification` in a comment and registers
        // nothing. Until someone writes that call site, a lid close mid-meeting
        // still does whatever it did before, and this row must not read as
        // though it does not.
        coverage: Coverage::PolicyOnly {
            test: "matrix_row16_sleep_wake.rs",
            wiring_pr: None,
            absent_call_site: "NSWorkspaceWillSleepNotification",
        },
    },
    MatrixRow {
        id: "17",
        failure: "Meeting exceeds the 3 h cap",
        behavior: "Warn once at 2 h 45 m; hard-stop once at 3 h, cleanly finalized",
        // The cap ships: `meeting::watchdog_tick` reads
        // `meeting::MEETING_HARD_CAP` / `MEETING_CAP_WARN_AT` and the 60 s
        // watchdog acts on it. This module re-declares neither threshold — the
        // row's test imports them, which is what makes the published behaviour
        // and the running behaviour the same numbers.
        coverage: Coverage::Test {
            test: "matrix_row17_meeting_cap.rs",
            subject: "watchdog_tick",
            subject_module: "meeting.rs",
        },
    },
    MatrixRow {
        id: "17b",
        failure: "…and the recording that was still going has nowhere to carry on",
        behavior: "A continuation meeting is created and linked to the one the cap stopped, titled so a 9 h session reads as three meetings rather than three copies of one",
        // NOT `Test`, for the same reason as `5b`: the naming rule is decided
        // and tested here, and `continuation_title` has zero callers. The
        // watchdog stops the meeting and stops there — nothing starts the next
        // one — so the cap half of row 17 is honest as `Test` only because the
        // half it does NOT do got its own cell.
        coverage: Coverage::PolicyOnly {
            test: "matrix_row17_meeting_cap.rs",
            wiring_pr: None,
            absent_call_site: "continuation_title",
        },
    },
    MatrixRow {
        id: "3a",
        failure: "User quits mid-processing (finding #3)",
        behavior: "Resume from `processed_through_seconds`, never re-decode from zero",
        // YV93 shipped this as `meeting_transcription_resume.rs`, a name the
        // phase's acceptance sweep could never reach; the rename to the
        // backlog's name is done here, on the branch that publishes the sweep.
        coverage: Coverage::Test {
            test: "matrix_new_quit_mid_processing.rs",
            subject: "JsonProgressStore",
            subject_module: "meeting_asr.rs",
        },
    },
    MatrixRow {
        id: "3b",
        failure: "An ASR chunk exceeds `TRANSCRIBE_TIMEOUT` (finding #3)",
        behavior: "That chunk gets `text=''` + `asr_failed`; its neighbours and the rest of the meeting are unaffected",
        coverage: Coverage::Test {
            test: "matrix_new_asr_chunk_timeout.rs",
            subject: "TranscriptionManager",
            subject_module: "transcription.rs",
        },
    },
    // ── 22-B · the tap-scoped rows (YV105) ──────────────────────────────────
    //
    // Appended rather than interleaved, for the same reason `3a`/`3b` were: the
    // rendered document is read top-to-bottom and a reader who knows the 22-A
    // table should see what was added, not have to diff it.
    MatrixRow {
        id: "1",
        failure: "System-audio (tap) permission denied at start",
        behavior: "The meeting still records, mic-only, badged `system audio unavailable`, with one deep link to the right Settings pane — never aborted",
        // `Test` at last, and YV110 is what earned it: a meeting now MAKES this
        // decision at T-0. `syscapture::track_b_plan` reads YV101's runtime OS
        // gate and YV102's setup row and answers `Attach` or `MicOnly { badge }`;
        // `meeting_control::SessionEngine::start` calls it, opens the tap only on
        // `Attach`, and hands the badge to the 1 Hz status emit the pill and the
        // main window already render. A tap that is denied *at start* now has a
        // meeting to be denied in, which is the one thing every earlier revision
        // of this cell was missing.
        //
        // The subject is deliberately the DECISION and not the tap: what row 1
        // requires is that the meeting proceeds mic-only and says why, and the
        // failure mode this cell has to keep out is a meeting that refuses to
        // start (or records silently) because system audio was unavailable.
        coverage: Coverage::Test {
            test: "matrix_row1_tap_permission_denied.rs",
            subject: "track_b_plan",
            subject_module: "syscapture.rs",
        },
    },
    MatrixRow {
        id: "2",
        failure: "Tap permission revoked, or the tap dies mid-meeting",
        behavior: "A `track_lost` marker into the meeting, Track A keeps recording, a banner in the pill — the meeting is NEVER stopped",
        // `Test`, and the subject is the thing whose ABSENCE kept this row
        // policy-only through four merges: a shipping `CaptureEnv` that answers
        // `Some` to `tap_liveness`. YV110's `syscapture::TappedEnv` is it. The
        // decision half never needed help — `meeting::watchdog_tick` has
        // consulted `syscapture::ghost_tick` since YV104, and
        // `TapWatchdogAction` has no stop variant, so "degrade, never end the
        // meeting" is enforced by the type. What was missing was a producer:
        // `WatchdogInputs::tap` was `None` on every real tick, so the branch was
        // unreachable in the app. A tap that cannot exist cannot die.
        //
        // What this cell still does NOT claim, and YV110 says so in its own
        // module docs: the seven CoreAudio calls of a mid-meeting rebuild are
        // not executed. The watchdog issues the attempts, logs them, and
        // degrades on schedule; a rebuilt tap rebases its `host_ns` epoch and
        // YV107's merge reads that field, so executing one is a real item with a
        // clock-continuity question in it. The behaviour this row publishes —
        // marker, mic keeps recording, banner, never stopped — is what ships.
        coverage: Coverage::Test {
            test: "matrix_row2_tap_revoked_mid_meeting.rs",
            subject: "TappedEnv",
            subject_module: "syscapture.rs",
        },
    },
    MatrixRow {
        id: "3",
        failure: "Mic permission denied",
        behavior: "The meeting still starts, system-audio only — a webinar or a lecture stream is a real meeting — and says which track it is missing",
        // NOT `Test`, and unlike rows 1, 2 and 14 **no open PR owns this one**.
        // The decision is written here (`meeting_start_plan`), because nothing
        // else in the tree makes it: `MeetingSession::start` holds one
        // `CaptureStream` and a `hold()` that fails is the end of the meeting.
        // Turning that into "start anyway with the other source" is a change to
        // the capture session, which is YV106's, and publishing this row as
        // covered before that exists would be exactly the optimistic cell rows
        // `5b`/`16`/`17b` were demoted for.
        coverage: Coverage::PolicyOnly {
            test: "matrix_row3_mic_denied_system_only.rs",
            wiring_pr: None,
            absent_call_site: "meeting_start_plan",
        },
    },
    MatrixRow {
        id: "12",
        failure: "macOS older than 14.4",
        behavior: "The system-audio track is refused with one plain sentence naming the requirement, and mic-only meeting recording keeps working all the way down to Yap's macOS 12 floor",
        // `Test`, and the subject is the door the shipping app actually walks
        // through: `notetaker_status` calls `meeting_availability_for` for BOTH
        // capture modes and publishes the two answers separately. The half that
        // matters most is the negative one — the gate must never touch
        // `MicOnly`, on any OS — because a gate that leaked would turn "system
        // audio needs a newer Mac" into "meetings do not work on your Mac" for
        // every pre-14.4 user, with a green build.
        coverage: Coverage::Test {
            test: "matrix_row12_macos_144_gate.rs",
            subject: "meeting_availability_for",
            subject_module: "meeting_asr.rs",
        },
    },
    MatrixRow {
        id: "12b",
        failure: "…and the sentence the gate produces has to reach a Notetaker surface",
        behavior: "The Notetaker's system-audio control is visible and disabled, carrying that sentence — the plan's own wording for row 12",
        // `Test` as of YV102 (#125) — the split closes. The sentence now has
        // exactly the surface the plan's own wording for row 12 asks for:
        // `App.tsx` invokes `notetaker_status` on mount, feeds
        // `systemAudioMessage` into `setupState`, and renders the
        // "Set up meeting recording" step with its button DISABLED and that
        // sentence underneath — visible and disabled, carrying the reason, on
        // every pre-14.4 Mac.
        //
        // The subject is the payload rather than the command, because the
        // command is a settings read plus `NotetakerStatus::for_os` and the
        // payload is the whole contract: the two answers are separate fields,
        // and their camelCase names are what the surface reads.
        coverage: Coverage::Test {
            test: "matrix_row12_macos_144_gate.rs",
            subject: "NotetakerStatus",
            subject_module: "lib.rs",
        },
    },
    MatrixRow {
        id: "14",
        failure: "Output device changes mid-meeting (AirPods connect)",
        behavior: "Tear down and rebuild the aggregate around the new output device, splice the spill, log a `device_change` marker — this happens constantly in real use and is not an edge case",
        // NOT `Test`. YV103's state machine ships, and so does a handler for its
        // action — but `record.rs`'s handler exists precisely to LOG that a
        // rebuild action arrived on the mic path, where it means nothing. The
        // producer is missing: nothing calls `InputFormatWatch::watch_output`,
        // so `output_device_uid` is empty everywhere in the app and
        // `observe_output` short-circuits on `NO_OUTPUT_WATCHED` before it can
        // ever compare a device. The watch's output half is armed by 22-B's tap
        // session and by nothing else.
        coverage: Coverage::PolicyOnly {
            test: "matrix_row14_output_device_change.rs",
            wiring_pr: Some("#123 (YV100)"),
            absent_call_site: "watch_output",
        },
    },
    // ── yap23 · the three diarization rows (YV132) ──────────────────────────
    //
    // Appended, like `3a`/`3b` and 22-B's five were, so a reader who knows the
    // table sees what arrived rather than having to diff it.
    MatrixRow {
        id: "7",
        failure: "Diarize sidecar OOM, panic or wedge",
        behavior: "Deadline + kill + restart budget 1; on give-up a plain single-speaker transcript, the meeting `complete` with `diarization_failed` — never `failed`, because the user still gets their notes",
        // NOT `Test`, and the half that IS shipping is what makes the other half
        // worth its own honest cell. `diarize::DiarizePool` really does hold the
        // deadline, the kill and the one-restart budget — YV121 ported all four
        // policies from `polish.rs` and `tests/diarize_sidecar_pool.rs` drives
        // them against a stub process. What does not exist is a meeting that
        // ever asks it anything: `diarize::pool()` has no caller in `src/`, so
        // no meeting can be marked `diarization_failed` because no meeting is
        // ever diarized. The decision below says what a meeting BECOMES when
        // the pool gives up; the day something calls it, this row is a `Test`
        // row and `matrix_coverage`'s tripwire says so out loud.
        coverage: Coverage::PolicyOnly {
            test: "matrix_row7_diarize_sidecar_wedge.rs",
            wiring_pr: None,
            absent_call_site: "diarize_sidecar_degrade",
        },
    },
    MatrixRow {
        id: "8",
        failure: "Diarization returns garbage — one cluster for five people, or forty clusters",
        behavior: "A ranking + floor, never a hard reject: clusters under the caller's floor roll into one bucket, and a pass with nothing above it degrades to the same plain transcript as row 7 — logged distinctly, so a crashed sidecar and a noisy one stay diagnosable apart. YV126 shipped `diarize::rank_and_floor` and made it APPLY this row rather than restate it: the shipping gate calls `cluster_sanity` and carries its verdict, so an all-below-floor pass draws no chips at all. Nothing in the app calls the shipping gate yet, which is what keeps this row unwired",
        // NOT `Test`, for the same reason as row 7 and one more of its own: the
        // gate can only be applied to output, and nothing in the app produces
        // any. The published behaviour is deliberately NOT the plan's original
        // `count > max(8, attendees×2)` reject — merged finding #25 killed that
        // rule (a manually started meeting has no attendee count, so the cap is
        // 8, and a real six-person far-field room legitimately produces 10–15
        // raw clusters before merge, which the rule would throw away whole).
        //
        // `absent_call_site` MOVED from `cluster_sanity` to `rank_and_floor`
        // when YV126 landed, and the move is the honest half of that landing.
        // `cluster_sanity` now HAS a caller — `diarize::rank_and_floor` applies
        // it — so a tripwire watching `cluster_sanity` would fire on the very
        // change that made the row's published behaviour true, and it would be
        // firing at the wrong symbol: the thing still absent from the app is
        // the gate itself.
        //
        // The alternative the old tripwire suggested was to promote this row to
        // `Coverage::Test`. That would be false. `Coverage::Test` means a test
        // drives code "wired into the shipping app", and nothing in the app
        // calls `rank_and_floor` — no meeting is ever diarized. Promoting on
        // "the degrade acquired a caller inside the diarization module" would
        // publish a reachability this row does not have, which is the exact
        // failure `PolicyOnly` was added for. So the row stays `PolicyOnly`,
        // names the symbol that is genuinely absent, and its test now drives the
        // SHIPPING gate rather than a pure function beside it.
        coverage: Coverage::PolicyOnly {
            test: "matrix_row8_diarize_garbage_clusters.rs",
            wiring_pr: None,
            absent_call_site: "rank_and_floor",
        },
    },
    MatrixRow {
        id: "13",
        failure: "Diarization model missing, or partially downloaded",
        behavior: "The same gate the polish stage uses — the speaker-detection affordance is OFF, not broken, naming the combined download; recording and transcripts are untouched",
        // NOT `Test`. The gate's INPUT ships: `models::is_diarize_downloaded`
        // checks the extracted graph at its full expected size (so a half
        // download reads as missing rather than as present-and-corrupt), and
        // `models::diarize_download_bytes` adds the two catalog entries up. What
        // is missing is the affordance: no Notetaker surface offers speaker
        // detection at all, so there is nothing yet to switch off politely, and
        // `NotetakerStatus` carries no field for it. The sentence is decided
        // here and reaches nobody, which is `PolicyOnly` exactly.
        coverage: Coverage::PolicyOnly {
            test: "matrix_row13_diarize_model_missing.rs",
            wiring_pr: None,
            absent_call_site: "speaker_detection_gate",
        },
    },
];

impl Coverage {
    /// The cell as it appears in the rendered table.
    pub fn cell(self) -> String {
        match self {
            Coverage::Test {
                test,
                subject,
                subject_module,
            } => format!(
                "`cargo test --test {}` — drives `{subject}` in `{subject_module}`",
                stem(test)
            ),
            Coverage::Manual(doc) => format!("Manual repro — `{doc}`"),
            Coverage::PolicyOnly {
                test,
                wiring_pr: Some(pr),
                absent_call_site,
            } => format!(
                "**Policy only** — the enforcement (`{absent_call_site}`) lands with {pr}; `cargo test --test {}` covers only the part this branch owns",
                stem(test)
            ),
            Coverage::PolicyOnly {
                test,
                wiring_pr: None,
                absent_call_site,
            } => format!(
                "**Policy only, NOT WIRED** — `cargo test --test {}` covers the decision; **nothing calls `{absent_call_site}`**, so the app does not do this yet",
                stem(test)
            ),
        }
    }

    /// The test file this cell names, if it names one. `Manual` rows name a
    /// document instead and are the one honest way for a row to have no test.
    pub fn test_file(self) -> Option<&'static str> {
        match self {
            Coverage::Test { test, .. } | Coverage::PolicyOnly { test, .. } => Some(test),
            Coverage::Manual(_) => None,
        }
    }

    /// The shipping symbol and the module it must live in, for a `Test` row.
    ///
    /// This is the pair `tests/matrix_coverage.rs` checks from both ends — the
    /// symbol is real code in that module, and the row's test names it — which
    /// is what stops a `Test` cell from being satisfied by a green test aimed
    /// at something else that merely looks like the row's subject.
    pub fn shipping_subject(self) -> Option<(&'static str, &'static str)> {
        match self {
            Coverage::Test {
                subject,
                subject_module,
                ..
            } => Some((subject, subject_module)),
            _ => None,
        }
    }
}

/// `foo.rs` → `foo`, which is what `cargo test --test` wants.
fn stem(file: &str) -> &str {
    file.strip_suffix(".rs").unwrap_or(file)
}

/// Render [`ROWS`] as the Markdown table that `docs/yap22a-error-matrix.md`
/// carries. The document is prose around this table; `tests/matrix_coverage.rs`
/// asserts the committed document still contains exactly this output, so a row
/// added in code and forgotten in the doc fails the build.
pub fn render_markdown() -> String {
    let mut out =
        String::from("| Row | Failure | Required behaviour | Coverage |\n|---|---|---|---|\n");
    for row in ROWS {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            row.id,
            row.failure,
            row.behavior,
            row.coverage.cell()
        ));
    }
    out
}

// ── Row 5 · the note the dropped writes owe the user ────────────────────────
//
// The bounded queue itself is NOT here, on purpose, and an earlier revision of
// this module got that wrong in a way worth recording. It carried a
// `JournalBackpressure` state machine — a pure model of a bounded queue with a
// drop counter — and row 5's test drove the model. A model is not the journal:
// `meeting::MeetingJournal` is the code a three-hour recording actually runs,
// it already exposes the seam needed to force the failure
// (`start_with_depth` + `pause_handle`, so the queue fills on demand instead of
// racing a disk), and a second implementation of the same invariant is the
// identical trap row 17's cap duplication was rejected for: change the shipping
// bound, and the model keeps agreeing with the published table while the app
// does something else. So the model is gone, and
// `tests/matrix_row5_journal_backpressure.rs` drives the real journal.
//
// What remains here is the half nothing else owns: the sentence the meeting
// owes the user when the disk cost it audio.

/// The sentence the meeting detail *would* show when the journal dropped
/// writes — row `5b`, published as **not wired**, because this function has no
/// callers. YV94 shipped the meeting detail before there was a capture path to
/// feed it a drop count, and this branch changes no UI, so the note is computed
/// and rendered nowhere. `tests/matrix_coverage.rs` fails the day
/// `quality_note` appears as code anywhere else in `src/`, which is the day
/// row `5b` becomes a real `Test` row.
///
/// **The argument is what the shipping journal actually counts.** An earlier
/// signature took a number of dropped *writes* plus a frames-per-write figure,
/// and `meeting::MeetingJournal::dropped_samples` — the only drop counter in
/// the app — reports neither: it counts SAMPLES, because a rejected `append`
/// carries however many frames that callback happened to hold. A policy whose
/// parameters cannot be filled from the data the caller has is a policy that
/// will be quietly re-derived at the call site, which is how the published
/// number and the shown number come apart. So it takes samples and a rate.
///
/// `None` when nothing was dropped — the overwhelmingly common case, and a
/// meeting detail that says "0 s dropped" is noise that teaches the user to
/// ignore the line on the day it matters.
///
/// The rule the plan asks for is *honest*, which here means: say what it cost in
/// seconds, do not dress it up, and do not claim precision we do not have — a
/// loss under a tenth of a second says so rather than rounding to `0.0 s`.
pub fn quality_note(dropped_samples: u64, sample_rate: u32) -> Option<String> {
    if dropped_samples == 0 {
        return None;
    }
    let seconds = if sample_rate == 0 {
        0.0
    } else {
        (dropped_samples as f64) / (sample_rate as f64)
    };
    let amount = if seconds < 0.1 {
        "under a tenth of a second".to_string()
    } else {
        format!("about {seconds:.1} s")
    };
    Some(format!(
        "The disk fell behind while recording: {amount} of audio was dropped and is missing from this meeting. Everything else recorded normally."
    ))
}

// ── Row 16 · sleep and wake ─────────────────────────────────────────────────

/// The value written to the meeting row when sleep interrupted it. A marker,
/// not a state: the meeting is still `partial`/`complete` by its own lifecycle,
/// and this says *why* there is a seam in it.
pub const PAUSED_BY_SLEEP: &str = "paused_by_sleep";

/// The two notifications that matter, in the order macOS delivers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepEvent {
    /// `NSWorkspaceWillSleepNotification`. Delivered *before* the machine goes
    /// down, with a short and unspecified budget to act in.
    WillSleep,
    /// `NSWorkspaceDidWakeNotification`.
    DidWake,
}

/// What the capture session must do about a sleep event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepAction {
    /// Nothing to do — no meeting is in flight, or this event is a duplicate.
    /// Duplicates are not hypothetical: dark wakes and repeated notifications
    /// are normal, and a second prompt stacked on the first is a bug.
    Ignore,
    /// Finalize the journal now and mark the meeting [`PAUSED_BY_SLEEP`].
    FinalizeAndPause,
    /// We woke while still believing we were recording — the will-sleep
    /// notification never arrived (or arrived too late to act on). Finalize what
    /// is on disk AND offer the resume prompt in one step. The one thing that
    /// must not happen is treating the audio either side of the gap as
    /// continuous, because it is not.
    FinalizeMissedSleepAndOfferResume,
    /// Ask the user whether to resume — as a NEW segment, explicitly not as a
    /// continuation of the pre-sleep audio.
    OfferResumeAsNewSegment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SleepState {
    Idle,
    Recording,
    PausedBySleep,
    AwaitingResume,
}

/// Row 16 as a four-state machine. Fed by the workspace notification observer;
/// its answers are what the session acts on.
#[derive(Debug, Clone)]
pub struct SleepPolicy {
    state: SleepState,
}

impl Default for SleepPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl SleepPolicy {
    pub fn new() -> Self {
        Self {
            state: SleepState::Idle,
        }
    }

    /// A meeting started recording.
    pub fn meeting_started(&mut self) {
        self.state = SleepState::Recording;
    }

    /// The user stopped the meeting (or it hit the cap). Any pending resume
    /// offer dies with it — resuming a meeting the user already ended would be
    /// the app arguing with them.
    pub fn meeting_stopped(&mut self) {
        self.state = SleepState::Idle;
    }

    /// Feed one notification, get the action to perform.
    pub fn observe(&mut self, event: SleepEvent) -> SleepAction {
        match (self.state, event) {
            (SleepState::Recording, SleepEvent::WillSleep) => {
                self.state = SleepState::PausedBySleep;
                SleepAction::FinalizeAndPause
            }
            (SleepState::Recording, SleepEvent::DidWake) => {
                self.state = SleepState::AwaitingResume;
                SleepAction::FinalizeMissedSleepAndOfferResume
            }
            (SleepState::PausedBySleep, SleepEvent::DidWake) => {
                self.state = SleepState::AwaitingResume;
                SleepAction::OfferResumeAsNewSegment
            }
            // Idle: nothing recording. PausedBySleep + WillSleep: already
            // finalized, a second notification changes nothing. AwaitingResume:
            // the prompt is already up; sleeping and waking again must not stack
            // a second one.
            _ => SleepAction::Ignore,
        }
    }

    /// The user accepted the resume prompt. Returns false (and changes nothing)
    /// if no prompt was outstanding.
    pub fn resume_accepted(&mut self) -> bool {
        if self.state == SleepState::AwaitingResume {
            self.state = SleepState::Recording;
            true
        } else {
            false
        }
    }

    /// The user declined, or dismissed the prompt.
    pub fn resume_declined(&mut self) -> bool {
        if self.state == SleepState::AwaitingResume {
            self.state = SleepState::Idle;
            true
        } else {
            false
        }
    }

    /// True while the meeting is stopped *because* of sleep — the condition the
    /// [`PAUSED_BY_SLEEP`] marker records.
    pub fn paused_by_sleep(&self) -> bool {
        matches!(
            self.state,
            SleepState::PausedBySleep | SleepState::AwaitingResume
        )
    }

    /// True while audio is being captured.
    pub fn is_recording(&self) -> bool {
        self.state == SleepState::Recording
    }

    /// True while a resume prompt is outstanding.
    pub fn awaiting_resume(&self) -> bool {
        self.state == SleepState::AwaitingResume
    }
}

// ── Row 17 · what carries on past the cap ───────────────────────────────────
//
// The cap itself is NOT here, on purpose.
//
// `meeting::MEETING_HARD_CAP` (3 h), `meeting::MEETING_CAP_WARN_AT` (2 h 45 m)
// and `meeting::watchdog_tick` are the thresholds and the rule that actually
// run — the 60 s watchdog reads them, latches `cap_warned` in its own loop, and
// stops the session. An earlier revision of this module declared its own cap
// constants and its own warn/stop latch. Two copies of a number with no
// compile-time link between them is a trap with a specific shape: someone
// changes the cap in the module that ships, this module's test stays green, and
// `docs/yap22a-error-matrix.md` — which is published as the row's required
// behaviour — goes quietly wrong. That is the exact hazard this PR cited to
// justify NOT duplicating row 4's preflight, so it does not get to make an
// exception for row 17. Row 17's test therefore IMPORTS those constants and
// drives `watchdog_tick`; `tests/matrix_row17_meeting_cap.rs` and
// `tests/matrix_coverage.rs` both fail if a copy reappears here.
//
// What remains is the half the capture session does not build: the meeting that
// continues after the stop. The watchdog stops at the cap and stops there —
// nothing starts the next meeting, and nothing calls `continuation_title` — so
// that half is published as its own cell, row `17b`, `PolicyOnly`.

/// The title of the meeting that carries on past the cap.
///
/// A 9-hour recording session is three meetings, so this has to survive being
/// applied to its own output: `Lecture` → `Lecture (continued)` →
/// `Lecture (continued 2)` → `Lecture (continued 3)`, never
/// `Lecture (continued) (continued)`.
pub fn continuation_title(previous_title: &str) -> String {
    let base = previous_title.trim_end();
    if base.is_empty() {
        return "(continued)".to_string();
    }
    if let Some(head) = base.strip_suffix(')') {
        if let Some((prefix, tail)) = head.rsplit_once('(') {
            let tail = tail.trim();
            let prefix = prefix.trim_end();
            if tail == "continued" {
                return format!("{prefix} (continued 2)");
            }
            if let Some(n) = tail.strip_prefix("continued ") {
                if let Ok(n) = n.trim().parse::<u32>() {
                    return format!("{prefix} (continued {})", n.saturating_add(1));
                }
            }
        }
    }
    format!("{base} (continued)")
}

// ── Row 3 · which tracks a meeting starts with when a source is missing ─────
//
// The tap itself is NOT here, and neither is the mic, for the same reason the
// cap and the journal are not: `MeetingSession` opens the streams and
// `syscapture` opens the tap. What is here is the half nothing in the tree
// decides — *given that one of the two sources could not be opened, does the
// meeting start anyway, and as what?*
//
// Today `MeetingSession::start` holds one `CaptureStream`, and a `hold()` that
// returns `Err` ends the attempt. That is correct for 22-A, where the mic is
// the only source and a meeting without it is nothing. It stops being correct
// the moment there is a second source, and the plan's row 3 is that case named:
// a lecture stream or a webinar recorded with the presenter's mic denied is a
// real, useful meeting. So this is the rule, written once, tested, and — until
// YV106 gives the session two sources to fail independently — called by
// nothing, which is what `Coverage::PolicyOnly` says out loud.

/// Whether one capture source could be opened, and if not, why — because the
/// two reasons need different sentences and only one of them is worth offering
/// a Settings deep link for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceAvailability {
    /// The source opened and is delivering.
    Ready,
    /// The OS refused it: TCC denied, or revoked, or the user said no. The one
    /// case where sending the user to Settings can actually fix something.
    Denied,
    /// The source cannot exist on this machine — the tap below macOS 14.4
    /// (matrix row 12), or no input device at all. Settings cannot fix it and
    /// the sentence must not pretend otherwise.
    Unsupported,
}

impl SourceAvailability {
    pub fn is_ready(self) -> bool {
        self == SourceAvailability::Ready
    }
}

/// What a meeting starts as, given what could be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeetingStartPlan {
    /// Start recording. `mic`/`system_audio` are the tracks that will actually
    /// carry audio — the pair `SessionConfig` needs — and `badge` is the note
    /// the meeting owes the user when one of them is missing.
    ///
    /// `badge` is `None` only when **both** sources opened. A meeting that is
    /// quietly missing half of what the user expected is the failure mode row 1
    /// and row 3 are both about; the plan says "badge it" for exactly that
    /// reason.
    Start {
        mic: bool,
        system_audio: bool,
        badge: Option<&'static str>,
    },
    /// Nothing could be opened. The only case where refusing is honest — and it
    /// carries the sentence, because "recording failed" is not a reason.
    Refuse { reason: &'static str },
}

impl MeetingStartPlan {
    /// How many journal tracks this plan opens: 2, 1, or 0.
    pub fn tracks(self) -> usize {
        match self {
            MeetingStartPlan::Start {
                mic, system_audio, ..
            } => usize::from(mic) + usize::from(system_audio),
            MeetingStartPlan::Refuse { .. } => 0,
        }
    }

    /// Did the meeting start at all?
    pub fn starts(self) -> bool {
        matches!(self, MeetingStartPlan::Start { .. })
    }
}

/// Row 3, and the fallback half of row 1, as one function: **a meeting starts
/// if either source opened, and it says which one it is missing.**
///
/// One function rather than two because the two rows are the same decision seen
/// from opposite ends — mic denied with a tap (row 3), tap denied with a mic
/// (row 1) — and splitting them is how the two halves end up disagreeing about
/// the both-denied case. The refusal sentence names the source that was
/// refused, so a user who denied one thing is not told the other is broken.
pub fn meeting_start_plan(
    mic: SourceAvailability,
    system_audio: SourceAvailability,
) -> MeetingStartPlan {
    match (mic, system_audio) {
        (SourceAvailability::Ready, SourceAvailability::Ready) => MeetingStartPlan::Start {
            mic: true,
            system_audio: true,
            badge: None,
        },
        // Row 1: the tap is missing. 22-A's meeting, badged — and the badge
        // distinguishes "you turned it off" from "this Mac cannot do it",
        // because only the first one has a next step.
        (SourceAvailability::Ready, SourceAvailability::Denied) => MeetingStartPlan::Start {
            mic: true,
            system_audio: false,
            badge: Some(
                "System audio is not being recorded — Yap does not have permission to capture it. \
                 Your microphone track is recording normally.",
            ),
        },
        (SourceAvailability::Ready, SourceAvailability::Unsupported) => MeetingStartPlan::Start {
            mic: true,
            system_audio: false,
            badge: Some(
                "System audio is not being recorded — this Mac cannot capture it. \
                 Your microphone track is recording normally.",
            ),
        },
        // Row 3: the mic is missing and the tap is not. A webinar or a lecture
        // stream recorded from the other end of the call is the whole point.
        (SourceAvailability::Denied, SourceAvailability::Ready) => MeetingStartPlan::Start {
            mic: false,
            system_audio: true,
            badge: Some(
                "Your microphone is not being recorded — Yap does not have permission to use it. \
                 The call's own audio is recording normally.",
            ),
        },
        (SourceAvailability::Unsupported, SourceAvailability::Ready) => MeetingStartPlan::Start {
            mic: false,
            system_audio: true,
            badge: Some(
                "Your microphone is not being recorded — Yap could not open an input device. \
                 The call's own audio is recording normally.",
            ),
        },
        // Neither source. The sentence names the microphone first because that
        // is the permission the user is far more likely to be able to fix, and
        // on a pre-14.4 Mac the system-audio half was never on offer anyway.
        (SourceAvailability::Denied, _) => MeetingStartPlan::Refuse {
            reason: "Yap does not have permission to use your microphone, and there is no system \
                     audio to record instead. Grant microphone access in System Settings › \
                     Privacy & Security › Microphone.",
        },
        (SourceAvailability::Unsupported, _) => MeetingStartPlan::Refuse {
            reason: "Yap could not open an input device, and there is no system audio to record \
                     instead. Connect a microphone and try again.",
        },
    }
}

// ── Rows 7 and 8 · what a meeting becomes when diarization does not ─────────
//
// The sidecar is NOT here, and neither is the clusterer. `diarize::DiarizePool`
// holds the deadline, the kill, the one-restart budget and the idle sweep;
// YV126's `cluster_track` will hold the clustering. What is here is the half
// neither of them owns — *given that the speaker labels did not arrive, what is
// the meeting?* — because that is a decision about a MEETING, and the module
// that talks to a child process over a pipe is the wrong place to make it.
//
// Both rows resolve to the same artifact, and the plan's §6 preamble is why:
// "always degrade to the next-best artifact… diarization fails ⇒ keep the plain
// transcript". They differ in the reason they carry, and that difference is the
// whole of row 8's separate existence: a sidecar that crashed and a sidecar that
// ran and produced noise are two different bugs with two different fixes, and a
// single `diarization_failed` with no reason attached makes them one line in a
// log that tells nobody which one happened.

/// The marker a meeting row carries when its transcript is real and its speaker
/// labels are not.
///
/// Deliberately a marker and NOT a `meetings.state`: the state says whether the
/// recording and the transcription finished, which is a different question that
/// diarization has no vote in. See [`SpeakerLabels::meeting_state_after`].
pub const DIARIZATION_FAILED: &str = "diarization_failed";

/// Why a meeting ended up with no speaker labels — row 7's failure and row 8's,
/// kept apart on purpose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DegradeReason {
    /// **Row 7.** The sidecar never produced an answer: it would not start, it
    /// died past its restart budget, it blew the deadline, or it answered
    /// something that is not a response. Carries `DiarizeError::tag`'s own short
    /// word rather than a second vocabulary invented here, so the log line and
    /// the pool's diagnostics agree by construction.
    Sidecar { tag: String },
    /// **Row 8.** The sidecar ran, answered, and the answer is unusable —
    /// nothing in it cleared the caller's floor. `raw_clusters` is what it
    /// returned, which is the number worth having in the log: 1 and 40 are both
    /// this case, and they are not the same bug.
    Clusters { raw_clusters: usize },
}

impl DegradeReason {
    /// The word that separates the two failure modes in a log or a diagnostics
    /// row. Short, fixed, and never a path or a sentence.
    pub fn log_tag(&self) -> &'static str {
        match self {
            DegradeReason::Sidecar { .. } => "sidecar",
            DegradeReason::Clusters { .. } => "clusters",
        }
    }

    /// The one line this degrade writes to the log — the two failure modes
    /// distinguishable by a human reading it, which is the acceptance criterion
    /// row 8 is really about.
    pub fn log_line(&self) -> String {
        match self {
            DegradeReason::Sidecar { tag } => format!(
                "{DIARIZATION_FAILED} reason=sidecar detail={tag} — the transcript is intact and \
                 the meeting is complete"
            ),
            DegradeReason::Clusters { raw_clusters } => format!(
                "{DIARIZATION_FAILED} reason=clusters raw_clusters={raw_clusters} — the diarizer \
                 answered and nothing it returned cleared the floor; the transcript is intact and \
                 the meeting is complete"
            ),
        }
    }
}

/// What a finished meeting's transcript carries for speaker attribution.
///
/// **There is deliberately no variant that fails, discards or truncates a
/// meeting**, and that absence is the mechanism behind the plan's governing
/// principle for this row ("the user still gets their notes"). A degrade path
/// that is a branch somebody has to remember to take is a degrade path that is
/// one refactor from being lost; a degrade path that is the only inhabitable
/// state of the return type is not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeakerLabels {
    /// Diarization ran and produced something worth showing.
    Attributed,
    /// The plain single-speaker transcript, plus the reason the labels are
    /// missing.
    Plain(DegradeReason),
}

impl SpeakerLabels {
    /// The marker the meeting row gains, or `None` when there is nothing to
    /// say.
    pub fn marker(&self) -> Option<&'static str> {
        match self {
            SpeakerLabels::Attributed => None,
            SpeakerLabels::Plain(_) => Some(DIARIZATION_FAILED),
        }
    }

    pub fn is_plain(&self) -> bool {
        matches!(self, SpeakerLabels::Plain(_))
    }

    pub fn reason(&self) -> Option<&DegradeReason> {
        match self {
            SpeakerLabels::Attributed => None,
            SpeakerLabels::Plain(reason) => Some(reason),
        }
    }

    /// The meeting's state, after diarization has had its say — which is the
    /// state it already had.
    ///
    /// **This is the identity function on purpose**, and it is the smallest
    /// honest way to write row 7's load-bearing half. The state answers "did the
    /// recording and the transcription finish", which diarization does not
    /// participate in: a meeting that transcribed cleanly is `Complete` whether
    /// or not the diarizer wedged, and one that lost chunks to YV93's per-chunk
    /// timeout is `Partial` for that reason and not this one. Expressing it as a
    /// function of the state the caller already holds means the call site
    /// *cannot* write `MeetingState::Failed` here without deleting this — which
    /// is a diff, in a file whose tests are about exactly this claim.
    pub fn meeting_state_after(&self, transcription_outcome: MeetingState) -> MeetingState {
        transcription_outcome
    }
}

/// **Row 7.** Every way the sidecar can fail to answer, mapped to the one thing
/// the meeting becomes.
///
/// The match is exhaustive over [`DiarizeError`] rather than a catch-all, so a
/// new failure mode added to the pool is a compile error here — a decision
/// somebody has to make about a meeting, instead of a variant that silently
/// inherits whatever the wildcard arm happened to say.
///
/// `Refused` is included and that is not an oversight. A refusal is the protocol
/// working (a missing model file, a request kind this build cannot serve), and
/// it must not spend the restart budget — which is `DiarizePool`'s rule and
/// stays `DiarizePool`'s rule. From the MEETING's point of view, though, a
/// refusal and a wedge are the same event: no labels arrived. The tag is what
/// keeps them apart afterwards.
pub fn diarize_sidecar_degrade(err: &DiarizeError) -> SpeakerLabels {
    let tag = match err {
        DiarizeError::Unavailable
        | DiarizeError::Deadline
        | DiarizeError::Protocol
        | DiarizeError::Refused(_) => err.tag().to_string(),
    };
    SpeakerLabels::Plain(DegradeReason::Sidecar { tag })
}

/// One cluster, as the sanity gate sees it. Total speech and turn count are the
/// two figures a ranking can be built from without asking anything about
/// identity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClusterSummary {
    /// The diarizer's own cluster index — passed through, never renumbered
    /// here.
    pub cluster: u32,
    pub speech_seconds: f64,
    pub turns: usize,
}

/// The floor a cluster must clear to earn a speaker chip.
///
/// **Both numbers belong to the caller, and this module holds no opinion about
/// either.** The plan's illustrative "30 s and 3 turns" is not a `const` here,
/// and neither is anything else: YV126 measures the floor against
/// `diarize_metrics` on the eval fixtures, and a number typed into the module
/// that renders the published matrix row would be a guess that arrives wearing
/// the authority of the table. `matrix_row8_diarize_garbage_clusters` asserts
/// the absence, from both ends — no threshold constant in this file, and the
/// same cluster set flipping verdict under two different floors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClusterFloor {
    pub min_speech_seconds: f64,
    pub min_turns: usize,
}

impl ClusterFloor {
    /// Does this cluster earn a chip?
    pub fn admits(&self, cluster: &ClusterSummary) -> bool {
        cluster.speech_seconds >= self.min_speech_seconds && cluster.turns >= self.min_turns
    }
}

/// What the UI does with one diarization pass.
#[derive(Debug, Clone, PartialEq)]
pub struct ClusterVerdict {
    /// Row 7's and row 8's shared artifact question: labels, or the plain
    /// transcript and why.
    pub labels: SpeakerLabels,
    /// The clusters that earned a chip, ranked by total speech, most first.
    /// Empty exactly when `labels` is [`SpeakerLabels::Plain`].
    pub chips: Vec<u32>,
    /// How many raw clusters rolled into the single "Other" bucket — the
    /// number the plan's "never show 40 speaker chips" is really about.
    pub other: usize,
}

/// **Row 8.** Rank the clusters, apply the caller's floor, and degrade only when
/// nothing survives it.
///
/// **This is deliberately NOT the plan's original rule.** §6 row 8 says "cluster
/// count > `max(8, attendees×2)` ⇒ reject", and merged finding #25 is why that
/// rule is not implemented: a manually started meeting has no attendee count, so
/// the cap is 8, and a genuine six-person far-field room routinely produces
/// 10–15 raw clusters before any merge — the exact case yap23 prioritises would
/// have its whole diarization pass thrown away by the sanity gate meant to
/// protect it. Ranking with a floor keeps that room's four real speakers and
/// puts the confetti in one "Other" bucket, and the forty-cluster pass the plan
/// was worried about still degrades, because forty clusters of noise are forty
/// clusters that are each too short to clear any floor.
///
/// **What this gate cannot see, said plainly.** The other half of the plan's row
/// 8 — "one cluster for five people" — is not detectable here and no threshold
/// makes it so: one long cluster clears every floor, and telling it apart from a
/// genuinely single-speaker recording needs ground truth. That half belongs to
/// YV126's DER gate on the eval fixtures, which is a measurement and not a
/// runtime check. A gate that pretended otherwise would be the more dangerous
/// failure: a green sanity check standing in for an accuracy claim nobody made.
///
/// **This is a requirement on YV126's gate, and YV126 met it by CALLING this
/// function rather than restating it.** [`crate::diarize::rank_and_floor`] is
/// the gate a meeting will call; it now builds a [`ClusterSummary`] per cluster,
/// asks this function for the verdict, and carries the answer on
/// `ClusterRanking::labels`, so `ClusterRanking::chips()` returns nothing for a
/// pass in which nothing cleared the floor. `rank_and_floor`'s "never reject"
/// half is untouched and is what finding #25 asked for — every raw cluster is
/// still present and still correctable by YV130 — because "never reject" is not
/// "never degrade": a pass in which *nothing* cleared the floor found no
/// speaker, and rendering it as one "Other" chip presents noise as attribution.
///
/// The matrix is the SSOT for what Yap owes a user on a failure, so it may not
/// publish a behaviour the mechanism serving it quietly does not implement.
/// There is now exactly one implementation of that behaviour and it is this
/// one; `matrix_row8_diarize_garbage_clusters` drives it through the shipping
/// gate as well as directly, and asserts the two cannot disagree.
pub fn cluster_sanity(clusters: &[ClusterSummary], floor: ClusterFloor) -> ClusterVerdict {
    let mut ranked: Vec<&ClusterSummary> = clusters.iter().filter(|c| floor.admits(c)).collect();
    // Most speech first; ties broken by the diarizer's own index so the order is
    // total and a chip row cannot reshuffle between two identical passes.
    ranked.sort_by(|a, b| {
        b.speech_seconds
            .partial_cmp(&a.speech_seconds)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cluster.cmp(&b.cluster))
    });

    if ranked.is_empty() {
        return ClusterVerdict {
            labels: SpeakerLabels::Plain(DegradeReason::Clusters {
                raw_clusters: clusters.len(),
            }),
            chips: Vec::new(),
            other: clusters.len(),
        };
    }

    let chips: Vec<u32> = ranked.iter().map(|c| c.cluster).collect();
    ClusterVerdict {
        other: clusters.len() - chips.len(),
        chips,
        labels: SpeakerLabels::Attributed,
    }
}

// ── Row 13 · the model that is not there ────────────────────────────────────
//
// The catalog half ships and is NOT re-declared here: `models::is_diarize_ready`
// is `is_diarize_downloaded` over both roles, and `is_diarize_downloaded`
// compares the file on disk against the size the catalog states — which is what
// makes "partially downloaded" read as missing rather than as present. This
// module takes the answer as a bool and the size as a number, so there is no
// second copy of either and nothing here touches a disk.

/// Whether the Notetaker can offer speaker detection, and the sentence it shows
/// when it cannot.
///
/// Two variants, and neither of them says anything about recording. That is the
/// type carrying row 13's other half: "the feature is off, not broken" is only
/// true if the gate is physically unable to express "and recording is off too",
/// which is the failure row 12 had to spend most of its test budget guarding
/// against with a boolean that COULD say it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeakerDetection {
    /// Both halves of the model pair are installed at their full expected size.
    Available,
    /// Off. The affordance is visible and disabled, carrying this sentence —
    /// the same shape the 14.4 gate's row `12b` already ships.
    NeedsDownload { megabytes: u64, message: String },
}

impl SpeakerDetection {
    pub fn is_available(&self) -> bool {
        matches!(self, SpeakerDetection::Available)
    }

    pub fn message(&self) -> Option<&str> {
        match self {
            SpeakerDetection::Available => None,
            SpeakerDetection::NeedsDownload { message, .. } => Some(message),
        }
    }
}

/// Megabytes as every other download size in this app is spelled.
///
/// `ModelSetup.tsx` renders a catalog size as `Math.round(bytes / 1e6)` and
/// `cli.rs` prints `bytes / 1e6` to zero decimals; a third convention here would
/// mean the same 36 MB download is "36 MB" in the model list and "35 MB" (MiB,
/// rounded) in the speaker-detection sentence, which reads as two downloads.
fn megabytes(bytes: u64) -> u64 {
    ((bytes as f64) / 1_000_000.0).round() as u64
}

/// **Row 13.** The gate, as one pure function over the two facts `models.rs`
/// already computes.
///
/// **The size is computed, never typed**, and the correction is worth recording
/// because the number in the plan is wrong. §6 row 13 says "download 37 MB to
/// enable speaker detection", written before the assets were vendored; the two
/// entries YV123 actually pinned are 6,958,444 B + 29,292,684 B = 36.25 MB, so
/// the sentence says **36 MB**. Passing `models::diarize_download_bytes()` in is
/// what keeps it true the day a model is re-pinned — a literal would have been
/// wrong twice: once against the plan, and again against the next catalog edit.
pub fn speaker_detection_gate(models_ready: bool, download_bytes: u64) -> SpeakerDetection {
    if models_ready {
        return SpeakerDetection::Available;
    }
    let megabytes = megabytes(download_bytes);
    SpeakerDetection::NeedsDownload {
        megabytes,
        message: format!(
            "Download {megabytes} MB to enable speaker detection. Meeting recording and \
             transcripts work without it."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_ids_are_unique_and_nonempty() {
        let mut seen: Vec<&str> = Vec::new();
        for row in ROWS {
            assert!(!row.id.is_empty());
            assert!(!row.failure.is_empty());
            assert!(!row.behavior.is_empty());
            assert!(!seen.contains(&row.id), "duplicate matrix row {}", row.id);
            seen.push(row.id);
        }
        assert_eq!(
            ROWS.len(),
            19,
            "22-A owns eight matrix failures, 22-B's YV105 wires five more and \
             YV132 wires yap23's three (7, 8, 13), published as nineteen cells \
             — three of them (5, 12 and 17) have a wired half and an unwired \
             half, and one cell each way is the only way to state both \
             truthfully"
        );
    }

    #[test]
    fn rendered_table_has_a_line_per_row() {
        let md = render_markdown();
        // header + separator + one line per row
        assert_eq!(md.lines().count(), ROWS.len() + 2);
        for row in ROWS {
            assert!(md.contains(&format!("| {} |", row.id)));
        }
    }

    #[test]
    fn coverage_cells_name_the_runnable_thing_and_the_code_it_drives() {
        let cell = Coverage::Test {
            test: "matrix_row5_journal_backpressure.rs",
            subject: "MeetingJournal",
            subject_module: "meeting.rs",
        }
        .cell();
        assert!(
            cell.contains("cargo test --test matrix_row5_journal_backpressure"),
            "{cell}"
        );
        // The subject is IN the published cell, not just in the code: a reader
        // of the table can see which journal row 5 is about without opening a
        // test file, which is precisely what nobody did last time.
        assert!(cell.contains("`MeetingJournal` in `meeting.rs`"), "{cell}");

        assert_eq!(
            Coverage::Manual("docs/yap22a-phase-demo.md").cell(),
            "Manual repro — `docs/yap22a-phase-demo.md`"
        );
    }

    /// A `PolicyOnly` cell must never be mistakable for a `Test` cell by
    /// someone skimming the rendered document — that mistake is the whole
    /// reason the variant exists.
    #[test]
    fn policy_only_cells_say_the_app_does_not_do_this_yet() {
        let unowned = Coverage::PolicyOnly {
            test: "matrix_row16_sleep_wake.rs",
            wiring_pr: None,
            absent_call_site: "NSWorkspaceWillSleepNotification",
        }
        .cell();
        assert!(unowned.contains("NOT WIRED"), "{unowned}");
        assert!(unowned.contains("nothing calls"), "{unowned}");
        assert!(
            unowned.contains("NSWorkspaceWillSleepNotification"),
            "{unowned}"
        );

        let owned = Coverage::PolicyOnly {
            test: "matrix_row16_sleep_wake.rs",
            wiring_pr: Some("#999 (example)"),
            absent_call_site: "NSWorkspaceWillSleepNotification",
        }
        .cell();
        assert!(owned.contains("Policy only"), "{owned}");
        assert!(owned.contains("#999 (example)"), "{owned}");
        // And it still names something runnable, so the row is not a shrug.
        assert!(
            owned.contains("cargo test --test matrix_row16_sleep_wake"),
            "{owned}"
        );
    }

    /// The rows whose enforcement is absent, spelled out. If a future edit
    /// promotes any of them to `Test`, that edit is claiming the call site now
    /// exists — `tests/matrix_coverage.rs` is where that claim gets checked, and
    /// this assertion is the reminder that flipping it is a deliberate act.
    #[test]
    fn the_unwired_rows_are_the_ones_whose_call_site_is_missing() {
        let unwired: Vec<&str> = ROWS
            .iter()
            .filter(|r| matches!(r.coverage, Coverage::PolicyOnly { .. }))
            .map(|r| r.id)
            .collect();
        assert_eq!(
            unwired,
            vec!["5b", "16", "17b", "3", "14", "7", "8", "13"],
            "22-A's three unwired policies, the four YV105 rows whose mechanism \
             has merged and whose producer has not, and YV132's three yap23 \
             rows — whose mechanism (the sidecar pool, the model catalog) has \
             merged and which nothing in the app can reach, because no meeting \
             is ever diarized. `12b` left this list with YV102: the 14.4 \
             sentence reaches the Settings step, which is the one thing that \
             row was waiting for."
        );
    }

    /// Of the unwired rows, which ones have somebody bringing the wiring and
    /// which are waiting on nobody. The distinction is the whole reason
    /// `wiring_pr` is an `Option`, and stating it here means an open PR that
    /// merges without promoting its rows shows up as a stale `Some` a reader
    /// can act on rather than a claim nobody re-checks.
    ///
    /// **YV132's three rows join the unowned set, and that was checked rather
    /// than assumed.** yap23 has open PRs (YV126's clustering among them), so
    /// `Some("#141")` was the tempting cell — but the PR that brings a
    /// mechanism is not the PR that brings a CALL SITE, which is the only thing
    /// `wiring_pr` is about, and #141 states in its own evidence that it adds no
    /// production callers. Naming it here would have been the stale `Some` row
    /// 14 already demonstrates, planted deliberately.
    #[test]
    fn the_rows_nobody_owns_are_5b_16_17b_and_3() {
        let unowned: Vec<&str> = ROWS
            .iter()
            .filter(|r| {
                matches!(
                    r.coverage,
                    Coverage::PolicyOnly {
                        wiring_pr: None,
                        ..
                    }
                )
            })
            .map(|r| r.id)
            .collect();
        // Rows 1 and 2 LEFT this set with YV110. Both joined it the same way —
        // the PR named as the owner of their wiring merged and the wiring did
        // not come with it — and both left it the same way too: something now
        // calls `start_system_tap` inside a meeting (`meeting_control::open_meeting_tap`)
        // and something now answers `Some` to `tap_liveness`
        // (`syscapture::TappedEnv`), so the tripwire that guarded their absence
        // fired and the rows were promoted rather than the assertion relaxed.
        // Row 3 stays: a meeting whose MIC is denied still cannot start on the
        // tap alone, because `MeetingSession::start` holds one `CaptureStream`
        // and a `hold()` that fails is the end of the meeting.
        assert_eq!(unowned, vec!["5b", "16", "17b", "3", "7", "8", "13"]);

        let owned: Vec<(&str, &str)> = ROWS
            .iter()
            .filter_map(|r| match r.coverage {
                Coverage::PolicyOnly {
                    wiring_pr: Some(pr),
                    ..
                } => Some((r.id, pr)),
                _ => None,
            })
            .collect();
        // Row 14 KEEPS `#123 (YV100)` on purpose, and it is the stale `Some`
        // this test's doc comment predicted rather than an oversight. YV103
        // shipped `InputFormatWatch::watch_output` and could not wire it,
        // because `syscapture.rs` was not on `main` to wire it to; #123 merging
        // is what makes that wiring possible, and it is not in #123's scope —
        // that PR is explicit, in its spec and in its body, that nothing calls
        // the tap. So the row is still waiting, its named owner has landed, and
        // the pair below is the signal a reader acts on. Row 2 could not stay
        // here: its cell asserts that the *enforcement* arrives with the PR,
        // and `start_system_tap` did arrive — it just has no caller, which is
        // what `wiring_pr: None` says and this cell does not.
        //
        // YV102 (#125) then emptied this list of everything but row 14: row
        // `12b`'s wiring genuinely arrived (it is a `Test` row now) and row 1's
        // did not (it is unowned, above). Row 14 is the last stale `Some`, and
        // it is the honest kind: `watch_output` ships, #123 merged, and giving
        // it a caller was never in that PR's scope.
        assert_eq!(owned, vec![("14", "#123 (YV100)")]);
    }

    /// The rows the shipping app actually performs, spelled out, because this
    /// is the number every reader of the table is really asking for. Ten cells
    /// are `Test`, one is a manual repro, and the remaining eight are decisions
    /// nothing in the app can reach.
    ///
    /// Row 12 was the only one of YV105's five that made it into this list on
    /// its own, because the 14.4 gate needs no tap to be in effect: it is a
    /// refusal, `notetaker_status` computes it on every call, and its
    /// load-bearing half — mic-only recording is never gated — is a property of
    /// the app today. `12b` joined it with YV102, which gave that refusal a
    /// surface, and rows **1 and 2 joined it with YV110**, which gave the tap a
    /// caller inside a meeting and the watchdog a producer for its liveness.
    /// Row 14 still waits on the other half of that: nothing calls
    /// `InputFormatWatch::watch_output`, so an output-device change mid-meeting
    /// still reaches no aggregate rebuild.
    #[test]
    fn the_wired_rows_are_the_ones_whose_mechanism_shipped() {
        let wired: Vec<&str> = ROWS
            .iter()
            .filter(|r| matches!(r.coverage, Coverage::Test { .. }))
            .map(|r| r.id)
            .collect();
        assert_eq!(
            wired,
            vec!["4", "5", "6", "17", "3a", "3b", "1", "2", "12", "12b"]
        );

        let manual: Vec<&str> = ROWS
            .iter()
            .filter(|r| matches!(r.coverage, Coverage::Manual(_)))
            .map(|r| r.id)
            .collect();
        assert_eq!(manual, vec!["15"]);
    }

    /// Every `Test` row names a subject, and no row names this module as the
    /// subject.
    ///
    /// This is the shape of the finding that produced the field. A `Test` cell
    /// whose subject were `meeting_matrix.rs` would be the table certifying its
    /// own policy as shipped behaviour — the exact move `PolicyOnly` exists to
    /// prevent, laundered through the stronger variant.
    #[test]
    fn no_test_row_certifies_this_modules_own_policy_as_shipping_behaviour() {
        for row in ROWS {
            let Some((subject, module)) = row.coverage.shipping_subject() else {
                continue;
            };
            assert!(!subject.is_empty(), "row {} names an empty subject", row.id);
            assert!(
                module != "meeting_matrix.rs",
                "matrix row {} is published as covered by code in this very module. \
                 A policy this module owns is `PolicyOnly` until something calls it.",
                row.id
            );
            assert!(
                module.ends_with(".rs"),
                "row {} names `{module}`, which is not a source file",
                row.id
            );
        }
    }

    /// Row `5b` is the quality note, and it is published as not wired because
    /// `quality_note` has no callers. Stated as a test so promoting the row
    /// without writing the call site fails here first.
    #[test]
    fn row_5b_admits_the_quality_note_reaches_no_surface() {
        let row = ROWS.iter().find(|r| r.id == "5b").expect("row 5b");
        assert_eq!(
            row.coverage,
            Coverage::PolicyOnly {
                test: "matrix_row5_journal_backpressure.rs",
                wiring_pr: None,
                absent_call_site: "quality_note",
            }
        );
        let cell = row.coverage.cell();
        assert!(cell.contains("NOT WIRED"), "{cell}");
        assert!(cell.contains("quality_note"), "{cell}");
    }

    /// Row 16 has no owning PR, and that is a fact about the phase rather than
    /// an oversight in the table. Stating it in a test means the day someone
    /// picks the work up, they update the row instead of leaving a stale `None`.
    #[test]
    fn row_16_admits_that_nobody_owns_its_wiring() {
        let row = ROWS.iter().find(|r| r.id == "16").expect("row 16");
        assert_eq!(
            row.coverage,
            Coverage::PolicyOnly {
                test: "matrix_row16_sleep_wake.rs",
                wiring_pr: None,
                absent_call_site: "NSWorkspaceWillSleepNotification",
            }
        );
    }

    /// YV132's three rows are published as decisions with no caller, and each
    /// names the symbol whose appearance promotes it. Stated as a test so that
    /// wiring diarization into a meeting cannot land without this file changing
    /// — the same device rows `5b`, 16 and `17b` have carried since YV99.
    #[test]
    fn the_three_yap23_rows_admit_that_no_meeting_is_ever_diarized() {
        for (id, test, call_site) in [
            (
                "7",
                "matrix_row7_diarize_sidecar_wedge.rs",
                "diarize_sidecar_degrade",
            ),
            // Row 8's absent call site is the GATE, not the degrade. YV126
            // gave `cluster_sanity` a caller (`diarize::rank_and_floor` applies
            // it, which is how the table and the code stopped giving two
            // answers to one input); what is still absent from the app is
            // `rank_and_floor` itself, because no meeting is ever diarized.
            (
                "8",
                "matrix_row8_diarize_garbage_clusters.rs",
                "rank_and_floor",
            ),
            (
                "13",
                "matrix_row13_diarize_model_missing.rs",
                "speaker_detection_gate",
            ),
        ] {
            let row = ROWS.iter().find(|r| r.id == id).unwrap_or_else(|| {
                panic!("row {id} left the table; yap23's failures did not stop existing")
            });
            assert_eq!(
                row.coverage,
                Coverage::PolicyOnly {
                    test,
                    wiring_pr: None,
                    absent_call_site: call_site,
                },
                "row {id}"
            );
            let cell = row.coverage.cell();
            assert!(cell.contains("NOT WIRED"), "{cell}");
            assert!(cell.contains(call_site), "{cell}");
        }
    }

    /// Row 7's published sentence and row 7's code agree on the one thing that
    /// sentence promises: the meeting still completes.
    #[test]
    fn the_row_7_cell_and_the_row_7_policy_agree_that_the_meeting_completes() {
        let row = ROWS.iter().find(|r| r.id == "7").expect("row 7");
        assert!(
            row.behavior.contains(DIARIZATION_FAILED),
            "row 7 publishes a marker it does not name: {}",
            row.behavior
        );
        let labels = diarize_sidecar_degrade(&DiarizeError::Deadline);
        assert_eq!(labels.marker(), Some(DIARIZATION_FAILED));
        assert_eq!(
            labels.meeting_state_after(MeetingState::Complete),
            MeetingState::Complete,
            "the published row says `complete`, never `failed`"
        );
    }

    /// Row 17 is split, and the split is the honest reading of what merged: the
    /// watchdog stops a meeting at three hours, and nothing starts the meeting
    /// that carries on. One cell claiming both would have to average them.
    #[test]
    fn row_17_ships_the_cap_and_admits_it_does_not_ship_the_continuation() {
        let cap = ROWS.iter().find(|r| r.id == "17").expect("row 17");
        assert_eq!(
            cap.coverage,
            Coverage::Test {
                test: "matrix_row17_meeting_cap.rs",
                subject: "watchdog_tick",
                subject_module: "meeting.rs",
            }
        );

        let carry_on = ROWS.iter().find(|r| r.id == "17b").expect("row 17b");
        assert_eq!(
            carry_on.coverage,
            Coverage::PolicyOnly {
                test: "matrix_row17_meeting_cap.rs",
                wiring_pr: None,
                absent_call_site: "continuation_title",
            }
        );
    }
}
