//! Y3-F — the dictation take has a DECLARED maximum length.
//!
//! Before this item `git grep "MAX_TAKE\|max_take\|MAX_RECORD\|session_len"`
//! over `desktop/` returned nothing functional: there was no ceiling, which is
//! not generosity. It means the failure mode at some unknown length is a
//! timeout or an OOM — a cliff found by falling off it — rather than a number
//! chosen, said out loud before it arrives, and tested here.
//!
//! The four claims, one test each:
//!
//! 1. the take STOPS at the ceiling and transcribes what it has;
//! 2. the warning fires ONCE, at 80%;
//! 3. the cut prefers a VAD boundary inside the grace window;
//! 4. the meeting cap and the dictation cap are SEPARATE constants, so
//!    tightening one can never silently tighten the other.
//!
//! Plus the fifth that keeps the other four honest: the declared number is a
//! measurement. `the_ceiling_is_a_measurement_not_a_guess` re-runs Y3-A's
//! capture probe at `MAX_SESSION_SECONDS` itself, so the day someone raises the
//! ceiling they re-measure capture at the new length or this file goes red.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use wilson_voice_lib::capture_probe::CaptureProbe;
use wilson_voice_lib::meeting::{MEETING_CAP_WARN_AT, MEETING_HARD_CAP};
use wilson_voice_lib::{
    cap_cut_index, max_session_samples, session_cap_tick, session_cap_warning, SessionCapAction,
    CUT_GRACE, MAX_RESIDENT_CAPTURE_BYTES, MAX_SESSION, MAX_SESSION_SECONDS, SESSION_WARN_AT,
    SESSION_WARN_PERCENT, TARGET_RATE,
};

fn src(file: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(file);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// `seconds` of a two-tone sweep at [`TARGET_RATE`], loud enough that the energy
/// VAD calls every frame voiced.
fn speech(seconds: f64) -> Vec<f32> {
    let n = (TARGET_RATE as f64 * seconds) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / TARGET_RATE as f32;
            (t * 220.0 * std::f32::consts::TAU).sin() * 0.35
                + (t * 1_400.0 * std::f32::consts::TAU).sin() * 0.15
        })
        .collect()
}

/// `seconds` of true silence.
fn quiet(seconds: f64) -> Vec<f32> {
    vec![0.0; (TARGET_RATE as f64 * seconds) as usize]
}

// ── 1. the ceiling stops the take, and the take is still transcribed ────────

/// At the ceiling the rule says STOP — and the stop is the ordinary end of a
/// take, not an error and not a discard.
///
/// The three halves of that claim, because "it compiles" proves none of them:
///
/// * the RULE returns `Stop` at the ceiling and only at the ceiling;
/// * the AUDIO survives it — `cap_cut_index` on an over-length take returns a
///   cut that is inside the take and far from zero, so what reaches ASR is the
///   recording rather than nothing;
/// * the rule is WIRED — `lib.rs` answers `SessionCapAction::Stop` by calling
///   `stop_and_transcribe`, the same function the hotkey release calls, and
///   `record::stop_recording` applies `cap_cut_index` to the finalized samples.
///   A pure function nothing calls is the defect this check exists to catch.
#[test]
fn take_stops_at_the_ceiling_and_transcribes_what_it_has() {
    // Below the ceiling nothing stops, even one tick short of it.
    assert_eq!(
        session_cap_tick(MAX_SESSION - Duration::from_millis(50), true),
        SessionCapAction::Continue
    );
    // At it, and past it, it stops — and `Stop` outranks a warning that never
    // fired, so a take that reached the ceiling un-warned still stops.
    for elapsed in [MAX_SESSION, MAX_SESSION + Duration::from_secs(90)] {
        assert_eq!(session_cap_tick(elapsed, false), SessionCapAction::Stop);
        assert_eq!(session_cap_tick(elapsed, true), SessionCapAction::Stop);
    }

    // The audio is kept, not dropped. A take that overran by three seconds of
    // unbroken speech comes back cut at the ceiling — every sample before it.
    let ceiling = 8 * TARGET_RATE as usize;
    let over = speech(11.0);
    let cut = cap_cut_index(&over, TARGET_RATE, ceiling, CUT_GRACE);
    assert_eq!(
        cut, ceiling,
        "unbroken speech through the ceiling cuts AT the ceiling"
    );
    assert!(cut > 0, "a capped take must never come back empty");

    // A take that fits is never touched.
    let under = speech(3.0);
    assert_eq!(
        cap_cut_index(&under, TARGET_RATE, ceiling, CUT_GRACE),
        under.len(),
        "a take under the ceiling is not cut at all"
    );

    // WIRING. Without these the three assertions above measure a function the
    // app never reaches.
    let record_rs = src("record.rs");
    let stop_body = record_rs
        .split("pub fn stop_recording(")
        .nth(1)
        .expect("stop_recording still exists");
    assert!(
        stop_body.contains("cap_cut_index"),
        "stop_recording must apply the ceiling cut to the finalized samples"
    );
    let lib_rs = src("lib.rs");
    assert!(
        lib_rs.contains("SessionCapAction::Stop"),
        "lib.rs must handle the Stop action"
    );
    let stop_arm = lib_rs
        .split("SessionCapAction::Stop =>")
        .nth(1)
        .expect("the Stop arm exists")
        .split("record::SessionCapAction")
        .next()
        .unwrap_or_default();
    assert!(
        stop_arm.contains("stop_and_transcribe"),
        "the ceiling must end the take through the SAME stop-and-transcribe path \
         the hotkey uses — not an error, not a discard. Arm was:\n{stop_arm}"
    );
}

// ── 2. one warning, at 80% ──────────────────────────────────────────────────

/// The warning fires at 80% of the ceiling, and exactly once.
///
/// "Once" is the half worth a test. The watchdog rides the HUD thread, which
/// ticks ~20 times a second, so a rule without the `warned` latch would repeat
/// the notice roughly seven thousand times over the last fifth of a take.
#[test]
fn warning_fires_once_at_eighty_percent() {
    // The threshold is DERIVED, never hand-written — it cannot drift away from
    // the ceiling when the ceiling moves.
    assert_eq!(SESSION_WARN_PERCENT, 80);
    assert_eq!(
        SESSION_WARN_AT,
        Duration::from_secs(MAX_SESSION_SECONDS * 80 / 100)
    );
    assert!(SESSION_WARN_AT < MAX_SESSION);

    // One tick early: nothing.
    assert_eq!(
        session_cap_tick(SESSION_WARN_AT - Duration::from_millis(50), false),
        SessionCapAction::Continue
    );
    // At the threshold, un-warned: the notice.
    assert_eq!(
        session_cap_tick(SESSION_WARN_AT, false),
        SessionCapAction::Warn
    );

    // And then never again. Walk the whole remaining fifth of the take at HUD
    // cadence with the latch set; not one of those ticks may warn.
    let mut elapsed = SESSION_WARN_AT;
    let tick = Duration::from_millis(50);
    let mut warns = 0usize;
    let mut ticks = 0usize;
    while elapsed < MAX_SESSION {
        if session_cap_tick(elapsed, true) == SessionCapAction::Warn {
            warns += 1;
        }
        elapsed += tick;
        ticks += 1;
    }
    assert!(ticks > 1_000, "the walk must be long enough to be a test");
    assert_eq!(
        warns, 0,
        "the latch must silence every tick after the first"
    );

    // The copy is calm and says what is coming, not just that something is.
    let msg = session_cap_warning();
    assert!(msg.contains("stops and transcribes"), "{msg}");
    assert!(
        msg.contains(&format!("{}", MAX_SESSION_SECONDS / 60)),
        "{msg}"
    );
    assert!(!msg.contains('!'), "a calm line does not shout: {msg}");

    // WIRING: the notice rides the status line the pill already renders, and
    // the latch is what `status_message` reads — no second channel, no toast.
    let lib_rs = src("lib.rs");
    assert!(
        lib_rs.contains("session_cap_warning()"),
        "the warning copy must reach the status line"
    );
    assert!(
        lib_rs.contains("*state.session_cap_warned.lock()"),
        "build_status must pass the latch into status_message"
    );
}

// ── 3. the cut lands on a VAD boundary when one is close ────────────────────

/// A take that hits the ceiling mid-word backs up to the last silence, if one
/// began within [`CUT_GRACE`] of the ceiling.
///
/// `vad.rs` + the bridged 20 ms voiced mask already decide what silence is on
/// every take; this reuses that answer rather than inventing a second one.
#[test]
fn cut_prefers_a_vad_boundary_within_the_grace_window() {
    let sr = TARGET_RATE;
    let ceiling = 10 * sr as usize;

    // The speaker finished a phrase at 9.0 s, left 0.6 s of silence, then
    // started another word at 9.6 s that is still going at the ceiling. The
    // honest cut is the start of that silence, not the middle of the new word.
    let mut take = speech(9.0);
    take.extend(quiet(0.6));
    take.extend(speech(2.4)); // runs past 10 s
    assert!(take.len() > ceiling);

    let cut = cap_cut_index(&take, sr, ceiling, CUT_GRACE);
    let grace_samples = (CUT_GRACE.as_secs_f64() * sr as f64) as usize;
    assert!(
        cut < ceiling,
        "with a boundary in the grace window the cut must back up: cut={cut} ceiling={ceiling}"
    );
    assert!(
        cut >= ceiling - grace_samples,
        "the cut may never back up further than the grace window: cut={cut}"
    );
    let cut_secs = cut as f64 / sr as f64;
    assert!(
        (8.8..=9.7).contains(&cut_secs),
        "the cut should land in the 9.0–9.6 s silence, landed at {cut_secs:.2}s"
    );

    // The negative case, which is what makes the positive one mean anything:
    // with the speaker talking straight through the window there is no boundary
    // to prefer, and truncating the fragment beats discarding the take.
    let unbroken = speech(13.0);
    assert_eq!(
        cap_cut_index(&unbroken, sr, ceiling, CUT_GRACE),
        ceiling,
        "no boundary in the window means cut exactly at the ceiling"
    );

    // A silence that ended LONG before the ceiling is not in the window and is
    // not a candidate — backing up to it would throw away seconds of speech.
    let mut early_gap = speech(2.0);
    early_gap.extend(quiet(0.8));
    early_gap.extend(speech(11.0));
    assert_eq!(
        cap_cut_index(&early_gap, sr, ceiling, CUT_GRACE),
        ceiling,
        "a boundary outside the grace window must be ignored"
    );

    // And the real ceiling is the one the app uses.
    assert_eq!(
        max_session_samples(),
        MAX_SESSION_SECONDS as usize * TARGET_RATE as usize
    );
}

// ── 4. two caps, two constants ──────────────────────────────────────────────

/// The meeting cap and the dictation cap are separate, and stay separate.
///
/// A meeting is explicitly NOT subject to the dictation ceiling — it runs to
/// `meeting::MEETING_HARD_CAP` (3 h), enforced by `meeting::watchdog_tick` and
/// covered by `tests/matrix_row17_meeting_cap.rs`. The failure this guards is
/// the cheap one: somebody "unifies" the two caps, or expresses one in terms of
/// the other, and tightening the dictation ceiling silently truncates every
/// meeting — or a meeting's three hours silently becomes the dictation ceiling
/// and the declared 30 minutes stops being true.
#[test]
fn meeting_cap_and_dictation_cap_are_separate_constants() {
    // Different values, different orders of magnitude, and the dictation cap is
    // the smaller one.
    assert_ne!(MAX_SESSION, MEETING_HARD_CAP);
    assert!(MAX_SESSION < MEETING_HARD_CAP);
    assert_eq!(MEETING_HARD_CAP, Duration::from_secs(3 * 60 * 60));
    assert_eq!(MAX_SESSION, Duration::from_secs(30 * 60));
    // Their warnings are separate too, and neither is derived from the other.
    assert_ne!(SESSION_WARN_AT, MEETING_CAP_WARN_AT);

    // Declared in DIFFERENT files, each owning its own number.
    let record_rs = src("record.rs");
    let meeting_rs = src("meeting.rs");
    assert!(
        record_rs.contains("pub const MAX_SESSION_SECONDS"),
        "the dictation ceiling is declared in record.rs"
    );
    assert!(
        meeting_rs.contains("pub const MEETING_HARD_CAP"),
        "the meeting cap is declared in meeting.rs"
    );

    // Neither file expresses its cap in terms of the other's. This is the
    // assertion that actually blocks the merge of the two: a `MAX_SESSION_SECONDS`
    // written as a fraction of `MEETING_HARD_CAP` would pass every value check
    // above and still couple them.
    assert!(
        !record_rs.contains("MEETING_HARD_CAP") && !record_rs.contains("MEETING_CAP_WARN_AT"),
        "record.rs must not reach for the meeting cap"
    );
    assert!(
        !meeting_rs.contains("MAX_SESSION_SECONDS") && !meeting_rs.contains("SESSION_WARN_AT"),
        "meeting.rs must not reach for the dictation ceiling"
    );

    // The meeting path is enforced by its OWN rule, which knows nothing about
    // this one: `session_cap_tick` is never consulted for a meeting.
    assert!(
        !meeting_rs.contains("session_cap_tick"),
        "a meeting is not subject to the dictation ceiling"
    );
}

// ── 5. the declared number is a measurement ─────────────────────────────────

/// The ceiling is chosen from what the capture path actually holds, at the
/// ceiling's own length — not at a fixture length that happens to be shorter.
///
/// Y3-A (`2ef8f0c`) measured a synthetic TWENTY-minute take holding under
/// `MAX_RESIDENT_CAPTURE_BYTES`. That is why `MAX_SESSION_SECONDS` is thirty
/// minutes rather than Wispr's declared twenty: capture is flat in the length of
/// the take, so twenty was a fixture, not a limit. This test moves the fixture
/// to the declared ceiling, which is what stops the doc comment from being a
/// story — raise the ceiling and this re-measures at the new number or fails.
#[test]
fn the_ceiling_is_a_measurement_not_a_guess() {
    let dir = std::env::temp_dir().join(format!("yap-max-session-{}", std::process::id()));
    let recovery = dir.join("recovery");
    fs::create_dir_all(&recovery).expect("recovery dir");
    let wav = dir.join("ceiling.wav");
    let mut probe = CaptureProbe::new(TARGET_RATE, 1, Some((wav.as_path(), recovery.as_path())))
        .expect("the temp volume has room for a capture spill");

    let source = speech(2.0);
    let callback = 1_024usize;
    let callbacks = max_session_samples() / callback;
    let mut peak = probe.resident_bytes();
    let mut cursor = 0usize;
    for _ in 0..callbacks {
        if cursor + callback > source.len() {
            cursor = 0;
        }
        probe.push(&source[cursor..cursor + callback]);
        cursor += callback;
        let now = probe.resident_bytes();
        if now > peak {
            peak = now;
        }
    }
    let out = probe
        .finish(false)
        .expect("a ceiling-length take finalizes");
    let _ = fs::remove_dir_all(&dir);

    assert!(
        peak < MAX_RESIDENT_CAPTURE_BYTES,
        "a {MAX_SESSION_SECONDS}s take held {peak} bytes resident during capture; \
         the Y3-A ceiling is {MAX_RESIDENT_CAPTURE_BYTES}"
    );
    // And the whole recording is still there at the end — a bounded capture that
    // lost audio would satisfy the byte ceiling and fail the user.
    assert!(
        out.len() as f64 > max_session_samples() as f64 * 0.95,
        "a ceiling-length take must still contain the whole recording, got {} of {} samples",
        out.len(),
        max_session_samples()
    );

    // The read-back is the one fully resident copy, and its size is arithmetic,
    // not a guess: 4 bytes per 16 kHz mono sample.
    let read_back_bytes = out.len() * std::mem::size_of::<f32>();
    assert_eq!(read_back_bytes / out.len(), 4);
    eprintln!(
        "MEASURED @ {MAX_SESSION_SECONDS}s ceiling: capture peak {peak} B \
         (ceiling {MAX_RESIDENT_CAPTURE_BYTES} B), finalize read-back {read_back_bytes} B \
         ({:.0} MB)",
        read_back_bytes as f64 / 1_048_576.0
    );
}
