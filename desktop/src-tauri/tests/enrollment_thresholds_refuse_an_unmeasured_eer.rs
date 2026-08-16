//! YV129 — the counterpart gate to YV124's expiring skip.
//!
//! OS-8's ordering requirement is one sentence: the anti-alias EER must be
//! measured *"before the enrollment thresholds are tuned, or those thresholds
//! permanently encode the aliasing"*. YV124 built the machine that goes red if a
//! capable machine skips the measurement. This file is the other end: it goes
//! red if this item tunes a threshold without one. Between the two, the ordering
//! is enforced by a machine at both ends instead of by a human remembering the
//! sequence.
//!
//! # What this test asserts, and where it departs from the backlog's wording
//!
//! The backlog says the test *"reads this document's YV124 `**MEASURED**` block
//! and FAILS while the literal string `EER: UNMEASURED` is still in it"*. Taken
//! literally that is an unconditionally red test on this base — the string IS
//! still there, and no machine in existence can remove it (YV122 has not merged,
//! so `yap-diarize` answers `no_backend` and there are no CAM++ embeddings
//! anywhere). A permanently red CI is not a gate; it is a broken build that
//! trains everyone to ignore the colour, and it would have blocked this item
//! from shipping the mechanism it is actually for.
//!
//! So the gate asserts the **conjunction** the surrounding sentence states, which
//! is the thing OS-8 actually requires:
//!
//! > **while the EER is unmeasured, no tuned enrollment band may exist in the
//! > shipping crate.**
//!
//! Both halves are read from the repository, so the gate can go red for either
//! reason: tune a band today and this test fails; measure the EER and replace
//! the block, and the test permits (and the golden-value pin in
//! `enrollment_threshold_from_harness.rs` then requires) a band. It is stated
//! here rather than quietly done, because a spec deviation nobody wrote down is
//! how a gate becomes decoration.
//!
//! # Why the block is mirrored into the repo
//!
//! The SSOT is an Obsidian note on one laptop. A gate that only runs there is
//! not a gate — YV124's own revision learned that when its
//! `YAP_EER_UNMEASURED_OK` declaration turned out never to be reached on CI. So
//! `docs/yap23-eer-status.md` carries a verbatim copy that every checkout has,
//! and `the_mirror_has_not_drifted_from_the_ssot` checks the copy against the
//! note on the machines that have the note.

#[path = "support/bands.rs"]
mod bands;

/// The gate.
#[test]
fn enrollment_thresholds_refuse_an_unmeasured_eer() {
    let block = bands::eer_status_block();
    let unmeasured = bands::block_says_unmeasured(&block);
    let sites = bands::tuned_band_sites();

    eprintln!(
        "YV124 EER: {} — tuned band sites in src/: {}",
        if unmeasured {
            "UNMEASURED"
        } else {
            "measured (per docs/yap23-eer-status.md)"
        },
        sites.len()
    );

    if unmeasured {
        assert!(
            sites.is_empty(),
            "OS-8 ORDERING VIOLATION — docs/yap23-eer-status.md still reads \
             `{}`, so the anti-alias EER has never been measured, yet the \
             shipping crate now carries {} tuned enrollment threshold(s):\n  {}\n\n\
             A band tuned on embeddings nobody has checked for aliasing \
             permanently encodes that aliasing. Fix it in the order OS-8 \
             names: land YV122 so `yap-diarize` has an inference backend, run \
             `cargo test --test meeting_eval anti_alias_eer_regression -- \
             --nocapture` on a corpus-equipped machine, paste the numbers into \
             YV124's block in the backlog note AND into \
             docs/yap23-eer-status.md, and only then tune. Do not delete this \
             test.",
            bands::UNMEASURED_MARKER,
            sites.len(),
            sites.join("\n  ")
        );
    } else {
        eprintln!(
            "the EER is recorded; tuned bands are now permitted and \
             enrollment_threshold_from_harness.rs's golden pin is what holds them \
             to the measurement"
        );
    }
}

/// The gate reads a copy. This is what stops the copy from drifting.
///
/// Binds only where the SSOT exists (a developer machine, or anywhere
/// `YAP23_BACKLOG_PATH` points at the note). On CI there is no note, and the
/// substance half above still binds — the same split YV124 settled on, for the
/// same reason.
#[test]
fn the_mirror_has_not_drifted_from_the_ssot() {
    let Some(ssot) = bands::ssot_backlog() else {
        eprintln!(
            "yap23 backlog note not on this machine, skipping the mirror-drift check \
             (set YAP23_BACKLOG_PATH to run it)"
        );
        return;
    };
    let note = std::fs::read_to_string(&ssot).expect("read the yap23 backlog note");
    let block = bands::eer_status_block();
    assert!(
        note.contains(&block),
        "docs/yap23-eer-status.md has drifted from {}. The mirror is a VERBATIM \
         copy of YV124's `**MEASURED**` block and nothing else; re-copy it rather \
         than editing either side by hand.",
        ssot.display()
    );
}

// ---------------------------------------------------------------------------
// Non-vacuity — every predicate above, driven both ways
// ---------------------------------------------------------------------------

#[test]
fn the_unmeasured_marker_is_detected_and_its_absence_is_too() {
    assert!(bands::block_says_unmeasured(
        "- **EER: UNMEASURED, deliberately not estimated.**"
    ));
    assert!(!bands::block_says_unmeasured(
        "- **EER: 0.0312 (shipped) vs 0.0714 (pre-fix), fixture (e).**"
    ));
}

#[test]
fn the_block_extractor_fails_loudly_rather_than_returning_nothing() {
    let good = format!(
        "prose\n{} x -->\ninside the block\n{}\ntail",
        bands::BLOCK_BEGIN,
        bands::BLOCK_END
    );
    assert_eq!(bands::extract_block(&good).as_deref(), Some("inside the block"));
    assert_eq!(
        bands::extract_block("no markers at all"),
        None,
        "a missing block must be None so the caller can panic, never an empty \
         string that reads as `nothing tuned`"
    );
    let inverted = format!("{} {} x -->", bands::BLOCK_END, bands::BLOCK_BEGIN);
    assert_eq!(bands::extract_block(&inverted), None);
}

#[test]
fn the_band_scanner_catches_a_tuned_constant_and_ignores_a_type_signature() {
    // The exact shapes this gate exists to catch — including the plan's own
    // blog-sourced numbers.
    for tuned in [
        "const ENROLLMENT_AUTO_CONFIRM: f32 = 0.70;",
        "pub const NEW_VOICE_FLOOR: CosineSimilarity = X;",
        "static SPEAKER_MATCH_THRESHOLD: f32 = 0.55;",
        "impl Default for EnrollmentBands {",
        "impl Default for ChipFloor {",
        "let floor = CosineSimilarity::new(0.55);",
    ] {
        assert!(
            bands::is_tuned_band_line(tuned),
            "scanner missed a tuned band: {tuned}"
        );
    }
    // Shapes that are NOT a tuned band, and a gate that flagged them would be
    // unsatisfiable rather than strict.
    for clean in [
        "pub fn match_cluster(c: &Embedding, p: &[SpeakerProfile], bands: EnrollmentBands)",
        "const IMPOSTOR_HEADROOM: f32 = 1e-4;",
        "return CosineSimilarity::new(0.0);",
        "Self::new(1.0 - similarity.get())",
        "let bands = EnrollmentBands::new(upper, lower)?;",
    ] {
        assert!(
            !bands::is_tuned_band_line(clean),
            "scanner false-positived on: {clean}"
        );
    }
}

#[test]
fn the_constructor_scan_is_paren_balanced_and_crosses_lines() {
    let split = "let b = EnrollmentBands::new( CosineSimilarity::new(0.70), \
                 CosineSimilarity::new(0.55), )?; const K: f64 = 9.9;";
    let args = bands::call_arguments(split, "EnrollmentBands::new(");
    assert_eq!(args.len(), 1, "one call, not one per line: {args:?}");
    assert!(
        bands::has_tuned_literal(&args[0]),
        "the literals inside the call must be seen: {:?}",
        args[0]
    );
    assert!(
        !args[0].contains("9.9"),
        "the unrelated constant after the call must NOT be folded into it: {:?}",
        args[0]
    );

    let clean = "EnrollmentBands::new(upper, lower)";
    let args = bands::call_arguments(clean, "EnrollmentBands::new(");
    assert_eq!(args.len(), 1);
    assert!(!bands::has_tuned_literal(&args[0]));
}

#[test]
fn unit_endpoints_are_not_tuned_thresholds_but_everything_else_is() {
    assert_eq!(
        bands::decimal_literals("f(0.70, -1.0, 2.0, 1e-4, x)"),
        vec!["0.70", "-1.0", "2.0"]
    );
    assert!(bands::has_tuned_literal("CosineSimilarity::new(0.55)"));
    assert!(!bands::has_tuned_literal("CosineSimilarity::new(0.0)"));
    assert!(!bands::has_tuned_literal("1.0 - similarity.get()"));
}

/// The scanner must be looking at the real tree, not at nothing.
///
/// `shipping_sources` already refuses a suspiciously small scan; this adds the
/// other half — the file this item wrote must be among the files scanned, so a
/// future `except` list cannot quietly exclude it and leave the gate green.
#[test]
fn the_scan_covers_the_module_this_item_added() {
    let scanned = bands::callsite::shipping_sources(&[]);
    assert!(
        scanned
            .iter()
            .any(|p| p.ends_with("speaker_profiles.rs")),
        "speaker_profiles.rs is where a tuned band would appear first and it was \
         not scanned: {scanned:?}"
    );
    assert!(
        scanned.iter().any(|p| p.ends_with("diarize_metrics.rs")),
        "the units module must be scanned too"
    );
}
