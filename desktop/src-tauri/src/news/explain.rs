//! yap-1106-news-explain — one line each for the top 3 stories, in Yappy's
//! chosen voice.
//!
//! The rules, in order:
//!
//! * Only the top [`TOP_N`] stories are sent, and of each only its headline
//!   and snippet ([`user_prompt`]): no link, outlet, topic, ticker, history or
//!   anything the user dictated.
//! * The answer must be one numbered line per story ([`parse_lines`]). A line
//!   that is missing, too long, gives money advice ([`is_advice`]) or says
//!   something the headline and snippet do not ([`supported`]) is replaced by
//!   the headline alone. A failed call (offline, no key, cap, timeout, error)
//!   is every headline alone. Yappy never makes something up.
//! * Never buy or sell advice: the prompt forbids it ([`SYSTEM_RULES`]) and
//!   the check above drops any line that slips through.
//!
//! Pure except for the one `call` seam ([`explain_with`]), so the whole path
//! is tested with a fake cloud. The shipping seam is `news_explain_cloud` in
//! `lib.rs` (the BYOM cloud route, its breaker, cap and ledger).
//!
//! Telemetry: `news explain ms=… fallback=… sent=… explained=…
//! unsupported=… advice=… voice=…`. Never a headline, snippet, line or URL.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use serde::Serialize;

use super::rank::RankedStory;
use crate::cloud_route::Budget;

/// How many stories are explained (and the most that are ever sent).
pub const TOP_N: usize = 3;
/// The longest line kept, in words. The prompt asks for 24.
pub const MAX_WORDS: usize = 32;
/// Headline and snippet caps, in characters, before they are sent.
pub const MAX_HEADLINE_CHARS: usize = 200;
pub const MAX_SNIPPET_CHARS: usize = 300;
/// Three short lines.
pub const MAX_TOKENS: u32 = 320;
/// Not latency-bound like a paste, but the user is looking at the card.
pub const BUDGET: Budget = Budget {
    ttft: Duration::from_millis(3000),
    total: Duration::from_secs(8),
};
/// At least this many word stems of a line must come from its story.
pub const MIN_OVERLAP: usize = 2;

/// Yappy's voice, from the stored `companionTone` (the same mapping as
/// `voiceFor` in `desktop/src/pill/voices.ts`; unknown is Best friend).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Voice {
    BestFriend,
    Badass,
    Professional,
    Rose,
}

impl Voice {
    pub fn from_setting(setting: &str) -> Self {
        match setting.trim() {
            "rude" | "badass" => Voice::Badass,
            "professional" => Voice::Professional,
            "rose" => Voice::Rose,
            _ => Voice::BestFriend,
        }
    }

    pub fn tag(self) -> &'static str {
        match self {
            Voice::BestFriend => "bestfriend",
            Voice::Badass => "badass",
            Voice::Professional => "professional",
            Voice::Rose => "rose",
        }
    }

    pub fn style(self) -> &'static str {
        match self {
            Voice::BestFriend => {
                "Voice: a warm best friend. Casual, upbeat, plain words, like texting a pal."
            }
            Voice::Badass => {
                "Voice: a blunt, confident badass. Short, punchy, a little cocky. No swearing."
            }
            Voice::Professional => {
                "Voice: a calm professional briefer. Neutral, precise, no slang, no jokes."
            }
            Voice::Rose => "Voice: a sweet, affectionate friend. Gentle and kind, plain words.",
        }
    }
}

/// The rules every voice shares. Never names a vendor; never asks for or
/// allows advice.
pub const SYSTEM_RULES: &str = "You are Yappy, a small companion who tells the user what a few news stories are about. \
For each numbered story write exactly ONE line of at most 24 words saying what happened. \
Use ONLY the facts in that story's headline and snippet: no outside facts, and no numbers, names or dates that are not in them. If the snippet says little, stay close to the headline. \
Never give financial advice: never tell anyone to buy, sell, hold, short or invest in anything, never predict a price, never say what to do with money. \
No swearing. No links. Never mention any AI company or model. \
Answer with only the numbered lines, \"1. ...\", \"2. ...\" and so on, nothing else.";

/// The system turn for `voice`.
pub fn system_prompt(voice: Voice) -> String {
    format!("{}\n{}", SYSTEM_RULES, voice.style())
}

/// What is sent for one story: its headline and snippet, nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainInput {
    pub headline: String,
    pub snippet: String,
}

fn one_line(s: &str, max_chars: usize) -> String {
    let collapsed = s.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(max_chars).collect()
}

/// The top [`TOP_N`] stories as inputs, best first. Anything past the third
/// is never looked at.
pub fn inputs_for(stories: &[RankedStory]) -> Vec<ExplainInput> {
    stories
        .iter()
        .take(TOP_N)
        .map(|r| ExplainInput {
            headline: one_line(&r.story.title, MAX_HEADLINE_CHARS),
            snippet: one_line(r.story.snippet.as_deref().unwrap_or(""), MAX_SNIPPET_CHARS),
        })
        .collect()
}

/// The user turn: numbered headlines and snippets, nothing else.
pub fn user_prompt(inputs: &[ExplainInput]) -> String {
    let mut out = String::new();
    for (i, input) in inputs.iter().enumerate() {
        let snippet = if input.snippet.is_empty() {
            "(none)"
        } else {
            input.snippet.as_str()
        };
        out.push_str(&format!(
            "{}. Headline: {}\n   Snippet: {}\n",
            i + 1,
            input.headline,
            snippet
        ));
    }
    out
}

/// The numbered lines of an answer: slot `i` is line `i + 1`, `None` when the
/// answer has no usable line for it. The first line with a number wins.
pub fn parse_lines(text: &str, n: usize) -> Vec<Option<String>> {
    let mut out: Vec<Option<String>> = vec![None; n];
    for raw in text.lines() {
        let line = raw.trim().trim_start_matches(&['*', '-', '#', ' '][..]);
        let digits: String = line.chars().take_while(|c| c.is_ascii_digit()).collect();
        let Ok(num) = digits.parse::<usize>() else {
            continue;
        };
        if num == 0 || num > n || out[num - 1].is_some() {
            continue;
        }
        let rest = &line[digits.len()..];
        let Some(rest) = rest
            .strip_prefix('.')
            .or_else(|| rest.strip_prefix(')'))
            .or_else(|| rest.strip_prefix(':'))
        else {
            continue;
        };
        let body = rest.trim_matches(|c: char| {
            c.is_whitespace() || c == '"' || c == '\u{201c}' || c == '\u{201d}' || c == '*'
        });
        if !body.is_empty() {
            out[num - 1] = Some(body.to_string());
        }
    }
    out
}

const STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "from", "into", "over", "about", "after", "just",
    "they", "their", "them", "there", "here", "what", "when", "were", "will", "would", "could",
    "should", "been", "have", "has", "had", "its", "it's", "your", "you", "are", "was", "not",
    "but", "big", "news", "story", "today", "now", "new", "more", "some", "very", "really", "says",
    "said", "than", "then", "also", "still", "yappy",
];

/// A lowercased word with its edge punctuation and possessive removed.
fn norm(word: &str) -> String {
    let w = word
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    if let Some(base) = w.strip_suffix("'s").or_else(|| w.strip_suffix("\u{2019}s")) {
        return base.to_string();
    }
    w
}

fn stem(word: &str) -> String {
    word.chars().take(5).collect()
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || c == '/' || c == '\u{2014}' || c == '\u{2013}')
        .map(norm)
        .filter(|w| !w.is_empty())
        .collect()
}

fn content_stems(text: &str) -> HashSet<String> {
    words(text)
        .into_iter()
        .filter(|w| w.chars().count() >= 4 && !STOPWORDS.contains(&w.as_str()))
        .map(|w| stem(&w))
        .collect()
}

/// Is `line` backed by its story? Every number and every mid-sentence name in
/// it must appear in the headline or snippet, and at least [`MIN_OVERLAP`]
/// word stems (fewer only when the story itself has fewer) must come from it.
pub fn supported(line: &str, input: &ExplainInput) -> bool {
    let source = format!("{} {}", input.headline, input.snippet);
    let source_words: HashSet<String> = words(&source).into_iter().collect();
    let source_digits: HashSet<String> = source_words
        .iter()
        .filter(|w| w.chars().any(|c| c.is_ascii_digit()))
        .map(|w| w.replace(',', ""))
        .collect();

    let mut sentence_start = true;
    for raw in line.split_whitespace() {
        let w = norm(raw);
        if w.is_empty() {
            continue;
        }
        if w.chars().any(|c| c.is_ascii_digit()) && !source_digits.contains(&w.replace(',', "")) {
            return false;
        }
        let capital = raw
            .trim_start_matches(|c: char| !c.is_alphanumeric())
            .chars()
            .next()
            .is_some_and(char::is_uppercase);
        if capital && !sentence_start && w != "i" && !source_words.contains(&w) {
            return false;
        }
        sentence_start = raw.ends_with(&['.', '!', '?', ':'][..]);
    }

    let have = content_stems(&source);
    let need = MIN_OVERLAP.min(have.len()).max(1);
    content_stems(line).intersection(&have).count() >= need
}

/// Phrases that are advice whatever the story says.
const ADVICE_PHRASES: &[&str] = &[
    "time to buy",
    "time to sell",
    "buy the dip",
    "buy now",
    "sell now",
    "load up",
    "go long",
    "go short",
    "short it",
    "short the stock",
    "price target",
    "financial advice",
    "invest in",
    "worth buying",
    "worth a buy",
    "your portfolio",
    "your money",
    "get in now",
    "get in early",
    "cash out",
    "take profits",
    "grab some shares",
    "hold your shares",
    "hold on to your",
    "should buy",
    "should sell",
    "should invest",
    "should hold",
    "to the moon",
];

/// Money-move words and their root. A root that is not in the story itself
/// ("Microsoft to buy a studio") is advice.
fn advice_root(word: &str) -> Option<&'static str> {
    match word {
        "buy" | "buys" | "buying" | "bought" => Some("buy"),
        "sell" | "sells" | "selling" | "sold" | "sell-off" | "selloff" => Some("sell"),
        "invest" | "invests" | "investing" | "investment" => Some("invest"),
        "dump" | "dumps" | "dumping" => Some("dump"),
        "hodl" => Some("hodl"),
        _ => None,
    }
}

/// Does `line` tell anyone what to do with money?
pub fn is_advice(line: &str, input: &ExplainInput) -> bool {
    let lower = words(line).join(" ");
    if ADVICE_PHRASES.iter().any(|p| lower.contains(p)) {
        return true;
    }
    let source = format!("{} {}", input.headline, input.snippet);
    let source_roots: HashSet<&'static str> = words(&source)
        .iter()
        .filter_map(|w| advice_root(w))
        .collect();
    words(line)
        .iter()
        .filter_map(|w| advice_root(w))
        .any(|root| !source_roots.contains(root))
}

/// Why a line was kept or dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Kept,
    Missing,
    TooLong,
    Advice,
    Unsupported,
}

pub fn check_line(line: Option<&str>, input: &ExplainInput) -> Verdict {
    let Some(line) = line else {
        return Verdict::Missing;
    };
    if line.split_whitespace().count() > MAX_WORDS {
        Verdict::TooLong
    } else if is_advice(line, input) {
        Verdict::Advice
    } else if !supported(line, input) {
        Verdict::Unsupported
    } else {
        Verdict::Kept
    }
}

/// One story as the card shows it: Yappy's line, or the headline alone
/// (`explained: false`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplainedLine {
    pub url_key: String,
    pub url: String,
    pub headline: String,
    pub line: String,
    pub explained: bool,
}

/// The result and its telemetry record.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Explained {
    pub lines: Vec<ExplainedLine>,
    pub voice: Voice,
    /// `none` (every line is Yappy's), `unsupported` (one or more lines fell
    /// back to the headline), `empty` (nothing to explain), or the reason the
    /// call did not happen or failed (`user`, `no_key`, `offline`, `cap`,
    /// `breaker`, `timeout`, `error`, ...).
    pub fallback: String,
    pub sent: usize,
    pub unsupported: usize,
    pub advice: usize,
    pub ms: u64,
}

/// Explain the top [`TOP_N`] of `stories` in `voice`. `call` gets the system
/// and user turns and returns the complete answer or a reason tag; it is
/// called at most once, and never when there is nothing to explain.
pub fn explain_with(
    stories: &[RankedStory],
    voice: Voice,
    call: &mut dyn FnMut(&str, &str) -> Result<String, &'static str>,
) -> Explained {
    let started = Instant::now();
    let top: Vec<&RankedStory> = stories.iter().take(TOP_N).collect();
    let inputs = inputs_for(stories);
    let headline_only = |r: &RankedStory| ExplainedLine {
        url_key: r.url_key.clone(),
        url: r.story.url.clone(),
        headline: r.story.title.clone(),
        line: r.story.title.clone(),
        explained: false,
    };

    let mut unsupported = 0usize;
    let mut advice = 0usize;
    let (lines, fallback): (Vec<ExplainedLine>, String) = if inputs.is_empty() {
        (Vec::new(), "empty".to_string())
    } else {
        match call(&system_prompt(voice), &user_prompt(&inputs)) {
            Err(tag) => (
                top.iter().map(|&r| headline_only(r)).collect(),
                tag.to_string(),
            ),
            Ok(text) => {
                let parsed = parse_lines(&text, inputs.len());
                let lines: Vec<ExplainedLine> = top
                    .iter()
                    .zip(inputs.iter())
                    .zip(parsed.iter())
                    .map(
                        |((r, input), line)| match check_line(line.as_deref(), input) {
                            Verdict::Kept => ExplainedLine {
                                line: line.clone().unwrap_or_default(),
                                explained: true,
                                ..headline_only(r)
                            },
                            verdict => {
                                if verdict == Verdict::Advice {
                                    advice += 1;
                                }
                                unsupported += 1;
                                headline_only(r)
                            }
                        },
                    )
                    .collect();
                let fallback = if unsupported == 0 {
                    "none"
                } else {
                    "unsupported"
                };
                (lines, fallback.to_string())
            }
        }
    };

    let ms = started.elapsed().as_millis() as u64;
    let explained = lines.iter().filter(|l| l.explained).count();
    log::info!(
        "news explain ms={ms} fallback={fallback} sent={} explained={explained} unsupported={unsupported} advice={advice} voice={}",
        inputs.len(),
        voice.tag()
    );
    Explained {
        lines,
        voice,
        fallback,
        sent: inputs.len(),
        unsupported,
        advice,
        ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::news::feeds::{Source, Story};

    fn ranked(title: &str, snippet: Option<&str>, url: &str) -> RankedStory {
        RankedStory {
            story: Story {
                source: Source::GoogleNews,
                topic: "markets".to_string(),
                title: title.to_string(),
                url: url.to_string(),
                outlet: Some("The Paper".to_string()),
                outlets: 1,
                published_ms: Some(1),
                points: None,
                comments: None,
                snippet: snippet.map(str::to_string),
            },
            score: 1.0,
            url_key: url.to_string(),
            title_key: title.to_lowercase(),
        }
    }

    fn five() -> Vec<RankedStory> {
        vec![
            ranked(
                "Apple shares slide after iPhone sales miss",
                Some("Apple reported iPhone sales of 41 million, below forecasts, and shares fell 4% in late trading."),
                "https://example.com/apple",
            ),
            ranked(
                "Microsoft to buy gaming studio Ironclad",
                Some("Microsoft agreed to buy Ironclad Games for $2 billion, its second studio deal this year."),
                "https://example.com/msft",
            ),
            ranked(
                "City council approves new bike lanes downtown",
                Some("The council voted 7-2 to add protected bike lanes on Main Street by spring."),
                "https://example.com/bikes",
            ),
            ranked(
                "FOURTH story must never be sent",
                Some("SECRET fourth snippet"),
                "https://example.com/four",
            ),
            ranked(
                "FIFTH story must never be sent",
                None,
                "https://example.com/five",
            ),
        ]
    }

    #[test]
    fn only_the_top_three_headlines_and_snippets_are_sent() {
        let stories = five();
        let mut sent: Vec<(String, String)> = Vec::new();
        let mut call = |system: &str, user: &str| -> Result<String, &'static str> {
            sent.push((system.to_string(), user.to_string()));
            Err("offline")
        };
        let out = explain_with(&stories, Voice::BestFriend, &mut call);
        assert_eq!(sent.len(), 1, "one call for all three");
        let (_, user) = &sent[0];
        assert!(user.contains("Apple shares slide"));
        assert!(user.contains("Ironclad Games for $2 billion"));
        assert!(user.contains("City council approves"));
        assert!(!user.contains("FOURTH") && !user.contains("SECRET") && !user.contains("FIFTH"));
        // Headline and snippet only: no link, outlet or topic.
        assert!(!user.contains("https://"));
        assert!(!user.contains("example.com"));
        assert!(!user.contains("The Paper"));
        assert!(!user.contains("markets"));
        assert_eq!(out.sent, 3);
        assert_eq!(out.lines.len(), 3);
    }

    #[test]
    fn nothing_is_sent_when_there_is_nothing_to_explain() {
        let mut calls = 0;
        let mut call = |_: &str, _: &str| -> Result<String, &'static str> {
            calls += 1;
            Ok(String::new())
        };
        let out = explain_with(&[], Voice::Badass, &mut call);
        assert_eq!(calls, 0);
        assert!(out.lines.is_empty());
        assert_eq!(out.fallback, "empty");
    }

    #[test]
    fn one_line_each_in_the_chosen_voice() {
        let stories = five();
        for (setting, voice) in [
            ("friendly", Voice::BestFriend),
            ("rude", Voice::Badass),
            ("badass", Voice::Badass),
            ("professional", Voice::Professional),
            ("rose", Voice::Rose),
            ("", Voice::BestFriend),
        ] {
            assert_eq!(Voice::from_setting(setting), voice);
        }
        let mut systems = Vec::new();
        for voice in [Voice::BestFriend, Voice::Badass, Voice::Professional] {
            let mut call = |system: &str, _: &str| -> Result<String, &'static str> {
                systems.push(system.to_string());
                Ok(
                    "1. Apple shares slid after iPhone sales came in below forecasts.\n\
                    2. Microsoft agreed to buy Ironclad Games for $2 billion.\n\
                    3. The council voted 7-2 for protected bike lanes on Main Street."
                        .to_string(),
                )
            };
            let out = explain_with(&stories, voice, &mut call);
            assert_eq!(out.fallback, "none", "{voice:?}");
            assert_eq!(out.lines.len(), 3);
            assert!(out
                .lines
                .iter()
                .all(|l| l.explained && !l.line.contains('\n')));
            assert_eq!(out.voice, voice);
        }
        assert!(systems[0].contains(Voice::BestFriend.style()));
        assert!(systems[1].contains(Voice::Badass.style()));
        assert!(systems[2].contains(Voice::Professional.style()));
        assert!(systems.iter().all(|s| s.contains("exactly ONE line")));
    }

    #[test]
    fn parse_lines_reads_numbered_lines_and_ignores_extras() {
        let p = parse_lines(
            "Sure!\n1. one\n2) two\n**3.** \"three\"\n4. four\n1. again",
            3,
        );
        assert_eq!(p[0].as_deref(), Some("one"));
        assert_eq!(p[1].as_deref(), Some("two"));
        assert_eq!(p[2].as_deref(), Some("three"));
        assert_eq!(parse_lines("no numbers here", 3), vec![None, None, None]);
    }

    #[test]
    fn headline_only_when_the_call_fails() {
        let stories = five();
        for tag in ["offline", "no_key", "timeout", "error", "cap"] {
            let mut call = |_: &str, _: &str| -> Result<String, &'static str> { Err(tag) };
            let out = explain_with(&stories, Voice::Professional, &mut call);
            assert_eq!(out.fallback, tag);
            assert_eq!(out.lines.len(), 3);
            for (line, story) in out.lines.iter().zip(stories.iter()) {
                assert!(!line.explained);
                assert_eq!(line.line, story.story.title);
                assert_eq!(line.url, story.story.url);
            }
        }
    }

    #[test]
    fn headline_only_when_the_line_is_not_supported_by_the_snippet() {
        let stories = five();
        let mut call = |_: &str, _: &str| -> Result<String, &'static str> {
            Ok(
                // 1: invented number. 2: invented name. 3: nothing from the story.
                "1. Apple shares fell 12% after iPhone sales missed forecasts.\n\
                 2. Microsoft is buying Ironclad, and Google is furious about it.\n\
                 3. Honestly what a wild week for everybody out there."
                    .to_string(),
            )
        };
        let out = explain_with(&stories, Voice::BestFriend, &mut call);
        assert_eq!(out.fallback, "unsupported");
        assert_eq!(out.unsupported, 3);
        assert!(out
            .lines
            .iter()
            .all(|l| !l.explained && l.line == l.headline));

        let missing = explain_with(&stories, Voice::BestFriend, &mut |_: &str, _: &str| {
            Ok("1. Apple shares slid after iPhone sales came in below forecasts.".to_string())
        });
        assert!(missing.lines[0].explained);
        assert!(!missing.lines[1].explained && !missing.lines[2].explained);
        assert_eq!(missing.unsupported, 2);
    }

    #[test]
    fn supported_accepts_voice_flavoured_lines_built_from_the_story() {
        let inputs = inputs_for(&five());
        let input = &inputs[0];
        assert!(supported(
            "Oof, Apple shares slid because iPhone sales missed forecasts.",
            input
        ));
        assert!(supported(
            "Apple's iPhone sales came in at 41 million. Shares fell 4%.",
            input
        ));
        assert!(!supported("Apple shares fell 9% today.", input));
        assert!(!supported(
            "Apple shares slid while Tim Cook watched.",
            input
        ));
    }

    /// Fixtures: advice the model might slip in. Every one falls back.
    const ADVICE_FIXTURES: &[&str] = &[
        "1. Apple shares slid after iPhone sales missed, time to buy the dip.\n2. Microsoft agreed to buy Ironclad Games, you should buy Microsoft stock.\n3. The council voted for bike lanes on Main Street, invest in bike shops.",
        "1. Apple shares slid on iPhone sales, sell now before it gets worse.\n2. Microsoft agreed to buy Ironclad Games, load up on shares.\n3. Bike lanes on Main Street got the council vote, great for your portfolio.",
        "1. Apple shares slid on weak iPhone sales, dump Apple.\n2. Microsoft agreed to buy Ironclad Games, price target goes up.\n3. The council voted for bike lanes, not financial advice but buy bikes.",
    ];

    #[test]
    fn never_buy_or_sell_advice_prompt_and_fixtures() {
        // The prompt forbids it in every voice.
        for voice in [
            Voice::BestFriend,
            Voice::Badass,
            Voice::Professional,
            Voice::Rose,
        ] {
            let s = system_prompt(voice);
            assert!(s.contains("Never give financial advice"));
            assert!(s.contains("buy, sell, hold, short or invest"));
        }
        // And any line that slips through is the headline alone.
        let stories = five();
        for fixture in ADVICE_FIXTURES {
            let out = explain_with(&stories, Voice::Badass, &mut |_: &str, _: &str| {
                Ok(fixture.to_string())
            });
            assert_eq!(out.advice, 3, "{fixture}");
            assert!(out
                .lines
                .iter()
                .all(|l| !l.explained && l.line == l.headline));
            assert!(out
                .lines
                .iter()
                .all(|l| !l.line.to_lowercase().contains("buy the dip")));
        }
    }

    #[test]
    fn reporting_a_deal_is_not_advice() {
        let inputs = inputs_for(&five());
        let input = &inputs[1];
        assert!(!is_advice(
            "Microsoft is buying Ironclad Games for $2 billion.",
            input
        ));
        assert!(is_advice(
            "Microsoft is buying Ironclad, so buy now.",
            input
        ));
        let apple = &inputs[0];
        assert!(is_advice("Apple shares slid, I'd be selling.", apple));
    }

    #[test]
    fn telemetry_record_has_ms_and_fallback() {
        let out = explain_with(&five(), Voice::BestFriend, &mut |_: &str, _: &str| {
            Err("timeout")
        });
        assert_eq!(out.fallback, "timeout");
        let v = serde_json::to_value(&out).unwrap();
        assert!(v.get("ms").is_some());
        assert_eq!(v["fallback"], "timeout");
        assert_eq!(v["lines"][0]["explained"], false);
    }

    #[test]
    fn too_long_lines_fall_back() {
        let inputs = inputs_for(&five());
        let input = &inputs[0];
        let long = "Apple shares slid ".repeat(12);
        assert_eq!(check_line(Some(long.as_str()), input), Verdict::TooLong);
        assert_eq!(check_line(None, input), Verdict::Missing);
    }

    #[test]
    fn snippet_is_the_description_as_one_plain_line() {
        use crate::news::feeds::snippet_from_html;
        assert_eq!(
            snippet_from_html(
                "<ol><li><a href=\"x\">Apple slides</a>&nbsp;&nbsp;<font color=\"#6f6f6f\">The Guardian</font></li></ol>",
                "Apple slides"
            )
            .as_deref(),
            Some("Apple slides The Guardian")
        );
        assert_eq!(
            snippet_from_html("<b>Apple slides</b>", "Apple slides"),
            None
        );
        assert_eq!(snippet_from_html("", "Apple slides"), None);
        let long = snippet_from_html(&"word ".repeat(200), "x").unwrap();
        assert_eq!(long.chars().count(), MAX_SNIPPET_CHARS);
    }

    #[test]
    fn cache_keeps_the_snippet_and_upgrades_an_old_table() {
        use crate::news::cache;
        let c = rusqlite::Connection::open_in_memory().unwrap();
        // The table as yap-1106-news-rank-cache made it, before `snippet`.
        c.execute_batch(
            "CREATE TABLE news_cache (url_key TEXT PRIMARY KEY, title_key TEXT NOT NULL,
             source TEXT NOT NULL, topic TEXT NOT NULL, title TEXT NOT NULL, url TEXT NOT NULL,
             outlet TEXT, outlets INTEGER NOT NULL DEFAULT 1, published_ms INTEGER,
             points INTEGER, comments INTEGER, fetched_ms INTEGER NOT NULL,
             expires_ms INTEGER NOT NULL);",
        )
        .unwrap();
        cache::ensure_schema(&c).unwrap();
        cache::ensure_schema(&c).unwrap();
        let stories: Vec<Story> = five().into_iter().map(|r| r.story).collect();
        let prefs = crate::news::rank::tests::prefs(&["markets"], &[], &[]);
        cache::ingest(&c, &stories, &prefs, 1_000, 10).unwrap();
        let back = cache::cached(&c, 1_000).unwrap();
        let apple = back
            .iter()
            .find(|s| s.url == "https://example.com/apple")
            .expect("apple cached");
        assert_eq!(apple.snippet, stories[0].snippet);
        assert!(apple.snippet.is_some());
    }
}
