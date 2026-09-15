//! PERM-D — the take that recorded NOTHING, and how Yap is required to answer it.
//!
//! The defect: a muted or unauthorized input does not deliver quiet audio, it
//! delivers arithmetic zero. That buffer used to fall into the YV16 no-speech
//! gate, which correctly refuses to paste and then says "Didn't catch any
//! speech — hold and speak". That is a sentence about the SPEAKER. The truth is
//! a sentence about the DEVICE (or the grant), and a first-run user with no
//! microphone permission was told they mumbled.
//!
//! Three properties are defended here, and each one is defended at the level it
//! can actually break at:
//!
//! * **Silent is not quiet** — a behavioural assertion on the pure classifier.
//!   A -60 dBFS whisper is a REAL take and must survive; only exact zero is
//!   silence. Turning this into a threshold would suppress genuine takes in a
//!   quiet room, which is a worse bug than the one being closed.
//! * **A silent take never reaches the paste path** — a structural assertion
//!   read out of `lib.rs` at compile time, the way `mic_gate.rs` and
//!   `license_gate.rs` do, because the property is about ORDER and about what
//!   one match arm is allowed to do. A second call site, or the gate moved
//!   below ASR, compiles and passes clippy and ships.
//! * **The grant is re-read at the END of the take** — revoking permission in
//!   System Settings mid-take is exactly the case that produces this buffer, so
//!   a status captured when the key went down reports the stale answer.
//!
//! Plus the usage strings. A missing `NS*UsageDescription` is an instant TCC
//! failure with NO dialog at all, the app simply never gets the microphone, and
//! no other test in this repo looks at `Info.plist`.

use wilson_voice_lib::dictation::{
    classify_take_audio, is_silent_capture, peak_amplitude, TakeAudioVerdict,
};
use wilson_voice_lib::mic_auth::MicAuth;

const LIB_RS: &str = include_str!("../src/lib.rs");
const INFO_PLIST: &str = include_str!("../Info.plist");

/// Code only — a comment mentioning `paste` is not a call to it.
fn code_of(line: &str) -> &str {
    line.split("//").next().unwrap_or("")
}

/// Index of the first line whose CODE contains `needle`.
fn line_of(needle: &str) -> usize {
    LIB_RS
        .lines()
        .position(|l| code_of(l).contains(needle))
        .unwrap_or_else(|| {
            panic!("lib.rs no longer contains `{needle}` — the gate moved or was renamed")
        })
}

// ───────────────────────────── silent is not quiet ─────────────────────────────

/// The whole point of the item, stated as the thing it must not do.
#[test]
fn all_zero_buffer_is_classified_silent_not_quiet() {
    // A muted / unauthorized input: digital silence, for the whole take.
    let muted = vec![0.0f32; 16_000];
    assert!(is_silent_capture(&muted), "an all-zero buffer IS silence");
    assert_eq!(peak_amplitude(&muted), 0.0);

    // A whisper in a treated room: -60 dBFS is ~0.001 full scale. This is a
    // REAL take. If this assertion ever flips, someone turned exact zero into a
    // threshold and quiet speakers stopped being able to dictate.
    let whisper: Vec<f32> = (0..16_000)
        .map(|i| 0.001 * (i as f32 * 0.05).sin())
        .collect();
    assert!(peak_amplitude(&whisper) > 0.0);
    assert!(
        peak_amplitude(&whisper) < 0.002,
        "fixture must really be ~-60 dBFS, not a loud take"
    );
    assert!(
        !is_silent_capture(&whisper),
        "-60 dBFS is quiet, NOT silent — a whisper is a real take"
    );
    assert_eq!(
        classify_take_audio(&whisper, MicAuth::Authorized),
        TakeAudioVerdict::Usable
    );

    // One non-zero sample anywhere in an otherwise dead buffer is still capture.
    let mut almost = vec![0.0f32; 16_000];
    almost[9_999] = f32::MIN_POSITIVE;
    assert!(
        !is_silent_capture(&almost),
        "exactly zero means EXACTLY zero across the whole buffer"
    );

    // `-0.0` is the same silence as `0.0`.
    assert!(is_silent_capture(&[-0.0f32; 64]));

    // An empty buffer is not a silent CAPTURE — there is nothing to diagnose,
    // and that path belongs to the device-failure gate above it.
    assert!(!is_silent_capture(&[]));
}

// ──────────────────────── never reaches the paste path ────────────────────────

/// The gate is asked BEFORE ASR, and before the no-speech gate that used to
/// swallow this case. Everything downstream of ASR — the transcript row, the
/// clipboard, `⌘V` — is therefore unreachable for a silent take by construction
/// rather than by an arm that happens not to call them today.
#[test]
fn silent_capture_never_reaches_the_paste_path() {
    let gate = line_of("classify_take_audio");
    let no_speech = line_of("has_enough_speech(rec.voiced_seconds)");
    let asr = line_of("let asr = transcribe_native(");
    let insert = line_of("db.insert_transcript_at(");

    assert!(
        gate < no_speech,
        "the silent gate must be asked BEFORE the no-speech gate (gate line {gate}, no-speech line {no_speech}) — below it, silence is reported as a user who mumbled"
    );
    assert!(
        gate < asr,
        "the silent gate must be asked BEFORE ASR (gate line {gate}, asr line {asr})"
    );
    assert!(
        gate < insert,
        "the silent gate must be asked BEFORE the transcript row is written"
    );

    // And the handling arm itself writes nothing and pastes nothing. Read the
    // arm's body out of the file rather than trusting the review that landed it.
    // NOTE: match the ARM, not the `return Ok(TakeOutcome::SilentCapture {` up
    // in the gate — that one appears first in the file.
    let arm = arm_body("Ok(TakeOutcome::SilentCapture {");
    for forbidden in [
        "insert_transcript",
        "paste::",
        "PASTE_OUTCOME_EVENT",
        "PASTE_FAILED_EVENT",
        "emit(\"transcript\"",
    ] {
        assert!(
            !arm.contains(forbidden),
            "the SilentCapture arm must never reach `{forbidden}` — a take that recorded nothing has nothing to paste and nothing to store"
        );
    }

    // The clip is KEPT, under the existing recovery-dir lifecycle. Deleting it
    // would destroy the one artefact showing what the device delivered, and
    // every gate can be wrong.
    assert!(
        arm.contains("keep_failed_take(") && arm.contains("recovery_dir()"),
        "a silent take must preserve its clip via keep_failed_take(&db, &recovery_dir(), …), not unlink it"
    );
    assert!(
        arm.contains("TAKE_FAILED_EVENT") && arm.contains("SILENT_CAPTURE_CODE"),
        "the arm must emit take_failed with the silent_capture code"
    );
    assert!(
        arm.contains("TAKE_DONE_EVENT"),
        "the arm must still emit the terminal take marker, or onboarding spins to its watchdog"
    );
}

/// Body of a `match` arm, from its head line to the line that closes it at the
/// same indentation. Crude on purpose: it reads what is actually in the file.
fn arm_body(head: &str) -> String {
    let lines: Vec<&str> = LIB_RS.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.trim_start().starts_with(head))
        .unwrap_or_else(|| panic!("lib.rs no longer has a `{head}` match arm"));
    let indent = lines[start].len() - lines[start].trim_start().len();
    let close = format!("{}}}", " ".repeat(indent));
    let end = lines[start + 1..]
        .iter()
        .position(|l| *l == close)
        .map(|i| start + 1 + i)
        .expect("SilentCapture arm never closes");
    lines[start..=end]
        .iter()
        .map(|l| code_of(l))
        .collect::<Vec<_>>()
        .join("\n")
}

// ───────────────────────── the grant is re-read, at the end ─────────────────────────

/// A silent buffer means something different depending on the grant, and the
/// grant that matters is the one at take END.
#[test]
fn silent_capture_rechecks_authorization() {
    let muted = vec![0.0f32; 8_000];

    // Authorized: the permission is fine, so the input device (or its mute
    // switch) is the suspect and the permission screen would be an errand.
    assert_eq!(
        classify_take_audio(&muted, MicAuth::Authorized),
        TakeAudioVerdict::Silent {
            needs_permission: false
        }
    );

    // Anything else: macOS is the reason the buffer is empty, so the permission
    // screen is the honest surface.
    for status in [MicAuth::Denied, MicAuth::Restricted, MicAuth::NotDetermined] {
        assert_eq!(
            classify_take_audio(&muted, status),
            TakeAudioVerdict::Silent {
                needs_permission: true
            },
            "{} must send the user to the permission screen",
            status.as_str()
        );
    }

    // Structural half: the status fed to that classifier is read at the END of
    // the take, after the clip guard is handed over — not reused from the press.
    let handoff = line_of("pending = Some((rec.clip");
    let reread = line_of("let mic_status_at_take_end = mic_auth::authorization_status();");
    let gate = line_of("classify_take_audio");
    assert!(
        handoff < reread && reread <= gate,
        "the grant must be re-read at take end (clip handoff line {handoff}, re-read line {reread}, gate line {gate})"
    );
}

// ─────────────────────────────── the usage strings ───────────────────────────────

/// Verified present at 4e8c9adf and never to be removed. Without one of these
/// macOS denies the capability with NO dialog at all — the app just never gets
/// the device, which is indistinguishable from the bug this item diagnoses.
#[test]
fn info_plist_keeps_every_tcc_usage_string() {
    for key in [
        "NSMicrophoneUsageDescription",
        "NSAudioCaptureUsageDescription",
        "NSAppleEventsUsageDescription",
    ] {
        assert!(
            INFO_PLIST.contains(key),
            "Info.plist lost {key} — that is an instant TCC failure with no dialog"
        );
        // A key with an empty string is the same failure with extra steps.
        let after = INFO_PLIST
            .split_once(key)
            .map(|(_, rest)| rest)
            .unwrap_or("");
        let value = after
            .split_once("<string>")
            .and_then(|(_, r)| r.split_once("</string>"))
            .map(|(v, _)| v.trim())
            .unwrap_or("");
        assert!(
            value.len() > 10,
            "{key} must carry a real sentence, got {value:?}"
        );
    }
}
