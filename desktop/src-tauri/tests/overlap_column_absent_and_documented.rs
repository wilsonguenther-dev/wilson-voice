//! YV127 — the deliverable is an ABSENCE, so this is what holds it.
//!
//! Merged finding #22: `meeting_segments.overlapped`, as the plan's §5 schema
//! specified it, cannot be populated. sherpa's `OfflineSpeakerDiarization`
//! answers `{start, end, speaker, text}` per turn, and the frames where two
//! people talked at once are DELETED before the embedding model runs — upstream
//! of that result type. There is no moment at which the app could learn which
//! stored segment sat under two voices, so a column would hold `0` and a guess.
//!
//! An absence leaves no code to read, which is the whole problem with shipping
//! one: the next person to open `meetings.rs` looking for the column finds
//! nothing and cannot tell "decided against" from "not built yet". So this item
//! ships three things and this file asserts all three:
//!
//!   1. no migration in the ladder creates the column, and no live database has
//!      it — the absence itself,
//!   2. `meetings.rs` says where it went, on a marker line a `grep` finds,
//!   3. the fact reaches the PERSON, not just the reader of the source: one
//!      sentence under the transcript it qualifies, spelled identically in Rust
//!      and in the TypeScript that renders it.
//!
//! (3) is the half that would rot silently. The Rust constant has no Rust
//! renderer — the Meetings detail is React — so nothing but this file connects
//! `meetings::OVERLAP_CAVEAT` to the string a user actually reads.

use wilson_voice_lib::diarize_protocol::{DiarizeResponse, DiarizeSegment};
use wilson_voice_lib::meetings::{
    MIGRATION_1_MEETINGS, MIGRATION_2_MEETING_DIAGNOSTICS, MIGRATION_3_TWO_TRACK,
    MIGRATION_4_MEETING_KIND, OVERLAP_CAVEAT,
};

mod support;

/// The whole ladder `db::run_migrations` walks, named here so a future step
/// added there and not here shows up as a version mismatch rather than as a
/// silently unchecked migration.
const LADDER: &[(&str, &str)] = &[
    ("1 (meetings)", MIGRATION_1_MEETINGS),
    ("2 (diagnostics)", MIGRATION_2_MEETING_DIAGNOSTICS),
    ("3 (two-track)", MIGRATION_3_TWO_TRACK),
    ("4 (kind)", MIGRATION_4_MEETING_KIND),
];

/// The marker line YV127 adds to `meetings.rs`. Written out here rather than
/// grepped loosely: the point of a marker is that it says the MECHANISM, and a
/// test that accepted any sentence containing "overlap" would accept a comment
/// that said the opposite.
const MARKER: &str = "NO overlap flag — sherpa deletes overlapped frames before embedding";

/// The sentence the user reads. Third copy on purpose — Rust's constant, the
/// TypeScript mirror, and this pin — because two copies that agree prove
/// nothing about which one is right, and a pin that imports one of them would
/// follow it wherever it drifted.
const CAVEAT: &str = "Speech during overlapping talk is attributed to only one speaker.";

const MEETINGS_RS: &str = include_str!("../src/meetings.rs");
const PROTOCOL_RS: &str = include_str!("../src/diarize_protocol.rs");
const TRANSCRIPT_TS: &str = include_str!("../../src/meetings/transcript.ts");
const TRANSCRIPT_LIST_TSX: &str = include_str!("../../src/meetings/TranscriptList.tsx");

/// Part 1 — the absence, asserted against the SQL and then against a real file.
///
/// Both halves are here because they fail differently: the string check catches
/// a column added to a shipped step (which the ladder would never re-run and
/// which no fresh install would show), and the `pragma_table_info` check catches
/// one added anywhere else — a stray `ALTER TABLE` in `db.rs`, a `CREATE TABLE`
/// in a later item that quietly includes the field.
#[test]
fn no_step_in_the_migration_ladder_creates_an_overlap_column() {
    for (step, sql) in LADDER {
        assert!(
            !sql.to_ascii_lowercase().contains("overlap"),
            "migration {step} names an overlap column; finding #22 is that it \
             cannot be populated:\n{sql}"
        );
    }

    let dir = support::temp_dir("yv127-overlap");
    let db = support::open_db(&dir);
    assert_eq!(
        db.schema_version().unwrap(),
        wilson_voice_lib::meetings::SCHEMA_VERSION,
        "a fresh DB must be at the head of the ladder before its columns mean anything"
    );
    assert_eq!(
        LADDER.len() as i64,
        wilson_voice_lib::meetings::SCHEMA_VERSION,
        "a migration was added to db::run_migrations without being listed here, \
         so this test is checking a schema that is no longer the shipped one"
    );
    drop(db);

    let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).unwrap();
    for table in ["meetings", "meeting_segments"] {
        let mut stmt = conn
            .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .unwrap();
        let columns: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(
            !columns.is_empty(),
            "{table} should exist on a fresh database"
        );
        assert!(
            !columns
                .iter()
                .any(|c| c.to_ascii_lowercase().contains("overlap")),
            "{table} grew an overlap column: {columns:?}"
        );
    }
}

/// Part 2 — the explanation is where a reader will look for the column, and
/// there is exactly one of it.
///
/// The count matters as much as the presence. The acceptance criterion is that
/// `grep -n "overlapped" src/meetings.rs` lands on ONE line: the explanation.
/// Two matches would mean the word had come back somewhere else in the file —
/// the most likely somewhere being a column definition — and a reader grepping
/// their way to the answer would have to decide which hit was the answer.
#[test]
fn meetings_rs_says_on_one_line_where_the_column_went() {
    let hits: Vec<(usize, &str)> = MEETINGS_RS
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains("overlapped"))
        .map(|(i, l)| (i + 1, l.trim()))
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one `overlapped` line in meetings.rs, the one that \
         explains the absence; found {hits:#?}"
    );
    let (line_no, line) = hits[0];
    assert!(
        line.starts_with("///") || line.starts_with("//!"),
        "meetings.rs:{line_no} names the column outside a doc comment, which is \
         where a definition would live: {line}"
    );
    assert!(
        line.contains(MARKER),
        "meetings.rs:{line_no} is the marker line but does not state the \
         mechanism.\n  expected: {MARKER}\n  found:    {line}"
    );

    // The comment must also carry the DECISION and the sentence that replaces
    // the column — a marker that only says "no flag" documents the API, not the
    // choice this item made on the plan's behalf.
    for phrase in ["finding #22", "not in v1"] {
        assert!(
            MEETINGS_RS.contains(phrase),
            "the YV127 comment in meetings.rs no longer says {phrase:?}"
        );
    }
    assert!(
        MEETINGS_RS
            .lines()
            .any(|l| l.trim_start().starts_with("///") && l.contains(CAVEAT)),
        "the comment must quote the sentence that ships in place of the column, \
         not just declare the constant below it"
    );
}

/// Part 3 — the sentence exists once, in two languages, and the UI draws it.
///
/// `OVERLAP_CAVEAT` is user-visible copy declared in Rust with no Rust renderer;
/// the transcript surface is React. That is the same split `MIC_SPEAKER_LABEL`
/// lives with, and the same drift risk YV108's review already caught once on the
/// rendering rules — so the strings are compared byte for byte, and the
/// component is checked to render the CONSTANT rather than a hand-typed copy of
/// it that a later edit would leave behind.
#[test]
fn the_caveat_is_one_sentence_in_both_languages_and_the_ui_draws_it() {
    assert_eq!(OVERLAP_CAVEAT, CAVEAT, "the Rust constant changed wording");
    assert!(
        TRANSCRIPT_TS.contains(&format!("\"{CAVEAT}\"")),
        "src/meetings/transcript.ts no longer holds the caveat verbatim; the \
         screen and this constant have drifted"
    );
    assert!(
        TRANSCRIPT_TS.contains("export const OVERLAP_CAVEAT"),
        "the mirror must EXPORT the sentence, or the component cannot render it \
         from one place"
    );
    assert!(
        TRANSCRIPT_LIST_TSX.contains("{OVERLAP_CAVEAT}"),
        "TranscriptList must render the constant, not a second copy of the text"
    );
    assert!(
        TRANSCRIPT_LIST_TSX.contains("showsOverlapCaveat(segments, kind)"),
        "the caveat must be gated on the diarization branch — a caveat with no \
         condition is a disclaimer, and `virtual`+tap never clustered Track A"
    );
    assert!(
        !CAVEAT.is_empty() && TRANSCRIPT_TS.matches(CAVEAT).count() == 1,
        "the sentence should appear once in the mirror; a second copy is the \
         next drift"
    );
}

/// Part 4 — the wire the parent reads has no flag either, which is WHY there is
/// nothing to store.
///
/// `diarize_protocol.rs` tests its own segment shape; what is asserted here is
/// the response as the parent receives it — the full `{ok, segments: [...]}`
/// line, with a segment in it, because a field can be added to the envelope as
/// easily as to the turn.
#[test]
fn the_diarize_wire_carries_no_overlap_flag() {
    let wire = serde_json::to_string(&DiarizeResponse::diarized(
        7,
        vec![DiarizeSegment {
            start: 0.0,
            end: 4.2,
            cluster: 0,
            embedding: vec![0.25; 192],
        }],
        11,
    ))
    .expect("encode");
    assert!(
        !wire.to_ascii_lowercase().contains("overlap"),
        "the diarize response gained an overlap field; if the child really can \
         report overlap now, finding #22's second option was taken and this \
         item's decision has to be revisited on purpose:\n{wire}"
    );
    assert!(
        PROTOCOL_RS.contains("YV127"),
        "diarize_protocol.rs's segments field must point at the decision, or the \
         next reader will file the missing flag as an oversight"
    );
}
