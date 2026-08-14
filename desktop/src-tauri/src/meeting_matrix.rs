//! YV99 — the 22-A error-handling matrix, as data and as policy.
//!
//! The plan's §6 matrix is seventeen rows of prose. Prose does not fail a build.
//! This module turns the six rows 22-A actually owns (4, 5, 6, 15, 16, 17) plus
//! the two rows finding #3 added (`3a` user quits mid-processing, `3b` an ASR
//! chunk exceeds `TRANSCRIBE_TIMEOUT`) into two things a merge queue can read:
//!
//!   1. **[`ROWS`] — the matrix itself, as a const table**, each row carrying
//!      the name of the test that covers it. `tests/matrix_coverage.rs` walks
//!      that table against the filesystem, so "every row has a passing named
//!      test or a documented manual repro" stops being a claim in a PR body and
//!      becomes something CI checks. `docs/yap22a-error-matrix.md` is rendered
//!      from the same table ([`render_markdown`]) and asserted against it, so
//!      the document cannot drift away from the code.
//!   2. **The pure policy for the rows whose *decision* YV99 owns** — row 5's
//!      backpressure accounting and the honest quality note it produces, row
//!      16's sleep/wake state machine, and row 17's continuation-title rule.
//!
//! A row is a *cell*, not a failure: where one failure's two halves have
//! different truth values, it gets two cells. Row 5 is the case — the bounded
//! queue that keeps the disk off the audio callback is wired into `record.rs`
//! and shipping, while the quality note that would tell the user what the drops
//! cost has no caller anywhere. One cell covering both would have to pick a
//! truth value, and either choice is a lie about the other half. So the failure
//! is published as `5` (shipping) and `5b` (policy only).
//!
//! Everything here is pure: no clock, no disk, no CoreAudio, no Tauri handle.
//! That is the point. Sleep/wake, a disk falling behind, and a three-hour cap
//! are all things that are miserable to reproduce by hand and trivial to
//! reproduce as a sequence of events fed to a state machine — the same shape
//! `sysaudio::restore_plan` and `input_format`'s handler already use in this
//! codebase. The capture session (YV91, PR #108) supplies the events and
//! performs the actions; this module decides what the action *is*.
//!
//! **What this module deliberately does NOT do.** It does not open a journal,
//! take a power assertion, register `NSWorkspaceWillSleepNotification`, or stop
//! a recording. Those call sites live in the capture session. A policy with no
//! caller is a policy that is not in effect, which is why [`ROWS`] never marks
//! such a row [`Coverage::Test`]: a row whose mechanism lands elsewhere is
//! [`Coverage::LandsWith`], and a row whose *decision* is tested here while its
//! *call site* does not exist anywhere is [`Coverage::PolicyOnly`]. Both are
//! tripwired in `tests/matrix_coverage.rs`, in opposite directions, so neither
//! can quietly rot into an implied "the app already does this".
//!
//! **And it does not re-declare a threshold that ships somewhere else.** The
//! 3 h cap and its 2 h 45 m warning are `meeting::MEETING_HARD_CAP` /
//! `meeting::MEETING_CAP_WARN_AT` in PR #108, enforced by
//! `meeting::watchdog_tick` — the code that actually runs. A second copy here
//! would be two numbers with no compile-time link between them: change the
//! shipping one and this module's tests stay green while the published matrix
//! row goes quietly wrong. So row 17 keeps only the half #108 does *not* build
//! — [`continuation_title`] — and `tests/matrix_row17_meeting_cap.rs` fails the
//! build if a cap threshold is ever re-declared here.

// ── The matrix, as data ─────────────────────────────────────────────────────

/// How one matrix row is covered. There are exactly four honest answers, and
/// "we thought about it" is not one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    /// End to end: a named integration test that exists in
    /// `desktop/src-tauri/tests/` on this branch, runs in CI, and drives code
    /// that is **wired into the shipping app**. The string is the file name.
    ///
    /// This is the strongest cell in the table and it is the one a reader
    /// interprets as "done", so it is deliberately the hardest to claim.
    Test(&'static str),
    /// No pure test is possible (it needs two processes, or a lid). The string
    /// is a repo-relative path to the written repro, which a human follows and
    /// records the result of in the PR.
    Manual(&'static str),
    /// The mechanism — and its test — belong to another item that is not merged
    /// yet. Carries the owning PR and the test file that arrives with it.
    ///
    /// This is a tripwire, not a note: `tests/matrix_coverage.rs` fails if the
    /// named file ever appears while the row still says `LandsWith`, so the day
    /// PR #108 merges, the matrix has to be updated to say so. A comment reading
    /// "depends on #108" is a thing a merge queue reads zero times. (That
    /// tripwire is the ignored merge-checklist test, not a CI gate — see
    /// `tests/matrix_coverage.rs`, which explains why a tripwire another open PR
    /// will trip must not be allowed to redden *that* PR's build.)
    LandsWith {
        pr: &'static str,
        /// The name the test must carry once it lands. Always `matrix_`-prefixed:
        /// the phase's acceptance is a `matrix_*` sweep, so a row whose test is
        /// named anything else is a row the acceptance command can never reach.
        /// [`REQUIRED_TEST_PREFIX`] is that rule, and a test enforces it.
        test: &'static str,
        /// What the owning PR calls that file **today**, when that is not
        /// [`test`](Coverage::LandsWith::test). Renaming it is a merge condition
        /// on that PR, published in the rendered table rather than agreed in a
        /// review thread — and the tripwire watches for *either* name, so the
        /// rename cannot be skipped by merging under the old one.
        named_today: Option<&'static str>,
    },
    /// The *decision* is implemented and tested here as pure policy; the *call
    /// site that would put it in effect does not exist in any branch yet*.
    ///
    /// This variant exists because the alternative was a lie. Row 16's
    /// sleep/wake state machine is fully tested — and nothing registers
    /// `NSWorkspaceWillSleepNotification`, so closing a lid mid-meeting still
    /// does whatever it did before. Publishing that row as [`Coverage::Test`]
    /// tells a reader the app finalizes on sleep. It does not. Row `5b` is the
    /// same shape with a smaller blast radius: [`quality_note`] computes the
    /// sentence a meeting owes the user when the disk cost it audio, and no
    /// surface anywhere calls it.
    ///
    /// Like [`LandsWith`](Coverage::LandsWith) this is tripwired, but in the
    /// opposite direction: `tests/matrix_coverage.rs` fails the moment
    /// `absent_call_site` shows up as code anywhere in `src/` outside this
    /// module, because that is the day the row became a real `Test` row and
    /// must be flipped.
    PolicyOnly {
        /// The test file that drives the pure policy on this branch.
        test: &'static str,
        /// The PR known to be bringing the call site — or `None` when **no open
        /// PR owns it at all**, which is the truth for row 16 and is worth
        /// saying out loud instead of leaving a reader to assume someone has it.
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
        coverage: Coverage::LandsWith {
            pr: "#108 (YV91)",
            test: "matrix_row4_disk_preflight.rs",
            named_today: None,
        },
    },
    MatrixRow {
        id: "5",
        failure: "Journal write falls behind",
        behavior: "Bounded queue, `try_send`, drops counted, the audio callback never blocks",
        coverage: Coverage::Test("matrix_row5_journal_backpressure.rs"),
    },
    MatrixRow {
        id: "5b",
        failure: "Journal write falls behind — and the meeting never says what it cost",
        behavior: "`dropped > 0` becomes an honest quality note on the meeting detail: a real number of blocks, the seconds they were worth, and that they are missing",
        // NOT `Test`. `quality_note` computes that sentence and has zero callers
        // — no meeting detail renders it, and this branch changes no UI. The
        // never-block half above IS wired (`record.rs`'s `JournalQueue::offer`),
        // which is why the failure is published as two cells rather than one
        // averaged claim.
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
        coverage: Coverage::LandsWith {
            pr: "#108 (YV91)",
            test: "matrix_row6_orphan_recovery.rs",
            named_today: None,
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
        // observer that would feed it is in no branch — #108 mentions
        // `NSWorkspaceWillSleepNotification` in a comment in `power.rs` and
        // registers nothing. Until someone writes that call site, a lid close
        // mid-meeting still does whatever it did before, and this row must not
        // read as though it does not.
        coverage: Coverage::PolicyOnly {
            test: "matrix_row16_sleep_wake.rs",
            wiring_pr: None,
            absent_call_site: "NSWorkspaceWillSleepNotification",
        },
    },
    MatrixRow {
        id: "17",
        failure: "Meeting exceeds the 3 h cap",
        behavior: "Warn once at 2 h 45 m; hard-stop once at 3 h, cleanly finalized, with a continuation meeting linked to the first",
        // Split honestly. The cap itself — thresholds, watchdog, one-shot warn —
        // is `meeting::watchdog_tick` in #108 and is NOT re-implemented here;
        // `matrix_row17_meeting_cap.rs` fails if it ever is. What this branch
        // owns and tests is the continuation-title rule #108 does not build.
        coverage: Coverage::PolicyOnly {
            test: "matrix_row17_meeting_cap.rs",
            wiring_pr: Some("#108 (YV91)"),
            absent_call_site: "MEETING_HARD_CAP",
        },
    },
    MatrixRow {
        id: "3a",
        failure: "User quits mid-processing (finding #3)",
        behavior: "Resume from `processed_through_seconds`, never re-decode from zero",
        // #110 carries this test as `meeting_transcription_resume.rs`. Under
        // that name the phase's acceptance sweep — a `matrix_*` run — never
        // reaches it, so the rename to the backlog's name is a merge condition
        // on #110, stated here and rendered into the published table.
        coverage: Coverage::LandsWith {
            pr: "#110 (YV93)",
            test: "matrix_new_quit_mid_processing.rs",
            named_today: Some("meeting_transcription_resume.rs"),
        },
    },
    MatrixRow {
        id: "3b",
        failure: "An ASR chunk exceeds `TRANSCRIBE_TIMEOUT` (finding #3)",
        behavior: "That chunk gets `text=''` + `asr_failed`; its neighbours and the rest of the meeting are unaffected",
        coverage: Coverage::LandsWith {
            pr: "#110 (YV93)",
            test: "matrix_new_asr_chunk_timeout.rs",
            named_today: Some("meeting_chunk_timeout_isolation.rs"),
        },
    },
];

impl Coverage {
    /// The cell as it appears in the rendered table.
    pub fn cell(self) -> String {
        match self {
            Coverage::Test(file) => format!("`cargo test --test {}`", stem(file)),
            Coverage::Manual(doc) => format!("Manual repro — `{doc}`"),
            Coverage::LandsWith {
                pr,
                test,
                named_today: None,
            } => {
                format!("Lands with {pr} — `{}`", stem(test))
            }
            Coverage::LandsWith {
                pr,
                test,
                named_today: Some(today),
            } => format!(
                "Lands with {pr} — `{}`, which that PR must **rename from `{}`** on merge so the acceptance sweep reaches it",
                stem(test),
                stem(today)
            ),
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
                "**Policy only, NOT WIRED** — `cargo test --test {}` covers the decision; **no open PR wires `{absent_call_site}`**, so the app does not do this yet",
                stem(test)
            ),
        }
    }

    /// The test file this cell names, if it names one. `Manual` rows name a
    /// document instead and are the one honest way for a row to have no test.
    pub fn test_file(self) -> Option<&'static str> {
        match self {
            Coverage::Test(test)
            | Coverage::LandsWith { test, .. }
            | Coverage::PolicyOnly { test, .. } => Some(test),
            Coverage::Manual(_) => None,
        }
    }

    /// Every file name that would prove this row's owning PR has merged — the
    /// required name and, while the rename is still outstanding, the name that
    /// PR uses today. The tripwire watches both so the rename cannot be dodged
    /// by merging under the old name.
    pub fn landing_file_names(self) -> Vec<&'static str> {
        match self {
            Coverage::LandsWith {
                test,
                named_today: Some(today),
                ..
            } => vec![test, today],
            Coverage::LandsWith { test, .. } => vec![test],
            _ => Vec::new(),
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

// ── Row 5 · journal backpressure, and the note it owes the user ─────────────

/// The accounting half of the never-block rule, split out from the disk so it
/// can be driven to overflow in a unit test without a wedged filesystem.
///
/// `record.rs`'s `JournalQueue` is the shipping implementation for a dictation
/// take: one `try_send` into a bounded `sync_channel`, a dropped counter, and
/// nothing else on the audio path. This type is the same contract expressed as
/// pure state so the invariants — *in-flight never exceeds the depth*, *every
/// offer is decided immediately*, *nothing is silently uncounted* — are
/// falsifiable. `tests/matrix_row5_journal_backpressure.rs` checks both: this
/// state machine behaviourally, and `record.rs` by reading its source for a
/// blocking `send` that would put the disk back on the callback.
///
/// A meeting makes this row matter far more than a dictation did. YV91 stops
/// retaining the session in RAM, so for a meeting the journal *is* the
/// recording: a dropped write is not a weakened crash-recovery copy, it is
/// missing audio. Hence [`quality_note`] — which is row `5b`, and which nothing
/// calls yet.
#[derive(Debug, Clone)]
pub struct JournalBackpressure {
    depth: usize,
    in_flight: usize,
    offered: u64,
    accepted: u64,
    dropped: u64,
}

/// What one offer to the journal writer did. There is no third outcome and, in
/// particular, no "waited".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offer {
    Accepted,
    Dropped,
}

impl JournalBackpressure {
    /// `depth` mirrors `record::JOURNAL_QUEUE_DEPTH`. Zero is clamped to one:
    /// a queue with no room at all would drop every write, which is a config
    /// bug that should degrade, not divide by zero.
    pub fn new(depth: usize) -> Self {
        Self {
            depth: depth.max(1),
            in_flight: 0,
            offered: 0,
            accepted: 0,
            dropped: 0,
        }
    }

    /// Offer one write. Returns immediately in every case — this is the whole
    /// invariant of row 5, and it is enforced here by there being no branch
    /// that can wait.
    pub fn offer(&mut self) -> Offer {
        self.offered += 1;
        if self.in_flight < self.depth {
            self.in_flight += 1;
            self.accepted += 1;
            Offer::Accepted
        } else {
            self.dropped += 1;
            Offer::Dropped
        }
    }

    /// The writer thread got `n` writes onto the disk. Returns how many it
    /// actually took (saturating, so a buggy caller cannot underflow the queue).
    pub fn drained(&mut self, n: usize) -> usize {
        let took = n.min(self.in_flight);
        self.in_flight -= took;
        took
    }

    pub fn depth(&self) -> usize {
        self.depth
    }
    pub fn in_flight(&self) -> usize {
        self.in_flight
    }
    pub fn offered(&self) -> u64 {
        self.offered
    }
    pub fn accepted(&self) -> u64 {
        self.accepted
    }
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
    /// True once the disk has cost the recording anything at all.
    pub fn fell_behind(&self) -> bool {
        self.dropped > 0
    }
}

/// The sentence the meeting detail *would* show when the journal dropped
/// writes — row `5b`, published as **not wired**, because this function has no
/// callers. YV94 shipped the meeting detail before there was a capture path to
/// feed it a drop count, and this branch changes no UI, so the note is computed
/// and rendered nowhere. `tests/matrix_coverage.rs` fails the day
/// `quality_note` appears as code anywhere else in `src/`, which is the day
/// row `5b` becomes a real `Test` row.
///
/// `None` when nothing was dropped — the overwhelmingly common case, and a
/// meeting detail that says "0 blocks dropped" is noise that teaches the user to
/// ignore the line on the day it matters.
///
/// The rule the plan asks for is *honest*, which here means: say a real number,
/// say what it cost in seconds, and do not dress it up. It also means not
/// claiming precision we do not have — the seconds figure is derived from the
/// queue's own frame size, so it is an estimate and says so.
pub fn quality_note(
    dropped_writes: u64,
    frames_per_write: usize,
    sample_rate: u32,
) -> Option<String> {
    if dropped_writes == 0 {
        return None;
    }
    let blocks = if dropped_writes == 1 {
        "1 audio block".to_string()
    } else {
        format!("{dropped_writes} audio blocks")
    };
    let seconds = if sample_rate == 0 {
        0.0
    } else {
        (dropped_writes as f64) * (frames_per_write as f64) / (sample_rate as f64)
    };
    let amount = if seconds < 0.1 {
        "under a tenth of a second".to_string()
    } else {
        format!("about {seconds:.1} s")
    };
    Some(format!(
        "The disk fell behind while recording: {blocks} ({amount}) were dropped and are missing from this meeting. Everything else recorded normally."
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
// and `meeting::watchdog_tick` in PR #108 are the thresholds and the rule that
// actually run — the 60 s watchdog reads them, latches `cap_warned` in its own
// loop, and stops the session. An earlier revision of this module declared its
// own cap constants and its own warn/stop latch. Two copies of a number with no
// compile-time link between them is a trap with a specific shape: someone
// changes the cap in the module that ships, this module's test stays green, and
// `docs/yap22a-error-matrix.md` — which is published as the row's required
// behaviour — goes quietly wrong. That is the exact hazard this PR cited to
// justify NOT duplicating row 4's preflight, so it does not get to make an
// exception for row 17.
//
// What remains is the half #108 does not build: the title of the meeting that
// continues after the stop. `tests/matrix_row17_meeting_cap.rs` asserts this
// module still declares no threshold of its own, and `matrix_coverage.rs`
// tripwires the row so that when #108 lands, row 17's test is rewritten to
// import #108's constants and drive `watchdog_tick` directly.

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
            9,
            "22-A owns eight matrix failures, published as nine cells — row 5's \
             wired half and its unwired quality-note half"
        );
    }

    /// The rename #110 owes is a claim about that PR, so state it where a test
    /// can hold it rather than in a review comment.
    #[test]
    fn the_two_rows_whose_tests_must_be_renamed_are_110s() {
        let renames: Vec<(&str, &str, &str)> = ROWS
            .iter()
            .filter_map(|r| match r.coverage {
                Coverage::LandsWith {
                    pr,
                    named_today: Some(today),
                    ..
                } => Some((r.id, pr, today)),
                _ => None,
            })
            .collect();
        assert_eq!(
            renames,
            vec![
                ("3a", "#110 (YV93)", "meeting_transcription_resume.rs"),
                ("3b", "#110 (YV93)", "meeting_chunk_timeout_isolation.rs"),
            ]
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
    fn coverage_cells_name_the_runnable_thing() {
        assert_eq!(
            Coverage::Test("matrix_row5_journal_backpressure.rs").cell(),
            "`cargo test --test matrix_row5_journal_backpressure`"
        );
        assert_eq!(
            Coverage::LandsWith {
                pr: "#108 (YV91)",
                test: "matrix_row4_disk_preflight.rs",
                named_today: None,
            }
            .cell(),
            "Lands with #108 (YV91) — `matrix_row4_disk_preflight`"
        );
        assert_eq!(
            Coverage::Manual("docs/yap22a-phase-demo.md").cell(),
            "Manual repro — `docs/yap22a-phase-demo.md`"
        );

        // A row still carrying its old name publishes the rename as a merge
        // condition, in the table, rather than in a review thread.
        let renamed = Coverage::LandsWith {
            pr: "#110 (YV93)",
            test: "matrix_new_quit_mid_processing.rs",
            named_today: Some("meeting_transcription_resume.rs"),
        }
        .cell();
        assert!(
            renamed.contains("rename from `meeting_transcription_resume`"),
            "{renamed}"
        );
        assert!(
            renamed.contains("matrix_new_quit_mid_processing"),
            "{renamed}"
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
        assert!(unowned.contains("no open PR wires"), "{unowned}");
        assert!(
            unowned.contains("NSWorkspaceWillSleepNotification"),
            "{unowned}"
        );

        let owned = Coverage::PolicyOnly {
            test: "matrix_row17_meeting_cap.rs",
            wiring_pr: Some("#108 (YV91)"),
            absent_call_site: "MEETING_HARD_CAP",
        }
        .cell();
        assert!(owned.contains("Policy only"), "{owned}");
        assert!(owned.contains("#108 (YV91)"), "{owned}");
        // And it still names something runnable, so the row is not a shrug.
        assert!(
            owned.contains("cargo test --test matrix_row17_meeting_cap"),
            "{owned}"
        );
    }

    /// Rows `5b`, 16 and 17 are the rows whose enforcement is absent. If a
    /// future edit promotes any of them to `Test`, that edit is claiming the
    /// call site now exists — `tests/matrix_coverage.rs` is where that claim
    /// gets checked, and this assertion is the reminder that flipping it is a
    /// deliberate act.
    #[test]
    fn the_unwired_rows_are_exactly_5b_16_and_17() {
        let unwired: Vec<&str> = ROWS
            .iter()
            .filter(|r| matches!(r.coverage, Coverage::PolicyOnly { .. }))
            .map(|r| r.id)
            .collect();
        assert_eq!(unwired, vec!["5b", "16", "17"]);
    }

    /// The rows the shipping app actually performs, spelled out, because this
    /// is the number every reader of the table is really asking for. One cell
    /// is `Test` — row 5's bounded queue, which `record.rs` runs — and one is a
    /// manual repro. Everything else is a mechanism in another PR or a policy
    /// nobody calls.
    #[test]
    fn exactly_one_row_claims_wired_shipping_behaviour() {
        let wired: Vec<&str> = ROWS
            .iter()
            .filter(|r| matches!(r.coverage, Coverage::Test(_)))
            .map(|r| r.id)
            .collect();
        assert_eq!(wired, vec!["5"]);

        let manual: Vec<&str> = ROWS
            .iter()
            .filter(|r| matches!(r.coverage, Coverage::Manual(_)))
            .map(|r| r.id)
            .collect();
        assert_eq!(manual, vec!["15"]);
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
}
