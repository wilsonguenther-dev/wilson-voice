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
//! # The measured branch is the half a review finding found open
//!
//! The first version of this gate read an adjective out of a file inside the
//! same repository. A tuning PR could flip `EER: UNMEASURED` to
//! `EER: 0.031 (measured)` and add a band in one diff, and on CI — where the
//! Obsidian SSOT does not exist — every test stayed green, because the only half
//! that could have caught the forgery (`the_mirror_has_not_drifted_from_the_ssot`)
//! skipped instead of failing. That is the same shape as YV124's
//! `YAP_EER_UNMEASURED_OK` declaration that was never reached, one level up.
//!
//! Two things close it, and both bind on CI:
//!
//! 1. **An absent SSOT is a hard FAILURE the moment the mirror stops saying
//!    `UNMEASURED`.** It is only tolerable while the mirror admits there is
//!    nothing to certify.
//! 2. **A measured block must carry the harness's machine-generated
//!    provenance** — run id, corpus digest, genuine/impostor counts, the ROC
//!    sweep, the placed edges — and this test VERIFIES that record for internal
//!    consistency against the rule `bands_from_distribution` implements, rather
//!    than reading a word. Forging it means fabricating a self-consistent run,
//!    not editing an adjective.

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
        return;
    }

    // ---- the measured branch: the mirror no longer certifies itself ----

    let ssot = bands::ssot_backlog().unwrap_or_else(|| {
        panic!(
            "docs/yap23-eer-status.md claims the anti-alias EER is MEASURED, and the SSOT it is \
             supposed to be a copy of is not reachable from this machine. That combination is a \
             FAILURE, never a skip: a status file inside this repository, editable in the same \
             diff that tunes a band, certifies nothing on its own — which is exactly how a review \
             probe flipped this gate green while adding the vendor pair. Either run this where \
             the note lives, point YAP23_BACKLOG_PATH at it, or put the mirror back to `{}`.",
            bands::UNMEASURED_MARKER
        )
    });
    let note = std::fs::read_to_string(&ssot).expect("read the yap23 backlog note");
    assert!(
        note.contains(&block),
        "the mirror claims MEASURED and does not match {} — re-copy YV124's block rather than \
         editing either side by hand",
        ssot.display()
    );

    let provenance = bands::parse_provenance(&block).unwrap_or_else(|e| {
        panic!(
            "the MEASURED block carries no machine-generated provenance ({e}). A measurement is a \
             run id, a corpus digest, the pair counts and the printed sweep — not the word \
             \"measured\". See docs/yap23-eer-status.md for the record this gate reads."
        )
    });
    let problems = bands::verify_provenance(&provenance);
    assert!(
        problems.is_empty(),
        "the MEASURED block's provenance is not internally consistent, so no run produced \
         it:\n  {}",
        problems.join("\n  ")
    );
    eprintln!(
        "EER measured: run {} over {} genuine / {} impostor pairs on {} — bands {:.4} / {:.4}",
        provenance.run_id,
        provenance.genuine,
        provenance.impostor,
        provenance.fixture,
        provenance.auto_confirm,
        provenance.new_voice_floor
    );
}

/// The gate reads a copy. This is what stops the copy from drifting.
///
/// Binds only where the SSOT exists (a developer machine, or anywhere
/// `YAP23_BACKLOG_PATH` points at the note). On CI there is no note — and that
/// is now only survivable while the mirror still says `UNMEASURED`, which the
/// gate above enforces.
#[test]
fn the_mirror_has_not_drifted_from_the_ssot() {
    let Some(ssot) = bands::ssot_backlog() else {
        assert!(
            bands::block_says_unmeasured(&bands::eer_status_block()),
            "no SSOT on this machine AND the mirror claims a measurement — see \
             enrollment_thresholds_refuse_an_unmeasured_eer"
        );
        eprintln!(
            "yap23 backlog note not on this machine, skipping the mirror-drift check \
             (set YAP23_BACKLOG_PATH to run it). The mirror still says UNMEASURED, which is \
             the only state in which that skip is honest."
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

/// The mirror's committed digest, checked everywhere, SSOT or not.
///
/// A hash is not a certificate — the same diff can move both — but it makes an
/// edit to the file this gate reads a second, deliberate, reviewable line
/// instead of a prose change nobody diffed.
#[test]
fn the_mirror_matches_its_committed_digest() {
    let block = bands::eer_status_block();
    let actual = bands::sha256_hex(block.as_bytes());
    assert_eq!(
        actual,
        bands::MIRROR_SHA256,
        "docs/yap23-eer-status.md's mirrored block has changed. If that was a re-copy of YV124's \
         block from the SSOT, update MIRROR_SHA256 in tests/support/bands.rs to {actual} in the \
         same commit and say so in the PR body."
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
        "let bands = EnrollmentBands::from_measured_edges(upper, lower)?;",
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

/// **The review probe, committed.**
///
/// A rename plus one level of indirection defeated the name-based scanner: both
/// threshold gates reported 8 passed / 8 passed with the vendor pair sitting in
/// shipping source. Every net has to catch it independently, so no single one
/// can rot and leave the others carrying a claim nobody checked.
#[test]
fn the_scanner_catches_the_renamed_indirect_probe() {
    let hits = bands::scan_source("speaker_profiles.rs", bands::PROBE_SOURCE, &[]);
    assert!(
        !hits.is_empty(),
        "the probe that defeated the first scanner is green again: {}",
        bands::PROBE_SOURCE
    );

    // Net 3 — a tuned float constant in the band module, whatever it is called.
    let consts = bands::numeric_const_sites(bands::PROBE_SOURCE);
    assert_eq!(
        consts.len(),
        2,
        "both renamed constants must be seen by location, not by name: {consts:?}"
    );

    // Net 2 — the literal reaches the constructor through one `const`.
    let code = bands::shipping_code(bands::PROBE_SOURCE);
    let table = bands::const_table(&code);
    let args = bands::call_arguments(&code, "EnrollmentBands::new(");
    assert_eq!(args.len(), 1);
    assert!(
        !bands::has_tuned_literal(&args[0]),
        "the whole point of the probe: the call site itself is literal-free"
    );
    assert!(
        bands::has_tuned_literal(&bands::expand_consts(&args[0], &table)),
        "one level of const indirection must be resolved before calling it clean"
    );

    // Net 1 — provenance: a producer that is not the measured one.
    let sites = bands::construction_sites(&code, "EnrollmentBands");
    assert!(
        sites.iter().any(|(owner, _)| owner == "shipped_bands"),
        "the construction must be attributed to the function that did it: {sites:?}"
    );

    // Net 4 — the SERDE door, which is the second probe and the one the
    // compile-time seal did nothing about: no constructor is involved at all.
    let data = bands::deserialization_sites(&code, "EnrollmentBands");
    assert!(
        data.iter().any(|s| s.contains("derives Deserialize")),
        "the derive is the door: serde produces an EnrollmentBands with no constructor \
         and no Inverted check — {data:?}"
    );
    assert!(
        data.iter()
            .any(|s| s.contains("from_str") && s.contains("shipped_bands_from_data")),
        "the call must be caught inside the function that names the type, however the \
         call is wrapped — {data:?}"
    );
    // …and none of the other four nets sees the serde probe, which is why it
    // needed its own. (Net 3 is checked above at exactly 2: the `&str` payload
    // is not an f32/f64 const.)
    let serde_probe = "pub fn shipped_bands_from_data() -> Result<EnrollmentBands, \
                       serde_json::Error> { serde_json::from_str(OPENWHISPR_BANDS) }";
    assert!(
        bands::call_arguments(serde_probe, "EnrollmentBands::new(").is_empty()
            && bands::construction_sites(serde_probe, "EnrollmentBands").is_empty()
            && !bands::is_tuned_band_line(serde_probe),
        "if nets 1, 2 and 5 could see this on their own, net 4 would not be load-bearing"
    );

    // …and the same scan over a file that only PASSES bands around is clean.
    let clean = "pub fn match_cluster(c: &Embedding, p: &[SpeakerProfile], bands: EnrollmentBands) \
                 -> MatchResult { if c.dim() == 0 { return MatchResult::New; } decide(bands) }";
    assert!(
        bands::scan_source("speaker_profiles.rs", clean, &[]).is_empty(),
        "a gate that flags a parameter is unsatisfiable rather than strict"
    );

    // Nor does merely SERIALIZING a measured band trip the net: the whole point
    // of the fix is that a band travels outward and never inward.
    let outward = "#[derive(Debug, Clone, Copy, PartialEq, Serialize)] \
                   #[serde(rename_all = \"camelCase\")] \
                   pub struct EnrollmentBands { auto_confirm: CosineSimilarity, \
                   new_voice_floor: CosineSimilarity }";
    assert!(
        bands::deserialization_sites(&bands::shipping_code(outward), "EnrollmentBands").is_empty(),
        "Serialize alone is the shipped shape and must not be a hit"
    );
}

/// Net 4's own unit tests: the two doors, and the two shapes that are not doors.
#[test]
fn the_data_net_sees_both_doors_and_neither_false_positive() {
    // The derive door, with a `pub` and an intervening attribute — the exact
    // shape that made a naive backwards walk report "no attributes at all".
    let derived = bands::shipping_code(
        "/// docs\n#[derive(Debug, Deserialize)]\n#[serde(rename_all = \"camelCase\")]\n\
         pub struct EnrollmentBands { a: f32 }\n",
    );
    assert!(
        !bands::deserialization_sites(&derived, "EnrollmentBands").is_empty(),
        "the derive above a `pub struct`, past a `#[serde]` and a doc comment"
    );

    // The hand-written form of the same door.
    let by_hand = bands::shipping_code(
        "impl<'de> Deserialize<'de> for EnrollmentBands { fn deserialize<D>(d: D) -> \
         Result<Self, D::Error> { todo!() } }",
    );
    assert!(
        !bands::deserialization_sites(&by_hand, "EnrollmentBands").is_empty(),
        "a hand-written impl is the same inward path as a derived one"
    );

    // NOT a door: a deserializer in a function that has nothing to do with a
    // band. `extend_from_slice` alone appears dozens of times in `support.rs`,
    // and a net that fired on the file rather than the function would flag
    // every one of them.
    let unrelated = bands::shipping_code(
        "pub fn zip(out: &mut Vec<u8>, data: &[u8]) { out.extend_from_slice(data); } \
         pub fn floor(f: ChipFloor) -> bool { f.min_turns > 0 }",
    );
    assert!(
        bands::deserialization_sites(&unrelated, "ChipFloor").is_empty(),
        "a gate that flagged `extend_from_slice` would be turned off within a week"
    );

    // NOT a door: the type's own `Serialize` half.
    let outward =
        bands::shipping_code("#[derive(Serialize)] pub struct ChipFloor { min_turns: usize }");
    assert!(bands::deserialization_sites(&outward, "ChipFloor").is_empty());
}

/// The seal's scanner half: a test-only item is not shipping code, and nothing
/// else is exempt.
#[test]
fn only_the_two_test_only_cfgs_are_blanked_from_the_shipping_half() {
    let src = "pub fn a() {}\n\
               #[cfg(any(test, feature = \"test-bands\"))]\n\
               pub fn for_test() -> Bands { Bands::new(0.70, 0.55) }\n\
               #[cfg(target_os = \"macos\")]\n\
               pub fn mac() -> Bands { Bands::new(0.70, 0.55) }\n\
               #[cfg(test)]\n\
               mod tests { fn t() { let _ = \"{{\"; } }\n\
               pub fn b() {}\n";
    let half = bands::shipping_half(src);
    assert!(half.contains("pub fn a()"), "{half}");
    assert!(
        half.contains("pub fn b()"),
        "blanking, not truncating: code AFTER a test module must still be scanned\n{half}"
    );
    assert!(
        !half.contains("for_test"),
        "a cfg(test)/test-bands item is not in a release build\n{half}"
    );
    assert!(
        half.contains("pub fn mac()"),
        "cfg(target_os) IS shipping code and must still be scanned\n{half}"
    );
    assert_eq!(
        half.lines().count(),
        src.lines().count(),
        "line numbers must survive so a hit can be reported at its line"
    );
}

/// The mask is what makes every brace-balanced scan safe.
#[test]
fn comments_and_string_literals_are_not_code() {
    let masked = bands::mask_code(
        "let x = 1; // const AUTO_CONFIRM: f32 = 0.70;\nlet s = \"const NEW_VOICE_FLOOR = 0.55;\";\nlet y = 2;",
    );
    assert!(masked.contains("let x = 1;"));
    assert!(masked.contains("let y = 2;"));
    assert!(
        !masked.contains("AUTO_CONFIRM") && !masked.contains("NEW_VOICE_FLOOR"),
        "a threshold named in a comment or a string is not a threshold: {masked}"
    );
    assert_eq!(
        masked.len(),
        "let x = 1; // const AUTO_CONFIRM: f32 = 0.70;\nlet s = \"const NEW_VOICE_FLOOR = 0.55;\";\nlet y = 2;".len(),
        "the mask is length-preserving so offsets stay meaningful"
    );
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

// ---------------------------------------------------------------------------
// The provenance verifier, driven both ways
// ---------------------------------------------------------------------------

/// A record shaped exactly like one a real run prints: twenty pairs a side, an
/// equal-error point at 0.59, a 5 % FAR budget putting auto-confirm at 0.61.
fn worked_provenance() -> String {
    format!(
        "{}\nharness: meeting_eval::tune_enrollment_band\n\
         run_id: 2026-08-20T17:04:11Z-9f3c1a\n\
         corpus_digest: sha256:{}\n\
         fixture: room-3-near-field\n\
         genuine: 20\nimpostor: 20\n\
         eer: 0.1000\nfar_at_eer: 0.1000\nfrr_at_eer: 0.1000\neer_threshold: 0.5900\n\
         target_far: 0.0500\nauto_confirm: 0.6100\nnew_voice_floor: 0.5900\n\
         far_at_auto_confirm: 0.0500\nfrr_at_new_voice_floor: 0.1000\n\
         sweep:\n  0.3900 far=0.1000 frr=0.0000\n  0.5900 far=0.1000 frr=0.1000\n  \
         0.6100 far=0.0500 frr=0.1000\n  0.6300 far=0.0000 frr=0.1000\n{}",
        bands::PROVENANCE_BEGIN,
        "a".repeat(64),
        bands::PROVENANCE_END
    )
}

#[test]
fn a_consistent_provenance_record_verifies() {
    let p = bands::parse_provenance(&worked_provenance()).expect("parses");
    assert_eq!((p.genuine, p.impostor), (20, 20));
    assert_eq!(p.sweep.len(), 4);
    let problems = bands::verify_provenance(&p);
    assert!(problems.is_empty(), "{problems:?}");
}

/// Every consistency rule, broken one at a time. This is the test that makes
/// "flip the adjective" stop working: each mutation is a thing a forger has to
/// get right, and each one is checked against another number in the same record.
#[test]
fn a_forged_measurement_fails_on_its_own_arithmetic() {
    let cases: [(&str, &str, &str); 7] = [
        (
            "\neer: 0.1000",
            "\neer: 0.0310",
            "an EER that is not the mean of its own FAR/FRR",
        ),
        (
            "far_at_auto_confirm: 0.0500",
            "far_at_auto_confirm: 0.0310",
            "an achieved FAR no sample of 20 impostor pairs can express",
        ),
        (
            "new_voice_floor: 0.5900",
            "new_voice_floor: 0.4000",
            "a floor that is not the equal-error point the shipped rule places it at",
        ),
        (
            "auto_confirm: 0.6100",
            "auto_confirm: 0.5000",
            "an auto-confirm edge below the floor",
        ),
        (
            "target_far: 0.0500",
            "target_far: 0.0100",
            "a FAR budget finer than the sample's resolution",
        ),
        (
            "  0.6100 far=0.0500 frr=0.1000\n",
            "",
            "an operating point that never appears in the sweep it was read off",
        ),
        (
            "  0.6300 far=0.0000 frr=0.1000",
            "  0.6300 far=0.9000 frr=0.1000",
            "a sweep whose FAR rises with the threshold",
        ),
    ];
    for (from, to, why) in cases {
        let forged = worked_provenance().replace(from, to);
        assert_ne!(forged, worked_provenance(), "mutation `{from}` did not apply");
        let problems = match bands::parse_provenance(&forged) {
            Ok(p) => bands::verify_provenance(&p),
            Err(e) => vec![e],
        };
        assert!(
            !problems.is_empty(),
            "the verifier accepted {why}: `{from}` → `{to}`"
        );
    }
    // And a missing record is a failure, not an empty pass.
    assert!(bands::parse_provenance("MEASURED. trust me.").is_err());
}
