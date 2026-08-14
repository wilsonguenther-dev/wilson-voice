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
    /// the row became a real `Test` row and must be flipped.
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

/// The eight failures of the plan's §6 matrix that belong to 22-A, as nine
/// cells (row 5's two halves have different truth values — see the module docs).
///
/// Rows 1, 2, 3, 7–14 are NOT here and that is deliberate: 1/2/14 are system
/// -audio process taps (22-B), 3 is a mic-denied fallback that needs the tap to
/// fall back *to*, 7/8 are diarization (yap23), 10 is the summarizer's own
/// degrade ladder (YV97 owns it), 11 is calendar (yap24), 12/13 are the 14.4 and
/// model-missing gates that 22-A does not reach. Listing them here with no
/// mechanism behind them would make the matrix look more covered than it is.
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
            10,
            "22-A owns eight matrix failures, published as ten cells — two of \
             them (5 and 17) have a wired half and an unwired half, and one \
             cell each way is the only way to state both truthfully"
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
    fn the_unwired_rows_are_exactly_5b_16_and_17b() {
        let unwired: Vec<&str> = ROWS
            .iter()
            .filter(|r| matches!(r.coverage, Coverage::PolicyOnly { .. }))
            .map(|r| r.id)
            .collect();
        assert_eq!(unwired, vec!["5b", "16", "17b"]);
    }

    /// The rows the shipping app actually performs, spelled out, because this
    /// is the number every reader of the table is really asking for. Six cells
    /// are `Test` now that YV91 and YV93 have merged, one is a manual repro, and
    /// the remaining three are decisions nobody calls.
    #[test]
    fn the_wired_rows_are_the_ones_whose_mechanism_shipped() {
        let wired: Vec<&str> = ROWS
            .iter()
            .filter(|r| matches!(r.coverage, Coverage::Test { .. }))
            .map(|r| r.id)
            .collect();
        assert_eq!(wired, vec!["4", "5", "6", "17", "3a", "3b"]);

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
