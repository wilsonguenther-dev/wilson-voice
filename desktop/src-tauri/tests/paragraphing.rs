//! Y4-C — paragraphing: long speech becomes paragraphs BY RULE.
//!
//! Before this item the rules stage had no paragraph rule at all. The only thing
//! that could produce a blank line was the spoken COMMAND ("new paragraph",
//! R2), so a five-minute dictation arrived as one unbroken block.
//!
//! Every case here runs the REAL signal: `dictation::PauseSpan`s, positioned as a
//! share of the take's voiced time, exactly as `record::pause_spans` emits them
//! from the bridged energy-VAD mask. `pause_at` below converts "the speaker
//! stopped right here in the text" into that fraction, which is the same
//! arithmetic under the assumption the rule itself is built on — a roughly
//! constant speaking rate across one take.

use wilson_voice_lib::dictation::{
    self, CleanupLevel, DictationMode, PauseSpan, Style, PARAGRAPH_PAUSE_SECONDS,
};

const S1: &str = "The migration landed last night and everything is green.";
const S2: &str =
    "We moved the whole history table over in about forty minutes and nothing needed a manual fix.";
const S3: &str = "So separately I want to talk about the pricing page.";
const S4: &str = "The current copy still promises a free tier we removed in June and support has had three tickets about it this week.";

/// The four-sentence take, as one block — what the user sees today.
fn take() -> String {
    format!("{S1} {S2} {S3} {S4}")
}

/// A pause the speaker left immediately after `needle`, expressed the way
/// capture expresses it: a duration plus the share of the take's voiced time
/// already spoken. Whitespace is collapsed first, which is the rule's own
/// coordinate system.
fn pause_at(text: &str, needle: &str, seconds: f64) -> PauseSpan {
    let flat: String = {
        let mut out = String::new();
        let mut in_ws = false;
        for c in text.chars() {
            if c.is_whitespace() {
                if !in_ws {
                    out.push(' ');
                }
                in_ws = true;
            } else {
                out.push(c);
                in_ws = false;
            }
        }
        out
    };
    let end = flat.find(needle).expect("needle must occur in the take") + needle.chars().count();
    PauseSpan {
        seconds,
        at_voiced_fraction: end as f64 / flat.chars().count() as f64,
    }
}

fn paragraphed(text: &str, pauses: &[PauseSpan], mode: DictationMode) -> String {
    dictation::insert_paragraphs(text, pauses, mode, Style::Default)
}

// ---------------------------------------------------------------------------
// The primary signal: silence.
// ---------------------------------------------------------------------------

/// A long pause that lands at a sentence boundary IS a paragraph break.
#[test]
fn pause_at_a_sentence_boundary_breaks() {
    let text = take();
    let pauses = [pause_at(&text, "a manual fix.", 1.4)];
    let out = paragraphed(&text, &pauses, DictationMode::Document);

    assert_eq!(out, format!("{S1} {S2}\n\n{S3} {S4}"));
    // The break is a BLANK line, not a bare newline, and nothing else moved.
    assert_eq!(out.matches("\n\n").count(), 1);
    assert_eq!(
        out.split_whitespace().collect::<Vec<_>>(),
        text.split_whitespace().collect::<Vec<_>>(),
        "paragraphing is additive — it may never change a word"
    );
}

/// The SAME pause, mid-clause, does not. There is no sentence boundary within
/// the projection window, so the rule produces nothing rather than cutting a
/// sentence in half.
#[test]
fn pause_mid_clause_does_not_break() {
    let text = take();
    let pauses = [pause_at(&text, "we removed in June", 1.4)];
    assert_eq!(paragraphed(&text, &pauses, DictationMode::Document), text);
}

/// A pause under the threshold is a speaker breathing, not a paragraph — even
/// sitting exactly on a sentence boundary.
#[test]
fn a_short_pause_at_a_boundary_does_not_break() {
    let text = take();
    let pauses = [pause_at(
        &text,
        "a manual fix.",
        PARAGRAPH_PAUSE_SECONDS - 0.2,
    )];
    assert_eq!(paragraphed(&text, &pauses, DictationMode::Document), text);
}

// ---------------------------------------------------------------------------
// Mode. Chat is a correctness case, not a taste call.
// ---------------------------------------------------------------------------

/// A paragraph break in Slack, Discord, Messages or Telegram SENDS the message.
/// No pause, of any length, may put one there.
#[test]
fn chat_mode_never_inserts_a_break() {
    let text = take();
    let pauses = [
        pause_at(&text, "a manual fix.", 4.0),
        pause_at(&text, "the pricing page.", 4.0),
    ];
    let out = paragraphed(&text, &pauses, DictationMode::Chat);
    assert_eq!(out, text);
    assert!(
        !out.contains('\n'),
        "a chat message is one block: a newline here would fire it off mid-thought"
    );
    // The marker fallback is gated by the same rule, with no timing at all.
    assert_eq!(paragraphed(&text, &[], DictationMode::Chat), text);
}

/// Email gets paragraphs, with a blank line between them.
#[test]
fn email_mode_gets_a_blank_line_between_paragraphs() {
    let text = take();
    let pauses = [pause_at(&text, "a manual fix.", 1.4)];
    let out = paragraphed(&text, &pauses, DictationMode::Email);
    assert_eq!(out, format!("{S1} {S2}\n\n{S3} {S4}"));
}

/// `Code` and `Plain` stay verbatim, like every other rule in the stage.
#[test]
fn verbatim_modes_are_untouched() {
    let text = take();
    let pauses = [pause_at(&text, "a manual fix.", 4.0)];
    assert_eq!(paragraphed(&text, &pauses, DictationMode::Code), text);
    assert_eq!(paragraphed(&text, &pauses, DictationMode::Plain), text);
}

// ---------------------------------------------------------------------------
// The user's own breaks, and running twice.
// ---------------------------------------------------------------------------

/// A break the user dictated with "new paragraph" (R2) survives untouched and is
/// never widened into four newlines.
#[test]
fn explicit_new_paragraph_is_preserved_and_not_doubled() {
    let text = format!("{S1} {S2}\n\n{S3} {S4}");
    let pauses = [pause_at(&text, "a manual fix.", 4.0)];
    let out = paragraphed(&text, &pauses, DictationMode::Document);

    assert_eq!(out, text, "the user's own break is not re-cut");
    assert!(!out.contains("\n\n\n"));
    assert_eq!(out.matches("\n\n").count(), 1);
}

/// Running the rule twice produces the same string, byte for byte.
#[test]
fn idempotent_on_second_application() {
    let text = take();
    let pauses = [pause_at(&text, "a manual fix.", 1.4)];
    let once = paragraphed(&text, &pauses, DictationMode::Document);
    let twice = paragraphed(&once, &pauses, DictationMode::Document);
    assert_eq!(once, twice);
    assert_eq!(twice.matches("\n\n").count(), 1);
}

// ---------------------------------------------------------------------------
// The fallback, and its guard.
// ---------------------------------------------------------------------------

/// Discourse markers are the SECONDARY signal and fire ONLY for a take with no
/// timing at all — a recovered spill, an imported clip, a History retry. With
/// timing present, measured silence decides, and a take whose pauses are all
/// short is a take the speaker did not break, whatever their speech habits.
#[test]
fn marker_fallback_only_fires_without_timing() {
    let text = take();

    // No timing: "So separately ..." opens a new thought, and it is the only
    // thing the rule has to go on.
    let no_timing = paragraphed(&text, &[], DictationMode::Document);
    assert_eq!(no_timing, format!("{S1} {S2}\n\n{S3} {S4}"));

    // Timing present, all of it short: the marker is ignored.
    let measured = [
        pause_at(&text, "a manual fix.", 0.25),
        pause_at(&text, "the pricing page.", 0.3),
    ];
    assert_eq!(paragraphed(&text, &measured, DictationMode::Document), text);
}

/// The fallback needs a genuinely multi-sentence take before a speech habit is
/// allowed to mean anything.
#[test]
fn marker_fallback_needs_several_sentences() {
    let long_two = format!(
        "{S2} So separately the pricing page still promises a free tier we removed in June and support has had three tickets about it this week and I would like it fixed."
    );
    assert_eq!(
        paragraphed(&long_two, &[], DictationMode::Document),
        long_two
    );
}

// ---------------------------------------------------------------------------
// Floors.
// ---------------------------------------------------------------------------

/// The no-regression floor: a short take is one paragraph, whatever happened in
/// the audio. Short takes are the common case and paragraphing one is never an
/// improvement.
#[test]
fn a_short_take_is_unchanged() {
    let text = format!("{S1} {S2}");
    let pauses = [pause_at(&text, "everything is green.", 4.0)];
    assert_eq!(paragraphed(&text, &pauses, DictationMode::Document), text);
    assert_eq!(paragraphed(&text, &[], DictationMode::Document), text);
    assert_eq!(paragraphed("", &[], DictationMode::Document), "");
}

/// A rendered list owns its own line breaks. Paragraphing never cuts inside one.
#[test]
fn a_detected_list_is_never_broken() {
    let list = "1. Move the history table over, which took about forty minutes and needed no manual fix at all.\n2. Point the read path at the new index so the dashboard feels quicker for everyone.\n3. Fix the pricing copy that still promises a free tier we removed back in June.";
    let pauses = [pause_at(list, "no manual fix at all.", 4.0)];
    assert_eq!(paragraphed(list, &pauses, DictationMode::Document), list);
}

// ---------------------------------------------------------------------------
// Reachability: the rule is wired into the pipeline, not a pure function that
// nothing calls.
// ---------------------------------------------------------------------------

/// The same break comes out of `run_cleanup` — the ordered stage the live take
/// and the History retry both run — with the LLM stage off, which is the
/// default. No model installed, paragraphs anyway.
#[test]
fn run_cleanup_paragraphs_a_long_take_with_no_model() {
    let text = take();
    let pauses = [pause_at(&text, "a manual fix.", 1.4)];
    let out = dictation::run_cleanup(
        &text,
        CleanupLevel::Medium,
        DictationMode::Document,
        Style::Default,
        &pauses,
        |t| t.to_string(),
        |_| None,
    );
    assert!(
        out.contains(&format!("{S2}\n\n{S3}")),
        "run_cleanup must paragraph the take; got: {out:?}"
    );
}

/// ...and in Chat it does not, through the same entry point.
#[test]
fn run_cleanup_never_paragraphs_chat() {
    let text = take();
    let pauses = [pause_at(&text, "a manual fix.", 4.0)];
    let out = dictation::run_cleanup(
        &text,
        CleanupLevel::Medium,
        DictationMode::Chat,
        Style::Default,
        &pauses,
        |t| t.to_string(),
        |_| None,
    );
    assert!(!out.contains('\n'), "chat stayed one block; got: {out:?}");
}
