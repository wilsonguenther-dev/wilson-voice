//! YV99 · error-matrix row 5 — "journal write falls behind".
//!
//! Required behaviour: bounded queue, `try_send`, drops counted, the audio
//! callback NEVER blocks, and `dropped > 0` surfaces as an honest quality note
//! on the meeting rather than disappearing into a log line nobody reads.
//!
//! **This file drives `meeting::MeetingJournal`, and which journal it drives is
//! the whole point.** An earlier revision of this row was green against
//! `record.rs` — the journal a five-second *dictation* take uses — and against
//! a pure model of a bounded queue that lived beside the matrix. Both are the
//! same shape as the meeting journal (a bounded `sync_channel`, one `try_send`,
//! a `dropped` counter), so every assertion passed and none of them was
//! evidence for this row: a meeting is the case where the queue matters,
//! because YV91 stops retaining the session in RAM and the journal *is* the
//! recording. A dropped write in a dictation is a weakened crash-recovery copy;
//! a dropped write in a three-hour lecture is missing audio. So the row names
//! `MeetingJournal` as its subject in `meeting_matrix::ROWS`, and
//! `matrix_coverage::test_rows_drive_the_shipping_code_they_name` fails if this
//! file ever stops touching it.
//!
//! **The failure is produced, not simulated.** `MeetingJournal::start_with_depth`
//! plus `pause_handle` park the writer thread so a depth-1 queue fills on
//! demand — the same seam `capture_journal_recovery.rs` uses — which means the
//! rejection is the real `append` path refusing a real chunk, not a hand-written
//! record claiming it did. The park is self-limiting (`WRITER_PAUSE_CAP`), so a
//! forgotten un-park costs seconds rather than the build.
//!
//! **The two halves of the row have different truth values, so they are two
//! rows.** The never-block half is wired and is what everything above tests.
//! The quality note is not: `quality_note` computes the sentence and nothing
//! anywhere calls it, so `ROWS` publishes row `5` as `Test` and row `5b` as
//! `PolicyOnly, NOT WIRED`. One averaged cell would have had to pick a truth
//! value and lie about the other half.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use wilson_voice_lib::meeting::{MeetingJournal, MeetingState, TARGET_RATE};
use wilson_voice_lib::meeting_matrix::{quality_note, Coverage, ROWS};

/// 100 ms of 16 kHz mono — one plausible capture callback's worth.
const BLOCK: usize = 1_600;

/// yap-1108-timing-open-m-budgets — "append never waits" is now COUNTED, not
/// timed (it replaces the old 500 ms per-append / 2 s total wall bounds).
///
/// With the writer parked, a full bounded queue can take a blocking `send` only
/// after the writer RECEIVES something, and a parked writer receives at most
/// the one block its `recv_timeout` was already sitting on when the park was
/// raised. So: if every `append` returned while the writer's receive count
/// stayed within that one block, no `append` waited for the writer — on any
/// machine, at any load. A blocking hand-off can only come back after the
/// park cap (`WRITER_PAUSE_CAP`) lets the writer drain, which moves the count
/// and fails here on the first offer that blocked.
const WRITER_RECEIVES_WHILE_PARKED: u64 = 1;

/// A HANG guard, never a measurement: how long the recovery half waits for the
/// unparked writer to drain the queue before calling it stuck.
const DRAIN_DEADLINE: Duration = Duration::from_secs(30);

/// Asserts no `append` so far had to wait for the parked writer (see
/// [`WRITER_RECEIVES_WHILE_PARKED`]).
fn assert_no_wait_entered(journal: &MeetingJournal, i: usize) {
    let received = journal.writer_audio_received();
    assert!(
        received <= WRITER_RECEIVES_WHILE_PARKED,
        "append #{i} came back only after the parked writer took {received} blocks: the \
         journal hand-off is blocking on the disk, which on the real path is the cpal \
         input callback parking mid-meeting"
    );
}

/// Waits (hang-guarded) until the writer has taken `blocks` audio blocks off
/// the queue.
fn wait_for_writer(journal: &MeetingJournal, blocks: u64) {
    let started = Instant::now();
    while journal.writer_audio_received() < blocks {
        assert!(
            started.elapsed() < DRAIN_DEADLINE,
            "the unparked writer took {} of {blocks} blocks in {DRAIN_DEADLINE:?} — it is stuck",
            journal.writer_audio_received()
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn tmpdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "yap-yv99-row5-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&dir).expect("tmpdir");
    dir
}

fn block(seed: usize) -> Vec<f32> {
    (0..BLOCK)
        .map(|i| (((seed + i) % 32) as f32 / 64.0) - 0.25)
        .collect()
}

fn src(file: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(file);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The audio callback hands the journal a chunk per buffer whatever the disk is
/// doing. With the writer parked, the bounded queue must start refusing —
/// immediately, every time, and without the callback ever waiting on it.
#[test]
fn a_wedged_writer_makes_the_meeting_journal_drop_and_never_wait() {
    let dir = tmpdir("wedged");
    let journal = MeetingJournal::start_with_depth(&dir, 1, 1).expect("journal opens");
    let pause = journal.pause_handle();
    pause.store(true, Ordering::SeqCst);

    let mut accepted_samples = 0u64;
    let mut rejections = 0usize;

    for i in 0..200 {
        let chunk = block(i);
        let took = journal.append(0, &chunk);

        // Checked per call, not after the loop: a blocking hand-off parks for
        // `WRITER_PAUSE_CAP` on EVERY offer, so a version of this that only
        // looked at the total would take a quarter of an hour to say so — and a
        // test that hangs is a test somebody eventually deletes.
        assert_no_wait_entered(&journal, i);
        assert!(
            took == chunk.len() || took == 0,
            "append must accept the whole chunk or none of it, got {took} of {}",
            chunk.len()
        );
        if took == 0 {
            rejections += 1;
        } else {
            accepted_samples += took as u64;
        }
    }

    assert!(
        rejections > 0,
        "a depth-1 queue with a parked writer accepted all 200 chunks — the journal is not \
         bounded, or the test seam stopped working, and either way row 5 is unproven"
    );
    // The whole run, not just each call: 200 offers against a wedged writer,
    // and the writer still took no more than the one block it was sitting on.
    assert_no_wait_entered(&journal, 199);

    // Nothing is uncounted: every sample the journal refused is in the counter,
    // and the counter counts nothing else. This is the invariant the gap
    // detector reads — `spilled` advances only by what `append` returned, so a
    // drop that is not counted here is a hole the finalize will never splice.
    assert_eq!(
        journal.dropped_samples(),
        rejections as u64 * BLOCK as u64,
        "dropped samples must equal exactly the refused chunks"
    );
    assert_eq!(
        journal.dropped_samples() + accepted_samples,
        200 * BLOCK as u64,
        "every offered sample must be accounted for as accepted or dropped"
    );

    pause.store(false, Ordering::SeqCst);
    drop(journal);
}

/// Recovery: once the writer catches up, the queue takes writes again. A
/// permanently-poisoned queue after one slow moment would silently end
/// journalling for the rest of a three-hour meeting — the same recording the
/// drop counter would then describe as almost intact.
#[test]
fn the_meeting_journal_takes_writes_again_once_the_writer_catches_up() {
    let dir = tmpdir("recovers");
    let journal = MeetingJournal::start_with_depth(&dir, 1, 1).expect("journal opens");
    let pause = journal.pause_handle();

    pause.store(true, Ordering::SeqCst);
    let mut accepted_blocks = 0u64;
    for i in 0..50 {
        if journal.append(0, &block(i)) > 0 {
            accepted_blocks += 1;
        }
        // Bailed on inside the loop, for the reason the other test explains: a
        // blocking hand-off parks per offer, so checking only the total turns a
        // failure into a four-minute wait.
        assert_no_wait_entered(&journal, i);
    }
    let dropped_during = journal.dropped_samples();
    assert!(dropped_during > 0, "the parked writer refused nothing");

    pause.store(false, Ordering::SeqCst);

    // yap-1108-timing-open-m-budgets — paced on the WRITER, not on a 5 ms
    // sleep: each block is offered only once the writer has taken everything
    // accepted so far, which is the "a drained queue keeps up" condition the
    // old pacing approximated (and which a starved runner could miss, offering
    // twenty blocks into a queue nobody had drained yet).
    let mut accepted_after = 0u64;
    for i in 50..70 {
        wait_for_writer(&journal, accepted_blocks);
        let took = journal.append(0, &block(i)) as u64;
        if took > 0 {
            accepted_blocks += 1;
        }
        accepted_after += took;
    }

    assert!(
        accepted_after > 0,
        "the journal never accepted another sample after an overflow — one disk hiccup would \
         end journalling for the rest of the meeting"
    );
    assert_eq!(
        journal.dropped_samples() - dropped_during,
        (20 * BLOCK as u64) - accepted_after,
        "post-recovery drops must still be counted exactly"
    );

    let finalized = journal
        .finalize(MeetingState::Partial)
        .expect("the meeting still finalizes after the queue overflowed");
    assert!(
        finalized.tracks.iter().all(|t| t.exists()),
        "a meeting that lost writes must still produce its track files"
    );
}

/// A disk hiccup of a few dozen callbacks is what the production depth
/// (`MEETING_QUEUE_DEPTH`) exists to absorb: at the real depth, an unparked
/// writer, a real meeting's pacing — it must cost nothing at all.
#[test]
fn a_short_hiccup_costs_the_meeting_nothing() {
    let dir = tmpdir("hiccup");
    let journal = MeetingJournal::start(&dir, 1).expect("journal opens");

    let mut accepted = 0u64;
    for i in 0..40 {
        accepted += journal.append(0, &block(i)) as u64;
    }

    assert_eq!(accepted, 40 * BLOCK as u64);
    assert_eq!(
        journal.dropped_samples(),
        0,
        "40 chunks into the production-depth queue dropped something"
    );
    // …so the meeting owes the user nothing, which row `5b`'s note has to say
    // by saying nothing at all.
    assert_eq!(
        quality_note(journal.dropped_samples(), TARGET_RATE),
        None,
        "a clean meeting must not be given a quality note"
    );
    drop(journal);
}

/// The rule that actually protects the audio callback, checked in the shipping
/// code: `MeetingJournal::append` hands writes over with `try_send` and nothing
/// else.
///
/// The behavioural tests above prove today's code does not block. This is the
/// guard for the change that would make it block: `append` is the ONLY place a
/// meeting's audio reaches the channel, and turning its `try_send` into a
/// blocking `send` is exactly what a well-meaning fix for "we are dropping
/// writes" looks like. It would compile, keep every other test green, and put a
/// cold-disk `write()` back on the cpal input callback.
#[test]
fn the_meeting_journal_hand_off_is_try_send_and_the_drop_is_counted() {
    let meeting = src("meeting.rs");

    let append = meeting
        .split_once("pub fn append(&self, track: usize, samples: &[f32]) -> usize")
        .expect("meeting.rs must still have MeetingJournal::append")
        .1;
    let body = &append[..append.find("\n    }").expect("append body ends")];

    assert!(
        body.contains("try_send"),
        "row 5: the meeting journal hand-off must be try_send, found:\n{body}"
    );
    assert!(
        !body.contains(".send("),
        "row 5: a blocking send on the meeting journal path would park the audio callback:\n{body}"
    );
    assert!(
        body.contains("self.dropped") && body.contains("fetch_add"),
        "row 5: a dropped write must be counted, not swallowed:\n{body}"
    );

    // The queue is bounded at its construction site, too — an unbounded channel
    // would make the drop counter unreachable and the memory bound a fiction.
    assert!(
        meeting.contains("mpsc::sync_channel::<JournalMsg>(depth.max(1))"),
        "row 5: the meeting journal channel must stay bounded"
    );
    assert!(
        !meeting.contains("mpsc::channel::<JournalMsg>"),
        "row 5: an unbounded meeting journal channel defeats the whole row"
    );
}

/// The dictation journal next door, held to the same rule.
///
/// This is NOT row 5's evidence — the row is about a meeting, and confusing the
/// two is the defect this file was rewritten to fix. It is here because the two
/// journals share a failure mode and one line of it is cheap to hold: YV92's
/// review already caught a blocking call sneaking onto `mark_device_change`.
#[test]
fn the_dictation_journal_keeps_the_same_never_block_rule() {
    let record = src("record.rs");

    let offer = record
        .split_once("fn offer(&self, write: JournalWrite)")
        .expect("record.rs must still have JournalQueue::offer")
        .1;
    let body = &offer[..offer.find("\n    }").expect("offer body ends")];

    assert!(body.contains("try_send"), "{body}");
    assert!(!body.contains(".send("), "{body}");
    assert!(body.contains("dropped.fetch_add"), "{body}");
}

/// Row `5b` · the honest quality note: a real number, what it cost in seconds,
/// no dressing up — and nothing at all on the overwhelmingly common clean
/// meeting.
#[test]
fn dropped_writes_become_an_honest_quality_note() {
    assert_eq!(
        quality_note(0, TARGET_RATE),
        None,
        "a clean meeting says nothing"
    );

    // The argument is exactly what `MeetingJournal::dropped_samples` returns —
    // 51 200 samples at 16 kHz is 3.2 s of missing audio.
    let note = quality_note(51_200, 16_000).expect("dropped writes must be surfaced");
    assert!(note.contains("3.2 s"), "{note}");
    assert!(
        note.contains("missing from this meeting"),
        "the note must say what it cost, not just that something happened: {note}"
    );

    // A sub-100 ms loss is not dressed up as 0.0 s.
    let one = quality_note(512, 16_000).expect("one dropped block still counts");
    assert!(one.contains("under a tenth of a second"), "{one}");

    // A nonsense sample rate must not panic or print `inf`.
    let odd = quality_note(3, 0).expect("still a note");
    assert!(!odd.contains("inf") && !odd.contains("NaN"), "{odd}");
}

/// …and the note reaches nobody, which the table has to say rather than imply.
///
/// The test above proves `quality_note` returns the right sentence. It does not
/// — cannot — prove any surface shows it, because none does. Row `5b` is
/// published as `PolicyOnly, NOT WIRED` for that reason, and this asserts the
/// two claims stay attached to each other: the day the meeting detail renders
/// the note, `matrix_coverage`'s unowned-policy tripwire fires and this row is
/// promoted with it.
#[test]
fn the_quality_note_half_is_published_as_unwired() {
    let row = ROWS.iter().find(|r| r.id == "5b").expect("row 5b");
    assert_eq!(
        row.coverage,
        Coverage::PolicyOnly {
            test: "matrix_row5_journal_backpressure.rs",
            wiring_pr: None,
            absent_call_site: "quality_note",
        }
    );

    let wired = ROWS.iter().find(|r| r.id == "5").expect("row 5");
    assert_eq!(
        wired.coverage,
        Coverage::Test {
            test: "matrix_row5_journal_backpressure.rs",
            subject: "MeetingJournal",
            subject_module: "meeting.rs",
        },
        "the never-block half IS wired — the meeting journal runs it — and must not be \
         downgraded along with the note, nor re-aimed at the dictation journal"
    );
}
