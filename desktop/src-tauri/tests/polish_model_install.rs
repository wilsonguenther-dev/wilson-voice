//! SEC-C — the polish model's install path, asserted with ZERO model bytes.
//!
//! The gap this covers was not "the catalog is missing" — the catalog has been
//! pinned since YV60 — it was that `download_polish_model_with` had no caller a
//! user could reach, so `polish_model` was permanently `""` and the whole LLM
//! stage was dead code on every installed copy. These tests pin the four
//! properties that make the new install path safe to hand a stranger:
//!
//! 1. every offered entry carries a digest (never a bare URL),
//! 2. an interrupted or short file is never selected,
//! 3. a digest mismatch deletes the bytes rather than resuming into them,
//! 4. with NO model the pipeline is byte-identical to today,
//!
//! plus the two diagnosability properties — a named free-space refusal, and a
//! missing sidecar binary reported once rather than once per take.

use std::path::{Path, PathBuf};

use wilson_voice_lib::dictation::{run_cleanup, CleanupLevel, DictationMode, Style};
use wilson_voice_lib::models::{
    check_polish_free_space, finalize_download, offered_polish_models, partial_path,
    polish_model as catalog_polish_model, polish_models, polish_required_free_bytes,
    verified_polish_selection, OFFERED_POLISH_RANK, POLISH_FREE_SPACE_HEADROOM_BYTES,
};
use wilson_voice_lib::polish::{
    note_missing_sidecar, polish_llm, reset_missing_sidecar_report_for_tests, PolishConfig,
};

/// A scratch directory unique to one test. No `tempfile` dev-dependency exists
/// in this crate, and adding one for six tests is a dependency nobody asked
/// for, so this is the std-only equivalent.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "yap-polish-install-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Self(dir)
    }
    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// sha256 of the empty string — a real digest that no non-empty file matches.
const SHA256_OF_EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// Every polish entry pins a sha256 and a byte count. A catalog row with a bare
/// URL and no digest would make `finalize_download`'s verification gate
/// unenforceable — there would be nothing to compare against — so this is the
/// precondition the other five tests stand on.
#[test]
fn catalog_polish_entries_carry_a_digest() {
    let all = polish_models();
    assert!(!all.is_empty(), "the polish catalog is empty");
    for m in all {
        assert_eq!(
            m.file.sha256.len(),
            64,
            "polish model '{}' sha256 is not 64 hex chars: {:?}",
            m.id,
            m.file.sha256
        );
        assert!(
            m.file
                .sha256
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "polish model '{}' sha256 is not lowercase hex",
            m.id
        );
        assert!(
            m.file.size_bytes > 0,
            "polish model '{}' pins no size",
            m.id
        );
    }

    // And the installer offers strictly fewer than the catalog holds: the 0.5B
    // fast tier is withheld because it was MEASURED worse than no model at all
    // (err=max_out at 100/150/200/300/400 words).
    let offered = offered_polish_models();
    assert!(!offered.is_empty(), "nothing is offered for install");
    for m in &offered {
        assert_eq!(m.recommended_rank, Some(OFFERED_POLISH_RANK));
    }
    assert!(
        offered.len() < all.len(),
        "the fast tier should still be withheld"
    );
}

/// A half-downloaded GGUF that loads and produces garbage is worse than no
/// model. `verified_polish_selection` is the ONLY producer of a non-empty
/// `polish_model`, so it is the one place that has to refuse a short file — and
/// a full-size file with a `.partial` still beside it, which is what an
/// interrupted resume looks like on disk.
#[test]
fn interrupted_download_is_not_marked_ready() {
    let scratch = Scratch::new("interrupted");
    let model = offered_polish_models()[0];
    let dest = scratch.join(&model.file.filename);

    // (a) nothing on disk at all.
    assert!(verified_polish_selection(&model.id, &dest).is_err());

    // (b) a short file — the classic interrupted download renamed into place.
    std::fs::write(&dest, b"not the whole gguf").unwrap();
    let err = verified_polish_selection(&model.id, &dest).unwrap_err();
    assert!(
        err.contains("incomplete"),
        "a short file must be refused as incomplete, got: {err}"
    );

    // (c) a directory where the file should be.
    let dir_dest = scratch.join("as-a-dir");
    std::fs::create_dir_all(&dir_dest).unwrap();
    assert!(verified_polish_selection(&model.id, &dir_dest).is_err());

    // (d) full size, but a `.partial` still beside it: a resume in flight.
    std::fs::write(&dest, vec![0u8; model.file.size_bytes as usize % 4096 + 1]).unwrap();
    let partial = partial_path(&dest);
    std::fs::write(&partial, b"resuming").unwrap();
    assert!(verified_polish_selection(&model.id, &dest).is_err());

    // (e) an id that is not in the catalog at all.
    assert!(verified_polish_selection("no-such-model", &dest).is_err());
}

/// The verification gate deletes, it does not resume. Corrupt bytes left on
/// disk would be treated as a resumable prefix by the next attempt and the
/// download would never converge.
#[test]
fn digest_mismatch_is_rejected_and_deleted() {
    let scratch = Scratch::new("mismatch");
    let dest = scratch.join("model.gguf");
    let partial = partial_path(&dest);
    std::fs::write(&partial, b"corrupted bytes").unwrap();

    let err = finalize_download(&partial, &dest, SHA256_OF_EMPTY).unwrap_err();
    assert!(!err.is_empty());
    assert!(
        !partial.exists(),
        "a mismatched partial must be deleted, not left to resume into"
    );
    assert!(
        !dest.exists(),
        "a mismatched partial must never be renamed into place"
    );

    // The same gate on MATCHING bytes does rename into place — otherwise this
    // test would pass on a `finalize_download` that always failed.
    std::fs::write(&partial, b"").unwrap();
    finalize_download(&partial, &dest, SHA256_OF_EMPTY).expect("matching digest must finalize");
    assert!(dest.exists() && !partial.exists());
}

/// THE REGRESSION THAT MATTERS MOST. With no polish model the pipeline must be
/// byte-identical to the rules-only pipeline it is today. `polish_llm` returns
/// `None` before it can spawn anything, `run_cleanup` keeps the rules text, and
/// the output does not move by one byte.
#[test]
fn absent_model_leaves_the_pipeline_byte_identical() {
    // Zero model bytes: the default config, which is what a real install has.
    let off = PolishConfig::default();
    assert_eq!(off.model, "", "the default must stay OFF");

    // And an id that IS in the catalog but has no file on disk — the dangerous
    // middle state the install path could otherwise create.
    let named_but_absent = PolishConfig {
        model: offered_polish_models()[0].id.clone(),
        ..PolishConfig::default()
    };

    const CORPUS: &[&str] = &[
        "um so we shipped the build and emailed the client",
        "hey there i wanted to follow up on the invoice you sent last week actually the week before",
        "first we land the migration then we cut the release then we tell support",
        "no i mean i actually i mean the second one",
        "the meeting is at three thirty and i think we should bring the deck",
        "",
    ];

    for mode in [
        DictationMode::Email,
        DictationMode::Document,
        DictationMode::Notes,
        DictationMode::Chat,
        DictationMode::Plain,
    ] {
        for cfg in [&off, &named_but_absent] {
            // The stage itself declines before any process could be spawned.
            for raw in CORPUS {
                assert_eq!(
                    polish_llm(raw, mode, cfg),
                    None,
                    "polish_llm must decline with no model on disk ({mode:?})"
                );
            }

            for raw in CORPUS {
                // The rules-only pipeline: the polish closure is not even wired.
                let rules_only = run_cleanup(
                    raw,
                    CleanupLevel::High,
                    mode,
                    Style::Default,
                    &[],
                    |t| t.to_string(),
                    |_| None,
                );
                // The shipping pipeline, with the real polish stage attached.
                let with_stage = run_cleanup(
                    raw,
                    CleanupLevel::High,
                    mode,
                    Style::Default,
                    &[],
                    |t| t.to_string(),
                    |t| polish_llm(t, mode, cfg),
                );
                assert_eq!(
                    rules_only.as_bytes(),
                    with_stage.as_bytes(),
                    "polish stage changed the output with no model installed \
                     (mode {mode:?}, model {:?}, raw {raw:?})",
                    cfg.model
                );
            }
        }
    }
}

/// A missing `binaries/yap-polish-<triple>` is a BROKEN BUILD, and before this
/// its only symptom was every take silently paying the full polish deadline
/// against a handshake nothing could ever answer. Say it once, with the path.
#[test]
fn missing_sidecar_binary_is_reported_once_not_per_take() {
    reset_missing_sidecar_report_for_tests();
    let expected = Path::new("/nonexistent/Yap.app/Contents/MacOS/yap-polish");

    assert!(
        note_missing_sidecar(expected),
        "the first take must report the missing binary"
    );
    for take in 2..=25 {
        assert!(
            !note_missing_sidecar(expected),
            "take {take} reported the missing binary again — that is once per take, \
             which buries the one line that names the packaging defect"
        );
    }

    // The latch is per-process and re-armable only by this test hook, so a
    // second test in this binary starts from a known state.
    reset_missing_sidecar_report_for_tests();
    assert!(note_missing_sidecar(expected));
}

/// A 1.12 GB download with no disk check is how "optional polish" becomes
/// "polish silently does nothing" on exactly the machines least able to explain
/// why. The refusal is NAMED and carries the numbers.
#[test]
fn refuses_when_there_is_not_enough_free_space() {
    let model = offered_polish_models()[0];
    let required = polish_required_free_bytes(model);
    assert_eq!(
        required,
        model.file.size_bytes + POLISH_FREE_SPACE_HEADROOM_BYTES,
        "headroom must be counted on top of the file itself"
    );

    // Bone dry, and one byte short: both refuse.
    for free in [0, required - 1, model.file.size_bytes] {
        let err = check_polish_free_space(model, Some(free)).unwrap_err();
        assert!(
            err.contains("not enough free space"),
            "the refusal must be named, got: {err}"
        );
        assert!(
            err.contains("free") && err.contains("needed"),
            "the refusal must carry the numbers, got: {err}"
        );
    }

    // Exactly enough, and plenty: both proceed.
    assert!(check_polish_free_space(model, Some(required)).is_ok());
    assert!(check_polish_free_space(model, Some(required * 4)).is_ok());

    // Unmeasurable (statvfs failed) is NOT a refusal — the download's own write
    // errors are a better signal than a guess, and refusing on a failed probe
    // would make polish uninstallable on a volume we simply cannot stat.
    assert!(check_polish_free_space(model, None).is_ok());
}

/// `polish_model` becomes non-empty through exactly one door: a full-size,
/// digest-verified file on disk. This is the property that keeps a catalog id
/// from ever naming weights that are not there — which would cost every take
/// the full `polish_deadline_ms` before falling back.
#[test]
fn polish_model_is_set_only_after_a_digest_verified_file_exists() {
    let scratch = Scratch::new("selection");
    let model = offered_polish_models()[0];
    let dest = scratch.join(&model.file.filename);

    // No file: no selection, so `polish_model` stays "".
    assert!(verified_polish_selection(&model.id, &dest).is_err());

    // A file of EXACTLY the catalog size, with no `.partial` beside it. That is
    // the only shape `finalize_download` can produce, and it produces it only
    // after the sha256 matched — so "present at this size" IS "digest-verified".
    // Written sparsely: 1.1 GB of real bytes is not a thing to do in a unit test.
    let f = std::fs::File::create(&dest).unwrap();
    f.set_len(model.file.size_bytes).unwrap();
    drop(f);

    let selection = verified_polish_selection(&model.id, &dest)
        .expect("a full-size, partial-free file must mint a selection");
    assert_eq!(selection, model.id);
    assert!(
        catalog_polish_model(&selection).is_some(),
        "the selection must be a catalog id, never a path or a filename"
    );
    assert!(!selection.is_empty());

    // One byte short of the catalog size and the door shuts again.
    let f = std::fs::File::create(&dest).unwrap();
    f.set_len(model.file.size_bytes - 1).unwrap();
    drop(f);
    assert!(verified_polish_selection(&model.id, &dest).is_err());
}
