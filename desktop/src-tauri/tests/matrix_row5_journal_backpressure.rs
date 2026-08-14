//! YV99 · error-matrix row 5 — "journal write falls behind".
//!
//! Required behaviour: bounded queue, `try_send`, drops counted, the audio
//! callback NEVER blocks, and `dropped > 0` surfaces as an honest quality note
//! on the meeting rather than disappearing into a log line nobody reads.
//!
//! **The two halves have different truth values, so they are two rows.** The
//! never-block half is wired: `record.rs`'s `JournalQueue::offer` is the
//! shipping code and it is what the source assertion at the bottom of this file
//! holds. The quality note is not: `quality_note` computes the sentence and
//! nothing anywhere calls it, and this branch changes no UI. So `ROWS`
//! publishes row `5` as `Test` and row `5b` as `PolicyOnly, NOT WIRED`. One
//! averaged cell would have had to pick a truth value and lie about the other
//! half — which is what an earlier revision did, publishing "`dropped > 0`
//! becomes an honest quality note on the meeting" behind a plain green
//! `cargo test` cell while nothing surfaced it.
//!
//! Two halves, because the row has two failure modes and they need different
//! instruments:
//!
//!   * **The accounting** is driven here as a state machine
//!     (`meeting_matrix::JournalBackpressure`): overflow it, starve it, drain it,
//!     and check that in-flight never exceeds the depth and that every single
//!     offer is accounted for as accepted or dropped. Nothing is uncounted.
//!   * **The never-block rule in the shipping code** cannot be observed from an
//!     integration test — the queue is private to `record.rs` and the only way
//!     to see it block is a wedged disk. So it is checked the way this repo
//!     already checks a rule it cannot execute (`crash.rs`'s
//!     `no_network_no_transcript_in_crash_rows`): by reading the source. A
//!     blocking `send(` on the journal path is what regresses this row, and it
//!     is exactly what a well-meaning refactor writes when it notices writes
//!     being dropped.

use std::fs;
use std::path::PathBuf;

use wilson_voice_lib::meeting_matrix::{quality_note, Coverage, JournalBackpressure, Offer, ROWS};

fn src(file: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(file);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The audio callback offers a write per buffer whatever the disk is doing. A
/// writer that never drains must therefore drop, immediately, forever — and must
/// not grow the queue past its bound while doing it.
#[test]
fn a_wedged_writer_drops_and_never_grows() {
    let depth = 64;
    let mut q = JournalBackpressure::new(depth);

    // 5 000 callbacks with a writer thread that never runs.
    for _ in 0..5_000 {
        let _ = q.offer();
        assert!(
            q.in_flight() <= depth,
            "queue grew past its bound: {} > {depth}",
            q.in_flight()
        );
    }

    assert_eq!(
        q.in_flight(),
        depth,
        "the bound is the ceiling, and it is hit"
    );
    assert_eq!(q.accepted(), depth as u64);
    assert_eq!(q.dropped(), 5_000 - depth as u64);
    assert_eq!(
        q.offered(),
        q.accepted() + q.dropped(),
        "every offer is accounted for as accepted or dropped — nothing is uncounted"
    );
    assert!(q.fell_behind());
}

/// A disk hiccup of tens of callbacks is what the depth exists to absorb: it
/// must cost nothing at all.
#[test]
fn a_short_hiccup_costs_nothing() {
    let mut q = JournalBackpressure::new(64);
    for _ in 0..40 {
        assert_eq!(q.offer(), Offer::Accepted);
    }
    q.drained(40);
    for _ in 0..40 {
        assert_eq!(q.offer(), Offer::Accepted);
    }
    assert_eq!(q.dropped(), 0);
    assert!(!q.fell_behind());
    assert_eq!(quality_note(q.dropped(), 512, 16_000), None);
}

/// Recovery: once the writer catches up, the queue takes writes again. A
/// permanently-poisoned queue after one slow moment would silently end
/// journalling for the rest of a three-hour meeting.
#[test]
fn draining_restores_capacity_after_an_overflow() {
    let mut q = JournalBackpressure::new(8);
    for _ in 0..32 {
        let _ = q.offer();
    }
    assert_eq!(q.dropped(), 24);

    assert_eq!(q.drained(8), 8);
    assert_eq!(q.in_flight(), 0);
    assert_eq!(q.offer(), Offer::Accepted, "the queue is usable again");

    // A drain larger than what is in flight is saturating, not a panic.
    assert_eq!(q.drained(1_000), 1);
    assert_eq!(q.in_flight(), 0);
}

/// Zero depth is a configuration bug, not a crash: it degrades to "drop
/// everything" and keeps the audio path alive.
#[test]
fn zero_depth_degrades_instead_of_dividing_by_zero() {
    let mut q = JournalBackpressure::new(0);
    assert_eq!(q.depth(), 1);
    assert_eq!(q.offer(), Offer::Accepted);
    assert_eq!(q.offer(), Offer::Dropped);
}

/// The honest quality note: a real number, what it cost in seconds, no dressing
/// up — and nothing at all on the overwhelmingly common clean meeting.
#[test]
fn dropped_writes_become_an_honest_quality_note() {
    assert_eq!(
        quality_note(0, 512, 16_000),
        None,
        "a clean meeting says nothing"
    );

    // 100 dropped 512-frame blocks at 16 kHz = 3.2 s of missing audio.
    let note = quality_note(100, 512, 16_000).expect("dropped writes must be surfaced");
    assert!(note.contains("100 audio blocks"), "{note}");
    assert!(note.contains("3.2 s"), "{note}");
    assert!(
        note.contains("missing from this meeting"),
        "the note must say what it cost, not just that something happened: {note}"
    );

    // Singular reads as English, and a sub-100 ms loss is not dressed up as 0.0 s.
    let one = quality_note(1, 512, 16_000).expect("one dropped write still counts");
    assert!(one.contains("1 audio block ("), "{one}");
    assert!(one.contains("under a tenth of a second"), "{one}");

    // A nonsense sample rate must not panic or print `inf`.
    let odd = quality_note(3, 512, 0).expect("still a note");
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
        Coverage::Test("matrix_row5_journal_backpressure.rs"),
        "the never-block half IS wired — record.rs runs it — and must not be \
         downgraded along with the note"
    );
}

/// The rule that actually protects the audio callback, checked in the shipping
/// code: the journal hands writes over with `try_send` and nothing else.
///
/// `JournalQueue::offer` is the ONLY place a write reaches the channel, and it
/// is the line a future "fix" for dropped writes would change to a blocking
/// `send`. That change would compile, pass every other test, and put a cold-disk
/// `write()` back on the cpal input callback — the exact regression YV92's
/// review already caught once on `mark_device_change`.
#[test]
fn record_rs_journal_still_hands_off_with_try_send() {
    let record = src("record.rs");

    let offer = record
        .split_once("fn offer(&self, write: JournalWrite)")
        .expect("record.rs must still have JournalQueue::offer")
        .1;
    let body = &offer[..offer.find("\n    }").expect("offer body ends")];

    assert!(
        body.contains("try_send"),
        "row 5: the journal hand-off must be try_send, found:\n{body}"
    );
    assert!(
        !body.contains(".send("),
        "row 5: a blocking send on the journal path would park the audio callback:\n{body}"
    );
    assert!(
        body.contains("dropped.fetch_add"),
        "row 5: a dropped write must be counted, not swallowed:\n{body}"
    );

    // The queue is bounded at its construction site, too — an unbounded channel
    // would make the drop counter unreachable and the memory bound a fiction.
    assert!(
        record.contains("mpsc::sync_channel::<JournalWrite>(depth.max(1))"),
        "row 5: the journal channel must stay bounded"
    );
    assert!(
        !record.contains("mpsc::channel::<JournalWrite>"),
        "row 5: an unbounded journal channel defeats the whole row"
    );
}
