//! Matrix rows 12 and `12b` — **macOS older than 14.4**.
//!
//! Required behaviour (plan §6): "Notetaker surfaces are visible but disabled
//! with a plain sentence explaining the requirement. Mic-only meeting recording
//! can still be offered."
//!
//! That is two claims with two different truth values today, so it is two
//! cells, exactly as rows 5 and 17 already split:
//!
//!   * **12 — `Test`.** The gate runs. `meeting_asr::meeting_availability_for`
//!     is consulted by the shipping `notetaker_status` command for both capture
//!     modes, it refuses system audio below 14.4 with one sentence naming the
//!     requirement, and — the load-bearing half — it can never refuse mic-only
//!     recording, on any OS, which is what keeps 22-A's macOS 12 floor.
//!   * **`12b` — `Test` as of YV102 (#125).** The Settings step invokes
//!     `notetaker_status` on mount and renders `systemAudioMessage` under a
//!     disabled "Set up meeting recording" control, which is the plan's own
//!     wording for row 12 — visible, disabled, carrying the reason. It was
//!     `PolicyOnly` until that PR: the sentence was computed on every call and
//!     rendered by nothing, so a macOS 13 Mac showed nothing at all.
//!
//! This file is the matrix's own row, not a second copy of
//! `meeting_availability_144_gate.rs`: that file is YV101's exhaustive version
//! table, and this one asserts the two things the *matrix row* publishes — the
//! floor is intact, and the sentence reaches the surface that has to carry it.

use wilson_voice_lib::meeting_asr::{
    meeting_availability, meeting_availability_for, MeetingCapture, MeetingUnavailable,
};
use wilson_voice_lib::meeting_matrix::{Coverage, ROWS};
use wilson_voice_lib::os_version_gate::{self, OsVersion};
use wilson_voice_lib::NotetakerStatus;

/// The English model the shipped catalog carries, so the OS is the only axis
/// varying below.
const ENGLISH_MODEL: &str = "handy-computer/parakeet-unified-en-0.6b-gguf";

fn os(text: &str) -> OsVersion {
    OsVersion::parse(text).unwrap_or_else(|| panic!("'{text}' should parse"))
}

/// Row 12, positive half: below 14.4 the system-audio track is refused, and the
/// refusal is one sentence that names the requirement and this Mac.
#[test]
fn below_14_4_the_system_audio_track_is_refused_with_a_sentence_that_names_the_requirement() {
    for text in ["12.0", "13.0", "13.6", "14.0", "14.3"] {
        let verdict = meeting_availability_for(
            MeetingCapture::MicPlusSystemAudio,
            Some(ENGLISH_MODEL),
            Some("en"),
            os(text),
        );
        let Err(MeetingUnavailable::RequiresMacOS14_4 { found }) = verdict else {
            panic!("macOS {text} must not be offered a system-audio track: {verdict:?}");
        };
        assert_eq!(found, os(text));

        let sentence = MeetingUnavailable::RequiresMacOS14_4 { found }.message();
        assert!(
            sentence.contains("14.4"),
            "the sentence must name the requirement: {sentence}"
        );
        assert!(
            sentence.contains(text),
            "…and this Mac's own version, so it is an explanation rather than a policy: {sentence}"
        );
        assert!(
            sentence.contains("microphone"),
            "…and what still works, because this refusal is the one with no next step on the \
             machine: {sentence}"
        );
    }
}

/// **Row 12's load-bearing half.** The gate must never touch mic-only
/// recording, on any OS — including an OS version that could not be read at
/// all. A gate that leaked here would turn "system audio needs a newer Mac"
/// into "meetings do not work on your Mac" for every pre-14.4 user, with a
/// green build and no error anywhere.
#[test]
fn mic_only_recording_is_never_gated_by_this_row() {
    let versions = [
        "12.0", "13.0", "13.6", "14.0", "14.3", "14.4", "14.10", "15.0", "26.0",
    ];
    for text in versions {
        assert_eq!(
            meeting_availability_for(
                MeetingCapture::MicOnly,
                Some(ENGLISH_MODEL),
                Some("en"),
                os(text)
            ),
            Ok(()),
            "22-A mic-only recording must not regress on macOS {text}"
        );
    }
    assert_eq!(
        meeting_availability_for(
            MeetingCapture::MicOnly,
            Some(ENGLISH_MODEL),
            Some("en"),
            OsVersion::UNKNOWN
        ),
        Ok(()),
        "an unreadable OS version fails closed for the TAP, never for the microphone"
    );
    // The convenience door the rest of the app calls is the mic-only one, so it
    // cannot produce this refusal on the machine running the suite either —
    // whatever that machine's OS turns out to be.
    let here = meeting_availability(Some(ENGLISH_MODEL), Some("en"));
    assert!(
        !matches!(here, Err(MeetingUnavailable::RequiresMacOS14_4 { .. })),
        "`meeting_availability` is the mic-only door and must never carry the tap's gate: \
         {here:?} on macOS {}",
        OsVersion::current()
    );
}

/// At and above the floor the gate stops being the reason for anything — so a
/// refusal on 14.4+ is a model or language problem, and says so.
#[test]
fn at_and_above_14_4_the_gate_is_no_longer_the_answer() {
    for text in ["14.4", "14.5", "14.10", "15.0", "26.0"] {
        assert_eq!(
            meeting_availability_for(
                MeetingCapture::MicPlusSystemAudio,
                Some(ENGLISH_MODEL),
                Some("en"),
                os(text)
            ),
            Ok(()),
            "macOS {text} clears the tap floor"
        );
        // No model: still refused, but for the honest reason.
        assert_eq!(
            meeting_availability_for(
                MeetingCapture::MicPlusSystemAudio,
                None,
                Some("en"),
                os(text)
            ),
            Err(MeetingUnavailable::NoModel)
        );
    }
}

/// The requirement text is declared once, in `os_version_gate`, and the
/// Notetaker's sentence quotes it — so the published row and the running app
/// cannot drift the way two copies of the 3 h cap once could.
#[test]
fn the_requirement_is_declared_in_exactly_one_place() {
    let sentence = MeetingUnavailable::RequiresMacOS14_4 { found: os("13.6") }.message();
    assert!(
        sentence.contains(os_version_gate::SYSTEM_AUDIO_REQUIREMENT),
        "the availability sentence must quote `os_version_gate::SYSTEM_AUDIO_REQUIREMENT` rather \
         than carry its own copy of the number: {sentence}"
    );
}

/// Every `.ts`/`.tsx`/`.js` under `desktop/src` whose text contains `needle`.
fn frontend_files_containing(needle: &str) -> Vec<String> {
    let web = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("desktop/")
        .join("src");
    let mut hits = Vec::new();
    let mut stack = vec![web];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read desktop/src").flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .is_some_and(|e| e == "ts" || e == "tsx" || e == "js")
                && std::fs::read_to_string(&path)
                    .map(|b| b.contains(needle))
                    .unwrap_or(false)
            {
                hits.push(path.display().to_string());
            }
        }
    }
    hits
}

/// **Row `12b`, the half YV102 closed: the sentence has a surface.**
///
/// This test was the absence tripwire — "`system_audio_setup` does not exist in
/// `src/`, no frontend file invokes `notetaker_status`, so on a macOS 13 Mac
/// the app shows nothing at all rather than a disabled control with a reason".
/// #125 wired both, the tripwire fired, and this is the promotion it demanded:
/// the row now asserts the shipping surface performs the behaviour.
///
/// The behaviour, in the plan's own words for row 12: *the Notetaker surface is
/// visible but disabled with a plain sentence explaining the requirement*.
#[test]
fn the_sentence_reaches_the_settings_step_on_every_pre_14_4_mac() {
    // 1. The payload the surface reads. `NotetakerStatus::for_os` is what the
    //    shipping `notetaker_status` command returns — the command is a
    //    settings read plus this call, so this is the decision itself and not a
    //    restatement of it.
    for text in ["12.0", "13.0", "13.6", "14.3"] {
        let status = NotetakerStatus::for_os(ENGLISH_MODEL, "en", os(text));
        assert!(
            !status.system_audio_available,
            "macOS {text} cannot hold the system-audio permission at all"
        );
        let sentence = status
            .system_audio_message
            .as_deref()
            .expect("a refusal owes the surface its sentence");
        assert!(
            sentence.contains(os_version_gate::SYSTEM_AUDIO_REQUIREMENT),
            "{sentence}"
        );
        // …and the OTHER field says meetings still record, which is the half a
        // collapsed boolean would destroy.
        assert!(
            status.available,
            "mic-only recording is not gated on macOS {text}"
        );
        assert_eq!(status.message, None);
    }
    let modern = NotetakerStatus::for_os(ENGLISH_MODEL, "en", os("14.4"));
    assert!(modern.system_audio_available);
    assert_eq!(modern.system_audio_message, None);

    // 2. The wire. The surface reads camelCase names off this payload, and a
    //    rename on either side is silent — the frontend would just see
    //    `undefined`, fall back to "available", and offer a permission that
    //    cannot exist. So the serialized keys are asserted, not assumed.
    let json = serde_json::to_value(NotetakerStatus::for_os(ENGLISH_MODEL, "en", os("13.6")))
        .expect("NotetakerStatus serializes");
    assert_eq!(json["systemAudioAvailable"], serde_json::json!(false));
    assert!(json["systemAudioMessage"]
        .as_str()
        .is_some_and(|s| s.contains(os_version_gate::SYSTEM_AUDIO_REQUIREMENT)));
    assert_eq!(json["available"], serde_json::json!(true));

    // 3. The surface. Something in the frontend has to actually ask for it and
    //    render it, or the two assertions above are a payload nobody reads —
    //    which is precisely the state this row was published as until now.
    //
    //    The needles are the QUOTED command name and the quoted state key, not
    //    the bare words: a bare-word scan is satisfied by a comment about the
    //    command and by a renamed invoke — `invoke("notetaker_status_v2")` still
    //    contains `notetaker_status` while the frontend asks the backend for a
    //    command that does not exist and silently keeps its default. That
    //    mutation was run against the bare-word version of this assertion and
    //    it stayed green, which is the only reason this comment exists.
    let callers = frontend_files_containing("\"notetaker_status\"");
    assert!(
        !callers.is_empty(),
        "no frontend file invokes `notetaker_status`, so the 14.4 sentence reaches no surface \
         again and row 12b is `PolicyOnly` once more"
    );
    let renderers = frontend_files_containing("systemAudioMessage");
    assert!(
        !renderers.is_empty(),
        "`notetaker_status` is invoked but its sentence is dropped on the floor: {callers:?}"
    );
    // The invoke and the render have to be reachable from the app's own tree,
    // not only from the dev preview page (which fakes its inputs by design).
    assert!(
        callers.iter().any(|f| f.ends_with("App.tsx")),
        "only {callers:?} asks for the gate — the shipping window does not, so a real macOS 13 \
         Mac still sees nothing"
    );

    // …and the step that carries it is DISABLED rather than merely present. The
    // rule lives in `meetings/systemAudio.ts` (`setupState`), whose unavailable
    // arm is the one thing this row is about: no button to press, and the
    // sentence plus what still works underneath it.
    let rule = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("desktop/")
            .join("src/meetings/systemAudio.ts"),
    )
    .expect("read src/meetings/systemAudio.ts");
    //
    // The slice is THAT arm, not the whole file. `setupState` has a second
    // `canRun: false` (the `unavailable` verdict), so a whole-file scan stays
    // green while the pre-14.4 gate becomes a pressable button that asks macOS
    // for a permission the OS has no concept of. That mutation was run, it
    // passed the file-wide version of this assertion, and this is the fix.
    let gate_arm = rule
        .split_once("if (!available) {")
        .expect("`setupState` must still branch on the 14.4 gate first")
        .1;
    let gate_arm = gate_arm
        .split_once("\n  }")
        .map(|(arm, _)| arm)
        .unwrap_or(gate_arm);
    assert!(
        gate_arm.contains("canRun: false"),
        "the pre-14.4 arm of `setupState` must leave the control visible and UNPRESSABLE — an \
         enabled button offers a permission this Mac cannot hold:\n{gate_arm}"
    );
    assert!(
        gate_arm.contains("requirement"),
        "…and it must render the sentence the backend sent rather than wording of its own:\n\
         {gate_arm}"
    );
    assert!(
        gate_arm.contains("Meeting notes still record your microphone"),
        "row 12's sentence has to say what still works, because it is the refusal with no next \
         step on the machine:\n{gate_arm}"
    );
}

/// The requirement sentence is declared in `os_version_gate` and quoted
/// everywhere else — including the frontend, which now has a surface that can
/// carry its own copy.
///
/// `App.tsx` holds one literal as the fallback for a `notetaker_status` that
/// never answers. A fallback is defensible; a fallback that drifts from the
/// constant is the two-copies failure this matrix exists to catch (the 3 h cap,
/// which shipped as `MEETING_HARD_CAP` twice). So every *user-visible string*
/// in the frontend that talks about the requirement must be the exact shipping
/// sentence.
///
/// Strings only, never comments: prose explaining the gate is documentation,
/// and a scan that read it would demand the constant be pasted into a comment
/// to stay green. Same exclusion `callsite.rs` and `matrix_coverage.rs` apply
/// in the other direction.
#[test]
fn the_frontend_never_carries_its_own_wording_of_the_requirement() {
    /// The double- and single-quoted string literals on one line.
    fn literals(line: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut current: Option<(char, String)> = None;
        let mut escaped = false;
        for c in line.chars() {
            match &mut current {
                Some((quote, buf)) => {
                    if escaped {
                        escaped = false;
                        buf.push(c);
                    } else if c == '\\' {
                        escaped = true;
                    } else if c == *quote {
                        out.push(std::mem::take(buf));
                        current = None;
                    } else {
                        buf.push(c);
                    }
                }
                None => {
                    if c == '"' || c == '\'' || c == '`' {
                        current = Some((c, String::new()));
                    }
                }
            }
        }
        out
    }

    let mut checked = 0;
    for file in frontend_files_containing("macOS 14.4") {
        // A `*.test.ts` describes the rule ("the macOS 14.4 gate outranks
        // everything"); it does not render to a user, and forcing the shipping
        // sentence into a test name would make the assertion meaningless.
        if file.contains(".test.") {
            continue;
        }
        let body = std::fs::read_to_string(&file).expect("read frontend file");
        for (n, line) in body.lines().enumerate() {
            for literal in literals(line) {
                if !literal.contains("macOS 14.4") {
                    continue;
                }
                checked += 1;
                assert!(
                    literal.contains(os_version_gate::SYSTEM_AUDIO_REQUIREMENT),
                    "{file}:{} ships its own wording of the 14.4 requirement: {literal:?}\n\
                     The shipping sentence is `os_version_gate::SYSTEM_AUDIO_REQUIREMENT` \
                     (\"{}\"), and a second copy here goes stale in silence — which is the row \
                     publishing one sentence while the app renders another.",
                    n + 1,
                    os_version_gate::SYSTEM_AUDIO_REQUIREMENT
                );
            }
        }
    }
    // Two today — `App.tsx`'s fallback for a `notetaker_status` that never
    // answers, and the dev preview page that renders all six states for the
    // screenshots. Both quote the constant, which is the rule; the count is not
    // pinned because a third legitimate surface is a normal thing to add and a
    // brittle number would only teach the next author to edit this line. Zero
    // is the failure worth catching: it means the check above swept nothing.
    assert!(
        checked > 0,
        "no frontend string mentions the 14.4 requirement at all, so this check proved nothing — \
         either the fallback was deleted (an unanswered gate now renders an empty sentence) or \
         the wording moved somewhere this scan cannot see"
    );
}

#[test]
fn the_published_cells_split_the_gate_from_the_surface_that_now_carries_it() {
    let gate = ROWS.iter().find(|r| r.id == "12").expect("row 12");
    assert_eq!(
        gate.coverage,
        Coverage::Test {
            test: "matrix_row12_macos_144_gate.rs",
            subject: "meeting_availability_for",
            subject_module: "meeting_asr.rs",
        }
    );

    // Still two cells, not one. The split was never about the surface being
    // missing — it is that the gate and the thing rendering it are different
    // claims, and rows 5 and 17 keep their splits for the same reason.
    let surface = ROWS.iter().find(|r| r.id == "12b").expect("row 12b");
    assert_eq!(
        surface.coverage,
        Coverage::Test {
            test: "matrix_row12_macos_144_gate.rs",
            subject: "NotetakerStatus",
            subject_module: "lib.rs",
        }
    );
    let cell = surface.coverage.cell();
    assert!(
        cell.contains("cargo test --test matrix_row12_macos_144_gate"),
        "{cell}"
    );
    assert!(
        !cell.contains("Policy only") && !cell.contains("NOT WIRED"),
        "{cell}"
    );
}
