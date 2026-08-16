//! YV128 — finding #19, enforced by SQLite rather than by remembering.
//!
//! The epic plan's schema said `embedding_dim INTEGER NOT NULL DEFAULT 512`.
//! Nothing this backlog ships is 512-dim — the recommended CAM++ embedder is
//! 192 and the ResNet34 fallback the plan considered is 256 — so the default
//! was wrong for every model that will ever reach it, and wrong in the silent
//! direction: with a default present, a row written by code that forgot to
//! supply the width still succeeds, and every length check downstream then
//! passes against the guess.
//!
//! The fix is `NOT NULL` with **no default**, which makes "supply the width the
//! model reported" a constraint the database enforces on every INSERT rather
//! than a convention a reviewer has to catch. This file inserts without one and
//! requires the failure.

mod support;

use support::{open_db, temp_dir};

/// The catalog id the embedding width comes from. Named here rather than
/// hardcoded per-test so the tests read like the calls YV129 will make.
const MODEL: &str = "wespeaker-en-voxceleb-campplus";

#[test]
fn inserting_a_profile_without_a_dimension_fails_at_the_schema_level() {
    let dir = temp_dir("no-default-dim");
    let _db = open_db(&dir);

    let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).unwrap();
    let err = conn
        .execute(
            "INSERT INTO speaker_profiles
               (id, display_name, embedding_model, created_at, updated_at)
             VALUES ('p1', 'Wilson', ?1, '2026-08-15T00:00:00+00:00', '2026-08-15T00:00:00+00:00')",
            rusqlite::params![MODEL],
        )
        .expect_err("a profile with no embedding_dim must be refused by the schema");

    let msg = err.to_string();
    assert!(
        msg.contains("NOT NULL constraint failed")
            && msg.contains("speaker_profiles.embedding_dim"),
        "the refusal must name the column, so the fix is obvious: {msg}"
    );
}

/// The same claim from the other side: SQLite itself reports the column as
/// `NOT NULL` with a NULL default. This is what would go red if somebody
/// re-added `DEFAULT 512` in a later migration — the INSERT above would then
/// SUCCEED, and only this assertion would notice.
#[test]
fn the_column_is_not_null_and_carries_no_default() {
    let dir = temp_dir("dim-pragma");
    let _db = open_db(&dir);

    let conn = rusqlite::Connection::open(dir.join("wilson_voice.db")).unwrap();
    let (notnull, dflt): (i64, Option<String>) = conn
        .query_row(
            "SELECT \"notnull\", dflt_value FROM pragma_table_info('speaker_profiles')
             WHERE name = 'embedding_dim'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("speaker_profiles.embedding_dim exists");

    assert_eq!(notnull, 1, "embedding_dim must be NOT NULL");
    assert_eq!(
        dflt, None,
        "embedding_dim must have NO default — a default is a width guessed on \
         behalf of a model the schema has never loaded (finding #19)"
    );
}

/// And no width is written down anywhere in the migration SQL. `512` was the
/// plan's number; `192` (CAM++'s actual width) would be no better, because the
/// point of finding #19 is that the schema does not get to have an opinion — the
/// sidecar reports the width at model-load time and that report is the only
/// source.
#[test]
fn the_migration_sql_names_no_embedding_width_at_all() {
    let sql = wilson_voice_lib::meetings::MIGRATION_5_SPEAKER_PROFILES;
    for width in ["512", "192", "256", "128"] {
        assert!(
            !sql.contains(width),
            "migration 5 must not name an embedding width ({width}):\n{sql}"
        );
    }
    assert!(
        sql.contains("embedding_dim INTEGER NOT NULL"),
        "the column must still be NOT NULL:\n{sql}"
    );
    assert!(
        !sql.to_ascii_uppercase()
            .contains("EMBEDDING_DIM INTEGER NOT NULL DEFAULT"),
        "no DEFAULT on embedding_dim, in any casing:\n{sql}"
    );
}

/// The width supplied by a caller round-trips, whatever it is — the same
/// storage layer holds a 192-dim CAM++ profile and a 256-dim ResNet34 one,
/// because it has no opinion about either.
#[test]
fn any_width_the_caller_supplies_round_trips() {
    let dir = temp_dir("dim-round-trip");
    let db = open_db(&dir);

    for dim in [192_u32, 256, 80] {
        let profile = db
            .create_speaker_profile(&format!("Speaker {dim}"), dim, MODEL, false)
            .unwrap();
        let stored = db.get_speaker_profile(&profile.id).unwrap().unwrap();
        assert_eq!(stored.profile.embedding_dim, dim);
        assert_eq!(stored.profile.embedding_model, MODEL);
        assert!(
            stored.profile.locked,
            "a profile exists because a person named a voice — that is a \
             user-confirmed assignment, which YV129/YV130 may never auto-overwrite"
        );
        assert!(stored.centroids.is_empty(), "no voice heard yet");
    }

    // A zero width is not a width. Refused before it reaches SQLite, where it
    // would satisfy NOT NULL perfectly happily.
    assert!(db
        .create_speaker_profile("Nobody", 0, MODEL, false)
        .is_err());
}

/// `is_me` is a flag on an enrolled profile (YV125's correction to finding #4),
/// not a track index — so it is storable, readable, and belongs to a voice
/// somebody actually enrolled.
#[test]
fn is_me_is_a_profile_flag() {
    let dir = temp_dir("is-me-flag");
    let db = open_db(&dir);

    let me = db
        .create_speaker_profile("Wilson", 192, MODEL, true)
        .unwrap();
    let them = db
        .create_speaker_profile("Jeisil", 192, MODEL, false)
        .unwrap();

    assert!(
        db.get_speaker_profile(&me.id)
            .unwrap()
            .unwrap()
            .profile
            .is_me
    );
    assert!(
        !db.get_speaker_profile(&them.id)
            .unwrap()
            .unwrap()
            .profile
            .is_me
    );

    let all = db.list_speaker_profiles().unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all.iter().filter(|p| p.profile.is_me).count(), 1);
}
