//! yap-audit-eval-call-bleed — the call-bleed metrics and the fixture's script,
//! held to negative controls. No corpus, no model, no `say`: every number below
//! is computed from text and sample vectors written in the test.
//!
//! A metric that returns the same number whatever it is given is the failure
//! this file exists to rule out, so each one is run in BOTH directions — an
//! input that must score high and an input that must score low.

mod support;

use support::call_bleed::{
    bleed_gain, bleed_of, gap_before, reference_blocks, CallTurn, Plant, Who, BLEED_DELAY_SECONDS,
    BLEED_GAIN_DB, BLEED_TAIL, SCRIPT,
};
use support::eval_metrics::{
    echo_dup_rate, median_segment_seconds, mid_sentence_end_rate, notes_owner_accuracy,
    summary_verbatim_ratio, NoteItem, PlantedItem, TimedText, ECHO_SEARCH_SECONDS,
};
use wilson_voice_lib::meeting_echo::{
    echo_words, is_echo_of, word_trigram_containment, ECHO_CONTAINMENT_THRESHOLD,
};

fn tt(start: f64, end: f64, text: &str) -> TimedText {
    TimedText::new(start, end, text)
}

// --- echo_dup_rate ---------------------------------------------------------

const FAR_A: &str = "the second weekend is the clear one and the third has a wedding booked";
const FAR_B: &str = "parking is fine there is a lot behind the building that holds sixty cars";

#[test]
fn echo_dup_rate_is_one_when_every_far_window_reappears_in_the_mic() {
    let call = [tt(0.0, 5.0, FAR_A), tt(10.0, 15.0, FAR_B)];
    let mic = [tt(0.1, 5.1, FAR_A), tt(10.1, 15.1, FAR_B)];
    assert_eq!(echo_dup_rate(&call, &mic), 1.0);
}

#[test]
fn echo_dup_rate_is_zero_when_the_mic_holds_only_the_near_side() {
    let call = [tt(0.0, 5.0, FAR_A), tt(10.0, 15.0, FAR_B)];
    let mic = [
        tt(
            5.0,
            9.0,
            "then the second weekend looks better to me unless the parking changes",
        ),
        tt(
            15.5,
            19.0,
            "good that settles one worry so let us talk about the money",
        ),
    ];
    assert_eq!(echo_dup_rate(&call, &mic), 0.0);
}

#[test]
fn echo_dup_rate_counts_the_share_of_windows_not_of_words() {
    let call = [tt(0.0, 5.0, FAR_A), tt(10.0, 15.0, FAR_B)];
    let mic = [tt(0.1, 5.1, FAR_A)];
    assert_eq!(echo_dup_rate(&call, &mic), 0.5);
}

#[test]
fn echo_dup_rate_only_looks_within_one_and_a_half_seconds() {
    let call = [tt(10.0, 14.0, FAR_A)];
    // The same words, but 20 s away: that is the near side repeating a phrase,
    // not the speakers leaking into the mic.
    let far_away = [tt(34.0, 38.0, FAR_A)];
    assert_eq!(echo_dup_rate(&call, &far_away), 0.0);
    // Just inside the search margin counts; just outside does not.
    let inside = [tt(14.0 + ECHO_SEARCH_SECONDS - 0.1, 18.0, FAR_A)];
    let outside = [tt(14.0 + ECHO_SEARCH_SECONDS + 0.1, 18.0, FAR_A)];
    assert_eq!(echo_dup_rate(&call, &inside), 1.0);
    assert_eq!(echo_dup_rate(&call, &outside), 0.0);
}

#[test]
fn echo_dup_rate_ignores_windows_too_short_to_have_a_trigram() {
    // "good point" has no trigram: it is not judged, neither echo nor not-echo.
    let call = [tt(0.0, 1.0, "good point"), tt(2.0, 6.0, FAR_A)];
    let mic = [tt(0.0, 1.0, "good point"), tt(2.1, 6.1, FAR_A)];
    assert_eq!(echo_dup_rate(&call, &mic), 1.0);
    assert_eq!(echo_dup_rate(&[tt(0.0, 1.0, "good point")], &mic), 0.0);
    assert_eq!(echo_dup_rate(&[], &mic), 0.0);
}

#[test]
fn echo_dup_rate_survives_a_decoder_that_mangles_some_words() {
    // 30% of the trigrams surviving is an echo; a transcript that kept one
    // trigram in twelve is not.
    let window = "we will need at least a dozen more tables for the craft stations";
    let mangled = "we will need at least a dozen more chairs for the crafty stations";
    assert!(word_trigram_containment(window, mangled) >= ECHO_CONTAINMENT_THRESHOLD);
    assert_eq!(
        echo_dup_rate(&[tt(0.0, 5.0, window)], &[tt(0.0, 5.0, mangled)]),
        1.0
    );
    let barely = "we will need something else entirely to say about those stations now";
    assert!(word_trigram_containment(window, barely) < ECHO_CONTAINMENT_THRESHOLD);
    assert_eq!(
        echo_dup_rate(&[tt(0.0, 5.0, window)], &[tt(0.0, 5.0, barely)]),
        0.0
    );
}

#[test]
fn echo_dup_rate_agrees_with_the_production_predicate() {
    // One definition: the metric must say "echo" exactly when `is_echo_of` does.
    let cases = [
        (FAR_A, FAR_A),
        (FAR_A, FAR_B),
        (FAR_A, "the second weekend is the clear one"),
        (FAR_A, "the second weekend is"),
    ];
    for (window, mic) in cases {
        let rate = echo_dup_rate(&[tt(0.0, 4.0, window)], &[tt(0.0, 4.0, mic)]);
        assert_eq!(
            rate == 1.0,
            is_echo_of(window, mic),
            "{window:?} vs {mic:?}"
        );
    }
}

// --- mid_sentence_end_rate and median_segment_seconds ----------------------

#[test]
fn mid_sentence_end_rate_separates_clock_cuts_from_utterance_cuts() {
    let clock_cut = [
        tt(
            0.0,
            30.0,
            "so the hall has the second and third weekend open but only",
        ),
        tt(
            30.0,
            60.0,
            "one of them is free all day and the morning sessions",
        ),
    ];
    let utterances = [
        tt(0.0, 5.0, "The hall has the second weekend open."),
        tt(5.5, 9.0, "Which one is free all day?"),
        tt(9.5, 12.0, "That one, yes!"),
    ];
    assert_eq!(mid_sentence_end_rate(&clock_cut), 1.0);
    assert_eq!(mid_sentence_end_rate(&utterances), 0.0);
    let mixed = [clock_cut[0].clone(), utterances[0].clone()];
    assert_eq!(mid_sentence_end_rate(&mixed), 0.5);
}

#[test]
fn mid_sentence_end_rate_reads_through_closing_quotes_and_skips_empty_text() {
    let s = [
        tt(0.0, 4.0, "She said \"we are done.\""),
        tt(4.0, 5.0, "   "),
        tt(5.0, 6.0, "and then"),
    ];
    assert_eq!(mid_sentence_end_rate(&s), 0.5);
    assert_eq!(mid_sentence_end_rate(&[]), 0.0);
}

#[test]
fn median_segment_seconds_is_a_median_not_a_mean() {
    let s = [tt(0.0, 3.0, "a"), tt(3.0, 5.0, "b"), tt(5.0, 105.0, "c")];
    assert_eq!(median_segment_seconds(&s), 3.0);
    let even = [tt(0.0, 2.0, "a"), tt(2.0, 6.0, "b")];
    assert_eq!(median_segment_seconds(&even), 3.0);
    assert_eq!(median_segment_seconds(&[]), 0.0);
    // 30 s windows are what the shipped pipeline stores today; utterances are not.
    let windows = [tt(0.0, 30.0, "a"), tt(30.0, 60.0, "b"), tt(60.0, 90.0, "c")];
    assert!(median_segment_seconds(&windows) > 12.0);
}

// --- notes_owner_accuracy --------------------------------------------------

fn planted() -> Vec<PlantedItem> {
    vec![
        PlantedItem {
            owner: "Me".into(),
            keywords: ["send", "revised", "budget", "outline"]
                .map(String::from)
                .to_vec(),
        },
        PlantedItem {
            owner: "Them".into(),
            keywords: ["call", "caterer", "confirm", "headcount"]
                .map(String::from)
                .to_vec(),
        },
        PlantedItem {
            owner: "Someone here".into(),
            keywords: ["pick", "folding", "tables", "church"]
                .map(String::from)
                .to_vec(),
        },
    ]
}

fn note(text: &str, owner: &str) -> NoteItem {
    NoteItem {
        text: text.into(),
        owner: owner.into(),
    }
}

#[test]
fn notes_owner_accuracy_is_one_for_right_owners_and_zero_for_wrong_ones() {
    let right = [
        note(
            "Send the revised budget outline to the group by Friday",
            "Me",
        ),
        note("Call the caterer to confirm the headcount", "Them"),
        note("Pick up the folding tables from the church", "Someone here"),
    ];
    assert_eq!(notes_owner_accuracy(&planted(), &right), 1.0);

    // The echo defect: the far side's commitment credited to the user.
    let echoed = [
        note(
            "Send the revised budget outline to the group by Friday",
            "Me",
        ),
        note("Call the caterer to confirm the headcount", "Me"),
        note("Pick up the folding tables from the church", "Me"),
    ];
    assert!((notes_owner_accuracy(&planted(), &echoed) - 1.0 / 3.0).abs() < 1e-9);
}

#[test]
fn notes_owner_accuracy_counts_blank_owners_and_missed_items_as_wrong() {
    let blank = [note("Send the revised budget outline to the group", "")];
    assert_eq!(notes_owner_accuracy(&planted(), &blank), 0.0);
    // An item about something else never matches, even with the right owner.
    let unrelated = [note("Order umbrellas for the door", "Me")];
    assert_eq!(notes_owner_accuracy(&planted(), &unrelated), 0.0);
    assert_eq!(notes_owner_accuracy(&planted(), &[]), 0.0);
    assert_eq!(notes_owner_accuracy(&[], &blank), 0.0);
}

#[test]
fn notes_owner_accuracy_matches_the_best_item_and_ignores_case() {
    let notes = [
        note("Order umbrellas for the door", "Them"),
        note("send revised budget outline", "me"),
    ];
    let one = &planted()[..1];
    assert_eq!(notes_owner_accuracy(one, &notes), 1.0);
}

// --- summary_verbatim_ratio ------------------------------------------------

#[test]
fn summary_verbatim_ratio_is_one_for_a_paste_and_low_for_a_rewrite() {
    let transcript = "then it is decided we cap each session at thirty people and open a \
                      waiting list for the rest agreed and the waiting list can live in the \
                      same sign up form";
    let paste = "we cap each session at thirty people and open a waiting list for the rest";
    let rewrite = "attendance is limited to thirty per session, overflow registrants queue";
    assert_eq!(summary_verbatim_ratio(paste, transcript), 1.0);
    assert!(summary_verbatim_ratio(rewrite, transcript) < 0.1);
    assert_eq!(summary_verbatim_ratio("too short", transcript), 0.0);
}

#[test]
fn summary_verbatim_ratio_uses_four_grams_not_trigrams() {
    // Shares trigrams with the transcript but no four-gram: three-word
    // coincidences are not copying.
    let transcript = "alpha bravo charlie delta echo foxtrot";
    let summary = "alpha bravo charlie then something delta echo foxtrot";
    assert!(word_trigram_containment(summary, transcript) > 0.0);
    assert_eq!(summary_verbatim_ratio(summary, transcript), 0.0);
}

// --- the bleed mix ---------------------------------------------------------

#[test]
fn the_bleed_is_twelve_db_down_sixty_ms_late_with_a_short_tail() {
    assert!((20.0 * (bleed_gain() as f64).log10() - BLEED_GAIN_DB).abs() < 1e-4);
    let rate = 16_000u32;
    let mut impulse = vec![0.0f32; 1_000];
    impulse[100] = 1.0;
    let bleed = bleed_of(&impulse, rate);
    let delay = (BLEED_DELAY_SECONDS * rate as f64).round() as usize;
    assert_eq!(delay, 960);
    // Silent until the delayed direct path.
    assert!(bleed[..100 + delay].iter().all(|s| *s == 0.0));
    // The direct path: unit impulse * -12 dB.
    assert!((bleed[100 + delay] - bleed_gain()).abs() < 1e-6);
    // The tail: each tap below the direct path and none later than the longest tap.
    let longest = BLEED_TAIL
        .iter()
        .map(|(d, _)| (d * rate as f64).round() as usize)
        .max()
        .unwrap();
    for (extra, gain) in BLEED_TAIL {
        let at = 100 + delay + (extra * rate as f64).round() as usize;
        assert!(
            (bleed[at] - bleed_gain() * gain).abs() < 1e-6,
            "tap at +{extra}s"
        );
        assert!(bleed[at] < bleed[100 + delay]);
    }
    assert!(bleed[100 + delay + longest + 1..].iter().all(|s| *s == 0.0));
    assert_eq!(bleed.len(), impulse.len() + delay + longest);
    // A short tail: 120 ms, nowhere near a room's second.
    assert!(longest as f64 / rate as f64 <= 0.150);
}

// --- the script ------------------------------------------------------------

#[test]
fn the_script_plants_six_commitments_and_two_decisions_with_owners() {
    let mut commitments: Vec<(&str, &[&str], &str)> = Vec::new();
    let mut decisions = 0usize;
    for turn in SCRIPT.iter() {
        match turn.plant {
            Plant::Nothing => {}
            Plant::Commitment(k) => commitments.push((turn.who.label(), k, turn.text)),
            Plant::Decision(k) => {
                decisions += 1;
                let words = echo_words(turn.text);
                assert!(
                    k.iter().all(|w| words.iter().any(|t| t == w)),
                    "{k:?} in {}",
                    turn.text
                );
            }
        }
    }
    assert_eq!(commitments.len(), 6);
    assert_eq!(decisions, 2);
    for (owner, keywords, text) in &commitments {
        let words = echo_words(text);
        assert!(
            keywords.iter().all(|w| words.iter().any(|t| t == w)),
            "{keywords:?} are not all in {text}"
        );
        assert!(["Me", "Them", "Someone here"].contains(owner));
    }
    let count = |who: &str| commitments.iter().filter(|c| c.0 == who).count();
    assert_eq!(
        (count("Them"), count("Me"), count("Someone here")),
        (3, 2, 1)
    );
}

#[test]
fn the_script_follows_the_corpus_rules() {
    assert_eq!(SCRIPT.len(), 48);
    for (i, turn) in SCRIPT.iter().enumerate() {
        assert!(
            !turn.text.chars().any(|c| c.is_ascii_digit()),
            "turn {i} has a digit"
        );
        let words = echo_words(turn.text).len();
        // 2-8 s at 175 wpm is 6-23 words; the generator measures the real audio.
        assert!((6..=22).contains(&words), "turn {i} has {words} words");
    }
    // Far-side and near-side turns both exist, the second voice speaks, and the
    // far side never speaks twice in the SAME turn slot (each turn is one voice).
    assert!(SCRIPT.iter().any(|t| t.who == Who::Them));
    assert!(SCRIPT.iter().any(|t| t.who == Who::Me));
    assert!(SCRIPT.iter().filter(|t| t.who == Who::Here).count() >= 3);
    // No far-side sentence appears twice (an echo test needs unique text).
    for (i, a) in SCRIPT.iter().enumerate() {
        for b in SCRIPT.iter().skip(i + 1) {
            assert_ne!(a.text, b.text);
        }
    }
    assert!((0..SCRIPT.len()).all(|i| gap_before(i) >= 0.4));
}

#[test]
fn reference_blocks_are_m_ss_speaker_headers_then_text() {
    let turns = vec![
        CallTurn {
            speaker: "Them".into(),
            voice: "Fred".into(),
            track: "track1.wav".into(),
            text: "Hello there everyone.".into(),
            start_seconds: 0.5,
            end_seconds: 3.0,
        },
        CallTurn {
            speaker: "Me".into(),
            voice: "Samantha".into(),
            track: "track0.wav".into(),
            text: "Hi.".into(),
            start_seconds: 75.9,
            end_seconds: 77.0,
        },
    ];
    assert_eq!(
        reference_blocks(&turns),
        "0:00 - Them\nHello there everyone.\n\n1:15 - Me\nHi.\n"
    );
}
