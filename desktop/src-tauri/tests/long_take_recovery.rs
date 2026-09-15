//! DB-B acceptance — **a crash or quit mid-long-take loses nothing.**
//!
//! The pieces all existed before this and none of them were joined up for the
//! dictation path. Y3-A spills a take's audio to `recovery/<id>.capture.wav` as
//! it is captured. The DB side of Y3-B persists each chunk's text to
//! `take_chunks` as it decodes. YV63 recovers a take that died while CAPTURING,
//! and its scan walks deliberately PAST a `.capture.wav` because that file is
//! not a journal marker. So a long take killed BETWEEN those two — audio on
//! disk, words on disk, no transcript row — left both artefacts sitting in the
//! recovery dir with nothing that could ever read them back. The 7-day sweep
//! deleted them a week later and the user never knew the words had survived at
//! all.
//!
//! `recover_dictation` is what reads them back. These tests pin the five things
//! that make it correct rather than merely present:
//!
//!   1. the kill really does leave audio + partial text + NO transcript row,
//!   2. the next launch OFFERS that take and never pastes it,
//!   3. finishing it runs the same gates a live take runs,
//!   4. the 7-day purge still bounds the recovery dir, text included,
//!   5. a take that COMPLETED leaves nothing recoverable (no phantom offers).

use std::path::{Path, PathBuf};

use wilson_voice_lib::db::Database;
use wilson_voice_lib::recover_dictation;

/// 16 kHz mono, the rate every recovery artefact in this app is written at.
const RATE: u32 = 16_000;

fn tmpdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "yap-dbb-long-take-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// Write the artefact `SpillWriter::drop` leaves behind: a PLAYABLE f32 WAV
/// named `<id>.capture.wav`, sitting in the recovery dir. Built with hound
/// directly rather than through the capture stack, because what is under test
/// is the READ side — what the next launch does with a file that is already on
/// disk when the process starts.
fn write_abandoned_spill(recovery: &Path, take_id: &str, seconds: f64) -> PathBuf {
    std::fs::create_dir_all(recovery).expect("recovery dir");
    let path = recovery.join(format!("{take_id}.capture.wav"));
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(&path, spec).expect("spill opens");
    let n = (RATE as f64 * seconds) as usize;
    for i in 0..n {
        // A tone, not silence: the recovery path has a minimum-length floor and
        // a real decode would have something to chew on.
        let s = 0.4 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / RATE as f32).sin();
        w.write_sample(s).expect("spill sample");
    }
    w.finalize().expect("spill finalizes");
    path
}

/// The body of one `fn` in `src/lib.rs`, for the structural assertions below.
///
/// This repo already proves gate properties this way (`retry_gate_truncates_
/// degenerate_tail` reads its own source the same way). It is the honest tool
/// for "this path calls the same gate as that path": the alternative is a mock
/// that proves only that the mock was wired up.
fn fn_body(name: &str) -> String {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
        .expect("lib.rs is readable");
    let after = src
        .split_once(&format!("fn {name}("))
        .unwrap_or_else(|| panic!("{name} is missing from lib.rs"))
        .1;
    // Everything up to the start of the next top-level item.
    match after.split_once("\n}\n") {
        Some((body, _)) => body.to_string(),
        None => after.to_string(),
    }
}

/// The same body with COMMENTS AND STRING LITERALS REMOVED.
///
/// This distinction is the whole difference between a grep that proves
/// something and a grep that proves nothing. `recover_dictation`'s prose says
/// "it never pastes" and its log line says "none pasted" — a naive search for
/// "paste" matches both and would fail a function that is provably correct,
/// while a search that tolerated them could be fooled by a comment. Only the
/// executable text can answer "does this call the paste path".
fn fn_code(name: &str) -> String {
    let body = fn_body(name);
    let mut out = String::with_capacity(body.len());
    for line in body.lines() {
        let code = match line.find("//") {
            Some(at) => &line[..at],
            None => line,
        };
        // Drop string literals: a message ABOUT pasting is not a call to paste.
        let mut in_str = false;
        for ch in code.chars() {
            if ch == '"' {
                in_str = !in_str;
                continue;
            }
            if !in_str {
                out.push(ch);
            }
        }
        out.push('\n');
    }
    out
}

#[test]
fn kill_mid_decode_leaves_audio_plus_partial_text_and_no_transcript_row() {
    // The state a `kill -9` during a long take's decode leaves on disk. Two
    // artefacts, written by two different subsystems, that only mean anything
    // together.
    let dir = tmpdir("kill-mid-decode");
    let db = Database::open(dir.join("wilson_voice.db")).expect("db opens");
    let recovery = dir.join("recovery");
    let take_id = "take-mid-decode";

    let spill = write_abandoned_spill(&recovery, take_id, 6.0);
    // Two chunks got through the decoder before the app died. They are on disk
    // the moment each one finished — that is the whole point of the table.
    db.record_take_chunk(take_id, 0, "the first chunk of a long take", 32_000)
        .expect("chunk 0 persists");
    db.record_take_chunk(take_id, 1, "and the second one as well", 64_000)
        .expect("chunk 1 persists");

    // Audio: on disk.
    assert!(spill.exists(), "the spilled audio survived the kill");
    // Partial text: on disk, in order, joined.
    assert_eq!(
        db.take_partial_text(take_id).expect("partial text reads"),
        Some("the first chunk of a long take and the second one as well".to_string()),
        "the decoded chunks must come back joined in chunk order"
    );
    // A resume knows where to start.
    assert_eq!(
        db.take_resume_sample(take_id).expect("resume sample reads"),
        64_000,
        "the resume point is the furthest chunk end, not the first"
    );
    // And no transcript row — the take never finished, so it is not history.
    assert!(
        db.list_transcripts(50, None)
            .expect("history reads")
            .is_empty(),
        "a take killed mid-decode must NOT be in history"
    );

    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn next_launch_offers_the_take_and_never_pastes_it() {
    let dir = tmpdir("next-launch");
    let db = Database::open(dir.join("wilson_voice.db")).expect("db opens");
    let recovery = dir.join("recovery");
    let take_id = "take-offered";

    write_abandoned_spill(&recovery, take_id, 5.0);
    db.record_take_chunk(take_id, 0, "words that already decoded", 48_000)
        .expect("chunk persists");

    // Before the sweep, nothing is on offer: the spill is not a journal marker,
    // which is exactly why YV63's scan alone left this take unreachable.
    assert!(
        db.list_failed_dictations().expect("list reads").is_empty(),
        "nothing is offered until the launch sweep runs"
    );

    let offered = recover_dictation(&db, &recovery);
    assert_eq!(offered, 1, "the unfinished take must be offered back");

    let rows = db.list_failed_dictations().expect("list reads");
    assert_eq!(rows.len(), 1, "one take, exactly one offer");
    let row = &rows[0];
    assert_eq!(
        row.take_id.as_deref(),
        Some(take_id),
        "the offer carries the take id, or its words can never be found again"
    );
    assert_eq!(
        row.partial_text.as_deref(),
        Some("words that already decoded"),
        "the offer carries the words the take had already produced"
    );
    // The audio was finalized into the same `<id>.wav` shape the YV52 retry
    // path and the 7-day purge already understand.
    let wav = PathBuf::from(&row.wav_path);
    assert!(wav.exists(), "the recovered clip is on disk at {wav:?}");
    assert!(
        wav.starts_with(&recovery),
        "recovered audio stays inside the recovery dir"
    );
    assert!(
        !recovery.join(format!("{take_id}.capture.wav")).exists(),
        "the spill was consumed, not left to be recovered a second time"
    );

    // NEVER PASTES, and not by inspection of behaviour but by construction:
    // `recover_dictation` takes no `AppHandle`, so it has nothing to paste
    // THROUGH. Pasting recovered words into whatever app happens to be focused
    // minutes or days later would drive straight through YV21's paste-target
    // guard.
    let code = fn_code("recover_dictation");
    for forbidden in ["paste", "AppHandle", "emit", "clipboard"] {
        assert!(
            !code.contains(forbidden),
            "recover_dictation must not CALL `{forbidden}` — a launch sweep that \
             can reach the paste path is one bug away from pasting recovered \
             words into whatever app happens to be focused days later"
        );
    }
    // The signature is the stronger half of the same proof: with no `AppHandle`
    // parameter there is nothing to paste THROUGH, so this is enforced by the
    // type checker and not merely by this test.
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
        .expect("lib.rs is readable");
    assert!(
        src.contains("pub fn recover_dictation(db: &Database, dir: &Path) -> usize"),
        "recover_dictation's signature must stay (db, dir) -> usize — an \
         AppHandle parameter is what would make pasting possible at all"
    );
    // And it must not DECODE either: a cold launch that pins the GPU for four
    // minutes because yesterday's take was unfinished is its own bug.
    for forbidden in ["transcribe", "asr_engine", "native_model_ready"] {
        assert!(
            !code.contains(forbidden),
            "recover_dictation must not CALL `{forbidden}` — recovery OFFERS the \
             take, and the user asks for the decode"
        );
    }

    // Still nothing in history: an OFFER is not a transcript.
    assert!(
        db.list_transcripts(50, None)
            .expect("history reads")
            .is_empty(),
        "offering a take must not write it to history"
    );

    // Idempotent — the launch after this one must not offer the same take twice.
    assert_eq!(recover_dictation(&db, &recovery), 0);
    assert_eq!(db.list_failed_dictations().expect("list reads").len(), 1);

    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn finishing_a_recovered_take_runs_the_same_gates_as_a_live_take() {
    // The rule this pins is the one lib.rs already records for the retry path:
    // "the retry path runs the SAME gate as the live path". DB-B extends that
    // path rather than forking it, so finishing a RECOVERED take has to go
    // through the same three things a live take does — the hallucination gate,
    // the dictionary, and the cleanup/polish pipeline — over the COMBINED text.
    let body = fn_body("retry_failed_dictation");

    // 1. The hallucination gate, by the same symbol the live path calls.
    assert!(
        body.contains("dictation::degenerate_cutoff"),
        "the finish path must run the same degenerate-tail gate as a live take"
    );
    assert!(
        body.contains("dictation::DEGEN_MIN_KEEP_TOKENS"),
        "…including the same keep threshold, not a second opinion about it"
    );
    // 2. The cleanup pipeline, dictionary and polish included.
    assert!(
        body.contains("dictation::run_cleanup"),
        "the finish path must run the same cleanup pipeline as a live take"
    );
    assert!(
        body.contains("apply_dictionary"),
        "…with the user's dictionary applied"
    );
    assert!(
        body.contains("polish::polish_llm"),
        "…and the same polish pass"
    );
    // 3. The recovered words are part of what those gates see — the partial
    //    text is folded into `raw_text` BEFORE the gate, not appended after it.
    let gate_at = body
        .find("dictation::degenerate_cutoff")
        .expect("the gate is in the body");
    let fold_at = body
        .find("partial_text")
        .expect("the finish path reads the take's partial text");
    assert!(
        fold_at < gate_at,
        "the recovered words must be folded in BEFORE the gate runs — text that \
         skips the gate is text the live path would never have produced"
    );
    // 4. It is a RESUME, not a replay: the decode starts at the persisted end.
    assert!(
        body.contains("take_resume_sample"),
        "the finish path must resume from the last persisted chunk end rather \
         than re-decoding audio whose words are already on disk"
    );
    // 5. And it still never auto-pastes into whatever is focused now.
    assert!(
        body.contains("copy_and_maybe_paste(&app, &text, false, None)"),
        "the finish path copies to the clipboard and does not paste — the user \
         is in Yap now, not in the app they dictated into"
    );

    // Behavioural half: a finished take really does leave the recoverable list
    // and land in history, carrying the moment it was SPOKEN.
    let dir = tmpdir("same-gates");
    let db = Database::open(dir.join("wilson_voice.db")).expect("db opens");
    let recovery = dir.join("recovery");
    let take_id = "take-finished";
    write_abandoned_spill(&recovery, take_id, 4.0);
    db.record_take_chunk(take_id, 0, "recovered words", 48_000)
        .expect("chunk persists");
    assert_eq!(recover_dictation(&db, &recovery), 1);
    let row = db.list_failed_dictations().expect("list").remove(0);

    // `convert_failed_dictation` is the one write the finish path ends in, and
    // it is the same write the YV52 retry has always used.
    let entry = db
        .convert_failed_dictation(
            &row.id,
            "recovered words and the rest".into(),
            "native".into(),
            1.0,
            Some("recovered words and the rest".into()),
        )
        .expect("a finished take converts");
    assert_eq!(entry.text, "recovered words and the rest");
    assert!(
        db.list_failed_dictations().expect("list").is_empty(),
        "a finished take must not stay on the recoverable list"
    );
    assert_eq!(
        db.list_transcripts(50, None).expect("history").len(),
        1,
        "a finished take lands in history exactly once"
    );

    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn recovery_dir_purge_still_bounds_growth() {
    // The recovery dir cannot become a place words accumulate forever. The
    // 7-day window applied to the audio before DB-B; it has to apply to the
    // TEXT too, or the words would outlive the sound they came from — a privacy
    // leak and an unbounded table in one.
    let dir = tmpdir("purge");
    let db = Database::open(dir.join("wilson_voice.db")).expect("db opens");
    let recovery = dir.join("recovery");
    let take_id = "take-expiring";

    write_abandoned_spill(&recovery, take_id, 4.0);
    db.record_take_chunk(
        take_id,
        0,
        "words that must not outlive their audio",
        48_000,
    )
    .expect("chunk persists");
    assert_eq!(recover_dictation(&db, &recovery), 1);
    let row = db.list_failed_dictations().expect("list").remove(0);
    let wav = PathBuf::from(&row.wav_path);
    assert!(wav.exists());

    // A cutoff in the future is the same instant arithmetic the 7-day sweep
    // does, without waiting a week for it.
    let purged = db
        .purge_failed_dictations(chrono::Utc::now() + chrono::Duration::days(1))
        .expect("purge runs");
    assert_eq!(purged.len(), 1, "the expired take's row is gone");
    for path in &purged {
        let _ = std::fs::remove_file(path);
    }
    assert!(!wav.exists(), "the expired take's AUDIO is gone");
    assert_eq!(
        db.take_partial_text(take_id).expect("partial text reads"),
        None,
        "the expired take's WORDS are gone too — text must never outlive the \
         audio it was decoded from"
    );
    assert!(
        db.list_failed_dictations().expect("list").is_empty(),
        "nothing is left on offer"
    );

    // Orphan chunk text — rows whose take row never existed or already went —
    // is swept on the same window, so the table cannot grow without bound.
    db.record_take_chunk("orphan-take", 0, "nobody points at these words", 16_000)
        .expect("orphan chunk persists");
    let swept = db
        .purge_take_chunks(chrono::Utc::now() + chrono::Duration::days(1))
        .expect("chunk purge runs");
    assert_eq!(swept, 1, "orphan chunk text is swept on the same window");
    assert_eq!(db.take_partial_text("orphan-take").expect("reads"), None);

    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_completed_take_leaves_nothing_recoverable() {
    // No phantom offers. A take that finished normally has consumed its spill
    // and cleared its chunks, so the next launch finds nothing — offering the
    // user a take they already have in History is its own kind of data loss,
    // because it teaches them the offers are noise.
    let dir = tmpdir("completed");
    let db = Database::open(dir.join("wilson_voice.db")).expect("db opens");
    let recovery = dir.join("recovery");
    let take_id = "take-completed";

    // The take ran, chunk by chunk…
    db.record_take_chunk(take_id, 0, "a take that finished", 32_000)
        .expect("chunk persists");
    db.record_take_chunk(take_id, 1, "all the way to the end", 64_000)
        .expect("chunk persists");
    // …and then COMPLETED: the transcript is written and the chunks are cleared.
    db.insert_transcript(
        "a take that finished all the way to the end".into(),
        "native".into(),
        1.0,
        4.0,
        0,
        None,
    )
    .expect("the completed take lands in history");
    assert_eq!(
        db.clear_take_chunks(take_id).expect("chunks clear"),
        2,
        "a completed take clears the chunk rows it accumulated"
    );

    // Its spill was consumed by the normal finish path, so the recovery dir is
    // empty. The launch sweep must find nothing at all.
    std::fs::create_dir_all(&recovery).expect("recovery dir");
    assert_eq!(
        recover_dictation(&db, &recovery),
        0,
        "a completed take must leave NOTHING for the next launch to offer"
    );
    assert!(
        db.list_failed_dictations().expect("list").is_empty(),
        "no phantom offer"
    );
    assert_eq!(
        db.take_partial_text(take_id).expect("partial text reads"),
        None,
        "no partial text lingers behind a completed take"
    );
    assert_eq!(
        db.list_transcripts(50, None).expect("history").len(),
        1,
        "the completed take is in history, once"
    );

    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}
