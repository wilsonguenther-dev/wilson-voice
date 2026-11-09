//! yap-1107-news-no-off-topic-stories — what a story is ABOUT, decided from
//! the story, never from the search that found it.
//!
//! Wilson's briefing (topics Developer, Stocks, Business, Tech, World,
//! Culture, Science; no Sports) showed three golf stories labelled TECH: the
//! Google News search for "technology" returned "... at the World Wide
//! Technology Championship" from PGA TOUR and Golfweek, and every result was
//! stamped with the topic that searched for it. Now each story is
//! classified, in this order:
//!
//! 1. **Declared source category** ([`declared_category`]): Hacker News is
//!    Developer; a curated outlet table ([`OUTLETS`]) plus outlet-name words
//!    ("Sports", "Golf", "Hollywood", ...) give a publisher's category. The
//!    declared category wins, so a golf outlet can never be Tech.
//! 2. **Cloud label** — when a cloud key is connected, ONE short call per
//!    refresh labels the undeclared stories ([`classify_prompt`],
//!    [`parse_labels`]); labels are cached by url key in
//!    [`super::cache`] so a story is sent once. A failed call falls through.
//! 3. **Offline keywords** ([`keyword_category`]): a sports/entertainment
//!    deny-list ([`SPORTS_WORDS`], [`ENTERTAINMENT_WORDS`]) vetoes first —
//!    multi-word event names such as "World Wide Technology", "World Series"
//!    and "World Cup" are deny phrases, never topic hits — then whole-word
//!    matches against a curated per-topic list ([`TOPIC_WORDS`]).
//!
//! A story whose category is not one of the user's topics (and that names
//! none of their tickers or own words) is DROPPED by [`super::rank`] and
//! counted as `off_topic`; it is never relabelled to the search topic.
//! Pure except for the one installable cloud seam ([`install_cloud`]).

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::feeds::{google_query, Source, Story};
use crate::cloud_route::Budget;
use crate::news_prefs::NewsPrefs;

/// The closed label set. `Entertainment` has no topic chip today, so an
/// entertainment story only shows when the user names it as an own word.
///
/// fix-l3b3-4-news-new-chips-fetch-and-match: every topic chip has a
/// category whose [`Category::tag`] is exactly the chip id, so the fetch
/// plan searches for it and [`match_prefs`] can keep its stories. The five
/// chips the approved page added (AI, Design, Startups, Crypto, Local) are
/// narrower than the old ones; [`Category::parent`] lets a story labelled
/// with one still reach a user who picked only the broader legacy chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Developer,
    Stocks,
    Business,
    Tech,
    Science,
    World,
    Culture,
    Ai,
    Design,
    Startups,
    Crypto,
    Local,
    Sports,
    Entertainment,
    Other,
}

impl Category {
    /// Every label, `Other` last (the cloud prompt lists them in this order).
    pub const ALL: [Category; 15] = [
        Category::Developer,
        Category::Stocks,
        Category::Business,
        Category::Tech,
        Category::Science,
        Category::World,
        Category::Culture,
        Category::Ai,
        Category::Design,
        Category::Startups,
        Category::Crypto,
        Category::Local,
        Category::Sports,
        Category::Entertainment,
        Category::Other,
    ];

    pub fn tag(self) -> &'static str {
        match self {
            Category::Developer => "developer",
            Category::Stocks => "stocks",
            Category::Business => "business",
            Category::Tech => "tech",
            Category::Science => "science",
            Category::World => "world",
            Category::Culture => "culture",
            Category::Ai => "ai",
            Category::Design => "design",
            Category::Startups => "startups",
            Category::Crypto => "crypto",
            Category::Local => "local",
            Category::Sports => "sports",
            Category::Entertainment => "entertainment",
            Category::Other => "other",
        }
    }

    /// The chip label, as Settings draws it.
    pub fn label(self) -> &'static str {
        match self {
            Category::Developer => "Developer",
            Category::Stocks => "Stocks",
            Category::Business => "Business",
            Category::Tech => "Tech",
            Category::Science => "Science",
            Category::World => "World",
            Category::Culture => "Culture",
            Category::Ai => "AI",
            Category::Design => "Design",
            Category::Startups => "Startups",
            Category::Crypto => "Crypto",
            Category::Local => "Local",
            Category::Sports => "Sports",
            Category::Entertainment => "Entertainment",
            Category::Other => "Other",
        }
    }

    pub fn parse(s: &str) -> Option<Category> {
        let s = s
            .trim()
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase();
        Category::ALL.into_iter().find(|c| c.tag() == s)
    }

    /// The topic chip id a story of this category matches. `Other` matches
    /// nothing.
    pub fn topic(self) -> Option<&'static str> {
        match self {
            Category::Other => None,
            c => Some(c.tag()),
        }
    }

    pub fn from_topic(topic: &str) -> Option<Category> {
        Category::parse(topic).filter(|c| *c != Category::Other)
    }

    /// The broader chip a narrower category also belongs to: an AI story is
    /// Tech news to a user who picked Tech but not AI. `None` for the broad
    /// chips themselves and for Local (local news is no other chip's).
    pub fn parent(self) -> Option<Category> {
        match self {
            Category::Ai | Category::Design => Some(Category::Tech),
            Category::Startups => Some(Category::Business),
            Category::Crypto => Some(Category::Stocks),
            _ => None,
        }
    }

    /// The narrower chips, each matched by its own words ([`TOPIC_WORDS`])
    /// when a story's label is a broader one ([`match_prefs`]).
    pub const NARROW: [Category; 5] = [
        Category::Ai,
        Category::Design,
        Category::Startups,
        Category::Crypto,
        Category::Local,
    ];

    /// Sports and entertainment are vetoed unless the user picked them.
    pub fn is_denied_kind(self) -> bool {
        matches!(self, Category::Sports | Category::Entertainment)
    }
}

/// How a story's category was decided (`news rank` telemetry, the card).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassifiedBy {
    Source,
    Cloud,
    Keywords,
    Unknown,
}

impl ClassifiedBy {
    pub fn tag(self) -> &'static str {
        match self {
            ClassifiedBy::Source => "source",
            ClassifiedBy::Cloud => "cloud",
            ClassifiedBy::Keywords => "keywords",
            ClassifiedBy::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Classified {
    pub category: Category,
    pub by: ClassifiedBy,
}

/// The curated outlet table, names normalized by [`normalize_outlet`]
/// (lowercase words, a leading "the" and a trailing ".com" dropped). General
/// outlets (Reuters, AP, BBC News, CNN, The New York Times, ...) cover every
/// topic and are deliberately absent: their stories are classified by the
/// cloud label or the keywords.
pub const OUTLETS: &[(&str, Category)] = &[
    // Developer
    ("hacker news", Category::Developer),
    ("github blog", Category::Developer),
    ("stack overflow blog", Category::Developer),
    ("infoq", Category::Developer),
    ("dev to", Category::Developer),
    ("new stack", Category::Developer),
    ("lwn net", Category::Developer),
    ("sd times", Category::Developer),
    ("smashing magazine", Category::Developer),
    // Tech
    ("ars technica", Category::Tech),
    ("verge", Category::Tech),
    ("techcrunch", Category::Tech),
    ("wired", Category::Tech),
    ("engadget", Category::Tech),
    ("cnet", Category::Tech),
    ("zdnet", Category::Tech),
    ("gizmodo", Category::Tech),
    ("tom s hardware", Category::Tech),
    ("9to5mac", Category::Tech),
    ("9to5google", Category::Tech),
    ("macrumors", Category::Tech),
    ("appleinsider", Category::Tech),
    ("android authority", Category::Tech),
    ("mit technology review", Category::Tech),
    ("techradar", Category::Tech),
    ("venturebeat", Category::Tech),
    ("information", Category::Tech),
    ("register", Category::Tech),
    // Stocks
    ("marketwatch", Category::Stocks),
    ("barron s", Category::Stocks),
    ("investor s business daily", Category::Stocks),
    ("motley fool", Category::Stocks),
    ("seeking alpha", Category::Stocks),
    ("yahoo finance", Category::Stocks),
    ("investopedia", Category::Stocks),
    ("benzinga", Category::Stocks),
    ("zacks investment research", Category::Stocks),
    ("thestreet", Category::Stocks),
    ("kiplinger", Category::Stocks),
    ("nasdaq", Category::Stocks),
    // Business
    ("bloomberg", Category::Business),
    ("wall street journal", Category::Business),
    ("wsj", Category::Business),
    ("fortune", Category::Business),
    ("forbes", Category::Business),
    ("business insider", Category::Business),
    ("financial times", Category::Business),
    ("cnbc", Category::Business),
    ("fast company", Category::Business),
    ("harvard business review", Category::Business),
    ("quartz", Category::Business),
    // Science
    ("nature", Category::Science),
    ("scientific american", Category::Science),
    ("science", Category::Science),
    ("new scientist", Category::Science),
    ("space", Category::Science),
    ("phys org", Category::Science),
    ("live science", Category::Science),
    ("quanta magazine", Category::Science),
    ("sciencealert", Category::Science),
    ("science news", Category::Science),
    ("sciencedaily", Category::Science),
    ("national geographic", Category::Science),
    ("nasa", Category::Science),
    // World
    ("al jazeera", Category::World),
    ("al jazeera english", Category::World),
    ("foreign policy", Category::World),
    ("foreign affairs", Category::World),
    ("diplomat", Category::World),
    ("dw", Category::World),
    ("france 24", Category::World),
    ("euronews", Category::World),
    ("south china morning post", Category::World),
    ("kyiv independent", Category::World),
    // Culture
    ("new yorker", Category::Culture),
    ("smithsonian magazine", Category::Culture),
    ("artnews", Category::Culture),
    ("artnet news", Category::Culture),
    ("hyperallergic", Category::Culture),
    ("literary hub", Category::Culture),
    ("paris review", Category::Culture),
    ("aeon", Category::Culture),
    // Sports — present in a fetch plan only when the user picked Sports.
    ("espn", Category::Sports),
    ("pga tour", Category::Sports),
    ("golfweek", Category::Sports),
    ("golf digest", Category::Sports),
    ("golf channel", Category::Sports),
    ("bleacher report", Category::Sports),
    ("athletic", Category::Sports),
    ("sports illustrated", Category::Sports),
    ("si", Category::Sports),
    ("cbs sports", Category::Sports),
    ("nbc sports", Category::Sports),
    ("fox sports", Category::Sports),
    ("yahoo sports", Category::Sports),
    ("sky sports", Category::Sports),
    ("bbc sport", Category::Sports),
    ("sportskeeda", Category::Sports),
    ("ringer", Category::Sports),
    ("marca", Category::Sports),
    ("nfl", Category::Sports),
    ("nba", Category::Sports),
    ("mlb", Category::Sports),
    ("247sports", Category::Sports),
    ("on3", Category::Sports),
    // Entertainment — never in a fetch plan (no chip); dropped unless named.
    ("variety", Category::Entertainment),
    ("people", Category::Entertainment),
    ("tmz", Category::Entertainment),
    ("hollywood reporter", Category::Entertainment),
    ("deadline", Category::Entertainment),
    ("entertainment weekly", Category::Entertainment),
    ("e online", Category::Entertainment),
    ("us weekly", Category::Entertainment),
    ("billboard", Category::Entertainment),
    ("vulture", Category::Entertainment),
    ("page six", Category::Entertainment),
    ("just jared", Category::Entertainment),
    ("rolling stone", Category::Entertainment),
    ("pitchfork", Category::Entertainment),
    ("indiewire", Category::Entertainment),
    ("screen rant", Category::Entertainment),
    ("collider", Category::Entertainment),
    ("ign", Category::Entertainment),
    ("gamespot", Category::Entertainment),
    ("polygon", Category::Entertainment),
    ("kotaku", Category::Entertainment),
];

/// Words in an outlet's name that declare it sports or entertainment even
/// when the outlet is not in [`OUTLETS`] ("Golf Monthly", "Yardbarker
/// Sports", "Hollywood Life").
const OUTLET_SPORTS_WORDS: &[&str] = &[
    "sport",
    "sports",
    "golf",
    "espn",
    "football",
    "soccer",
    "basketball",
    "baseball",
    "hockey",
    "racing",
    "nfl",
    "nba",
    "mlb",
    "nhl",
];
const OUTLET_ENTERTAINMENT_WORDS: &[&str] = &[
    "entertainment",
    "hollywood",
    "celebrity",
    "movies",
    "gossip",
];

/// Sports deny-list: vetoes before any topic word. Phrases are whole-word
/// sequences.
pub const SPORTS_WORDS: &[&str] = &[
    "world wide technology",
    "world series",
    "world cup",
    "super bowl",
    "stanley cup",
    "cup final",
    "march madness",
    "final four",
    "grand slam",
    "grand prix",
    "us open",
    "u s open",
    "french open",
    "australian open",
    "british open",
    "open championship",
    "missed the cut",
    "home run",
    "series lead",
    "premier league",
    "champions league",
    "la liga",
    "formula 1",
    "formula one",
    "transfer window",
    "draft pick",
    "championship",
    "championships",
    "tournament",
    "putt",
    "putts",
    "birdie",
    "birdies",
    "bogey",
    "pga",
    "lpga",
    "golf",
    "golfer",
    "nfl",
    "nba",
    "mlb",
    "nhl",
    "mls",
    "ncaa",
    "wnba",
    "ufc",
    "nascar",
    "f1",
    "touchdown",
    "touchdowns",
    "quarterback",
    "playoff",
    "playoffs",
    "inning",
    "innings",
    "homers",
    "pitcher",
    "striker",
    "midfielder",
    "goalkeeper",
    "olympics",
    "olympic",
    "wimbledon",
    "tennis",
    "rugby",
    "cricket",
    "heavyweight",
    "qualifier",
    "semifinal",
    "semifinals",
    "quarterfinal",
];

/// Entertainment deny-list.
pub const ENTERTAINMENT_WORDS: &[&str] = &[
    "box office",
    "red carpet",
    "reality show",
    "reality tv",
    "music video",
    "season finale",
    "celebrity",
    "celebrities",
    "hollywood",
    "oscar",
    "oscars",
    "emmy",
    "emmys",
    "grammy",
    "grammys",
    "sitcom",
    "kardashian",
    "album",
    "rapper",
    "movie",
    "movies",
];

/// Curated per-topic words, matched whole-word on the headline. On a tie
/// the earlier topic wins (market words are the most specific).
pub const TOPIC_WORDS: &[(Category, &[&str])] = &[
    (
        Category::Stocks,
        &[
            "stock",
            "stocks",
            "shares",
            "stock market",
            "wall street",
            "nasdaq",
            "s p 500",
            "dow",
            "dow jones",
            "earnings",
            "investors",
            "ipo",
            "share price",
            "rally",
            "selloff",
            "sell off",
            "bull market",
            "bear market",
            "dividend",
            "dividends",
            "futures",
            "bond yields",
            "yields",
            "market cap",
            "premarket",
            "traders",
            "etf",
            "etfs",
            "price target",
            "bitcoin",
        ],
    ),
    (
        Category::Developer,
        &[
            "developer",
            "developers",
            "programming",
            "programmer",
            "programmers",
            "open source",
            "github",
            "rust",
            "python",
            "javascript",
            "typescript",
            "compiler",
            "api",
            "apis",
            "linux",
            "kernel",
            "database",
            "framework",
            "sdk",
            "pull request",
            "pull requests",
            "repository",
            "coding",
            "webassembly",
            "kubernetes",
            "docker",
            "npm",
            "git",
            "devops",
        ],
    ),
    (
        Category::Science,
        &[
            "science",
            "scientist",
            "scientists",
            "study",
            "research",
            "researchers",
            "nasa",
            "space",
            "astronomers",
            "astronomy",
            "telescope",
            "planet",
            "exoplanet",
            "mars",
            "moon",
            "asteroid",
            "galaxy",
            "physics",
            "physicists",
            "quantum",
            "chemistry",
            "biology",
            "species",
            "fossil",
            "dinosaur",
            "climate",
            "gene",
            "genes",
            "dna",
            "brain",
            "neuroscience",
            "sleep",
            "evolution",
            "rocket",
            "rockets",
            "spacecraft",
            "orbit",
            "vaccine",
            "clinical trial",
            "cancer",
            "disease",
            "nuclear fusion",
        ],
    ),
    (
        Category::Tech,
        &[
            "technology",
            "tech",
            "ai",
            "artificial intelligence",
            "chip",
            "chips",
            "chipmaker",
            "semiconductor",
            "semiconductors",
            "smartphone",
            "smartphones",
            "iphone",
            "android",
            "apple",
            "google",
            "microsoft",
            "nvidia",
            "openai",
            "software",
            "app",
            "apps",
            "gadget",
            "gadgets",
            "laptop",
            "computer",
            "computing",
            "cybersecurity",
            "hackers",
            "data breach",
            "robot",
            "robots",
            "robotics",
            "internet",
            "broadband",
            "5g",
            "self driving",
            "tesla",
            "device",
            "devices",
            "wearable",
            "virtual reality",
            "augmented reality",
            "tablet",
            "browser",
            "chatbot",
            "machine learning",
        ],
    ),
    (
        Category::Business,
        &[
            "business",
            "company",
            "companies",
            "ceo",
            "layoffs",
            "merger",
            "acquisition",
            "acquires",
            "startup",
            "startups",
            "revenue",
            "profit",
            "sales",
            "retail",
            "retailer",
            "economy",
            "economic",
            "inflation",
            "federal reserve",
            "interest rates",
            "rate cut",
            "tariff",
            "tariffs",
            "jobs report",
            "unemployment",
            "bankruptcy",
            "supply chain",
            "manufacturing",
            "funding",
            "consumers",
            "airline",
            "airlines",
            "automaker",
        ],
    ),
    (
        Category::World,
        &[
            "war",
            "ukraine",
            "russia",
            "china",
            "israel",
            "gaza",
            "hamas",
            "iran",
            "election",
            "elections",
            "prime minister",
            "minister",
            "government",
            "parliament",
            "congress",
            "senate",
            "ceasefire",
            "sanctions",
            "united nations",
            "nato",
            "diplomats",
            "military",
            "troops",
            "refugees",
            "protest",
            "protests",
            "summit",
            "eu",
            "european union",
            "taiwan",
            "venezuela",
            "politics",
            "white house",
            "immigration",
            "migration",
            "coup",
            "earthquake",
            "hurricane",
        ],
    ),
    (
        Category::Culture,
        &[
            "art",
            "arts",
            "artist",
            "artists",
            "museum",
            "museums",
            "painting",
            "gallery",
            "exhibition",
            "book",
            "books",
            "novel",
            "novelist",
            "author",
            "poet",
            "poetry",
            "literature",
            "film festival",
            "theater",
            "theatre",
            "opera",
            "ballet",
            "architecture",
            "heritage",
            "photography",
            "fashion week",
            "pulitzer",
            "booker prize",
        ],
    ),
    // fix-l3b3-4-news-new-chips-fetch-and-match — the approved page's five
    // chips. Last on purpose: on a tie the earlier (broader) topic keeps the
    // story, and match_prefs still finds these words for a picked chip.
    (
        Category::Ai,
        &[
            "ai",
            "artificial intelligence",
            "openai",
            "chatgpt",
            "chatbot",
            "chatbots",
            "machine learning",
            "deep learning",
            "neural network",
            "llm",
            "llms",
            "large language model",
            "language model",
            "generative ai",
            "gpt",
            "anthropic",
            "claude",
            "gemini",
            "deepmind",
            "copilot",
            "agi",
            "ai model",
            "ai models",
            "new model",
        ],
    ),
    (
        Category::Design,
        &[
            "design",
            "designs",
            "designer",
            "designers",
            "redesign",
            "redesigned",
            "ux",
            "user interface",
            "user experience",
            "typography",
            "typeface",
            "font",
            "fonts",
            "logo",
            "rebrand",
            "branding",
            "figma",
            "graphic design",
            "product design",
            "industrial design",
            "web design",
        ],
    ),
    (
        Category::Startups,
        &[
            "startup",
            "startups",
            "founder",
            "founders",
            "venture capital",
            "venture",
            "seed round",
            "pre seed",
            "series a",
            "series b",
            "series c",
            "funding round",
            "raises",
            "unicorn",
            "y combinator",
            "accelerator",
            "angel investor",
            "angel investors",
        ],
    ),
    (
        Category::Crypto,
        &[
            "crypto",
            "cryptocurrency",
            "cryptocurrencies",
            "bitcoin",
            "btc",
            "ethereum",
            "blockchain",
            "stablecoin",
            "stablecoins",
            "coinbase",
            "binance",
            "solana",
            "dogecoin",
            "nft",
            "nfts",
            "defi",
        ],
    ),
    (
        Category::Local,
        &[
            "local",
            "city council",
            "mayor",
            "county",
            "school board",
            "school district",
            "city hall",
            "sheriff",
            "neighborhood",
            "zoning",
            "downtown",
            "township",
            "commissioners",
            "city budget",
            "town hall",
        ],
    ),
];

/// Lowercase words joined by single spaces, padded so `contains(" w ")` is
/// a whole-word (or whole-phrase) test.
fn padded_words(s: &str) -> String {
    let words: Vec<String> = s
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    format!(" {} ", words.join(" "))
}

fn has_phrase(padded: &str, phrase: &str) -> bool {
    padded.contains(&format!(" {phrase} "))
}

/// Outlet name as the table spells it: "The Verge" -> "verge",
/// "People.com" -> "people", "Barron's" -> "barron s".
pub fn normalize_outlet(outlet: &str) -> String {
    let p = padded_words(outlet);
    let mut s = p.trim();
    if let Some(rest) = s.strip_prefix("the ") {
        s = rest;
    }
    if let Some(rest) = s.strip_suffix(" com") {
        s = rest;
    }
    s.to_string()
}

/// The outlet's declared category: the table first, then sports or
/// entertainment words in its name.
pub fn outlet_category(outlet: &str) -> Option<Category> {
    let norm = normalize_outlet(outlet);
    if norm.is_empty() {
        return None;
    }
    if let Some((_, c)) = OUTLETS.iter().find(|(name, _)| *name == norm) {
        return Some(*c);
    }
    let padded = format!(" {norm} ");
    if OUTLET_SPORTS_WORDS.iter().any(|w| has_phrase(&padded, w)) {
        return Some(Category::Sports);
    }
    if OUTLET_ENTERTAINMENT_WORDS
        .iter()
        .any(|w| has_phrase(&padded, w))
    {
        return Some(Category::Entertainment);
    }
    None
}

/// Step 1: the source's declared category, when it has one.
pub fn declared_category(source: Source, outlet: Option<&str>) -> Option<Category> {
    match source {
        Source::HackerNews => Some(Category::Developer),
        Source::Quotes => Some(Category::Stocks),
        Source::GoogleNews => outlet.and_then(outlet_category),
    }
}

/// Step 3 (offline only): the deny-lists veto first, then the topic with
/// the most whole-word hits; `Other` when nothing matches.
pub fn keyword_category(title: &str) -> Category {
    let padded = padded_words(title);
    if SPORTS_WORDS.iter().any(|w| has_phrase(&padded, w)) {
        return Category::Sports;
    }
    if ENTERTAINMENT_WORDS.iter().any(|w| has_phrase(&padded, w)) {
        return Category::Entertainment;
    }
    let mut best = (0usize, Category::Other);
    for (cat, words) in TOPIC_WORDS {
        let hits = words.iter().filter(|w| has_phrase(&padded, w)).count();
        if hits > best.0 {
            best = (hits, *cat);
        }
    }
    best.1
}

/// The full order: declared source, then the cached cloud label, then the
/// offline keywords.
pub fn classify(story: &Story, cloud: Option<Category>) -> Classified {
    if let Some(category) = declared_category(story.source, story.outlet.as_deref()) {
        return Classified {
            category,
            by: ClassifiedBy::Source,
        };
    }
    if let Some(category) = cloud {
        return Classified {
            category,
            by: ClassifiedBy::Cloud,
        };
    }
    match keyword_category(&story.title) {
        Category::Other => Classified {
            category: Category::Other,
            by: ClassifiedBy::Unknown,
        },
        category => Classified {
            category,
            by: ClassifiedBy::Keywords,
        },
    }
}

/// Why a story is in the briefing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub category: Category,
    pub by: ClassifiedBy,
    /// The user's topic id, ticker or own word it matches.
    pub topic: String,
    /// Matched by a ticker or own word rather than a topic chip.
    pub own_word: bool,
}

/// Does the classified story belong in this user's briefing? A topic chip
/// matches its category; a ticker or own word matches when the headline
/// names it (the user typed it, so it is kept whatever its category). `None`
/// = off topic: the caller drops it.
///
/// fix-l3b3-4-news-new-chips-fetch-and-match: after the exact chip, a
/// picked narrow chip (AI, Design, Startups, Crypto, Local) keeps a story
/// whose headline has that chip's own words even when its label is a
/// broader one ("OpenAI ships a new model" labelled Tech still shows for
/// AI); then a narrow label reaches the broader chip a user picked
/// ([`Category::parent`]: an AI story shows as Tech for a Tech-only user).
/// The sports/entertainment veto stands: a vetoed story only ever matches
/// its own chip or an own word.
pub fn match_prefs(story: &Story, prefs: &NewsPrefs, cloud: Option<Category>) -> Option<Match> {
    let c = classify(story, cloud);
    let picked = |cat: Category| prefs.topics.iter().any(|p| p == cat.tag());
    let chip = |category: Category| Match {
        category,
        by: c.by,
        topic: category.tag().to_string(),
        own_word: false,
    };
    if let Some(t) = c.category.topic() {
        if prefs.topics.iter().any(|p| p == t) {
            return Some(Match {
                category: c.category,
                by: c.by,
                topic: t.to_string(),
                own_word: false,
            });
        }
    }
    let padded = padded_words(&story.title);
    if !c.category.is_denied_kind() {
        if let Some(narrow) = Category::NARROW
            .into_iter()
            .filter(|n| picked(*n))
            .find(|n| topic_words(*n).iter().any(|w| has_phrase(&padded, w)))
        {
            return Some(chip(narrow));
        }
        if let Some(parent) = c.category.parent().filter(|p| picked(*p)) {
            return Some(chip(parent));
        }
    }
    let named = prefs.tickers.iter().chain(prefs.custom.iter()).find(|w| {
        let w = padded_words(w);
        let w = w.trim();
        !w.is_empty() && has_phrase(&padded, w)
    })?;
    Some(Match {
        category: c.category,
        by: c.by,
        topic: named.clone(),
        own_word: true,
    })
}

/// The curated words of one topic (empty for a category with none).
fn topic_words(category: Category) -> &'static [&'static str] {
    TOPIC_WORDS
        .iter()
        .find(|(c, _)| *c == category)
        .map(|(_, w)| *w)
        .unwrap_or(&[])
}

/// The card's "Why this story" line, e.g. "From Ars Technica (Tech) ·
/// matches Tech". Never a URL.
pub fn why_line(story: &Story, m: &Match) -> String {
    let from = match (story.source, story.outlet.as_deref()) {
        (Source::HackerNews, _) => "Hacker News".to_string(),
        (_, Some(o)) if !o.trim().is_empty() => o.trim().to_string(),
        _ => "the news".to_string(),
    };
    let matches = if m.own_word {
        format!("matches \u{201c}{}\u{201d}", m.topic)
    } else {
        format!(
            "matches {}",
            Category::from_topic(&m.topic)
                .map(Category::label)
                .unwrap_or(m.topic.as_str())
        )
    };
    format!("From {from} ({}) \u{b7} {matches}", m.category.label())
}

/// One planned fetch: which source serves a picked topic, its search words
/// and the declared outlets that topic draws on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFetch {
    pub topic: &'static str,
    pub source: Source,
    pub words: Option<&'static str>,
    pub category: Category,
    pub outlets: Vec<&'static str>,
}

/// The fetch plan for the user's topic chips, in their order. Only picked
/// topics appear, so sports outlets are in the plan only when Sports is
/// picked and entertainment outlets never are (there is no chip).
///
/// A picked chip that gets no fetch is logged once as
/// `news topic_unplanned topic=<id>` (fix-l3b3-4-news-new-chips-fetch-and-match:
/// the five new chips were silently skipped before), so an empty briefing
/// is never unexplained.
pub fn fetch_plan(topics: &[String]) -> Vec<PlannedFetch> {
    let mut plan: Vec<PlannedFetch> = Vec::new();
    let unplanned = |t: &str| {
        let id = if crate::news_prefs::is_topic_chip(t) {
            t
        } else {
            "unknown"
        };
        log::info!("news topic_unplanned topic={id}");
    };
    for t in topics {
        let Some(category) = Category::from_topic(t) else {
            unplanned(t.as_str());
            continue;
        };
        if category == Category::Entertainment || plan.iter().any(|p| p.category == category) {
            continue;
        }
        let (source, words) = if category == Category::Developer {
            (Source::HackerNews, None)
        } else {
            match google_query(category.tag()) {
                Some(w) => (Source::GoogleNews, Some(w)),
                None => {
                    unplanned(t.as_str());
                    continue;
                }
            }
        };
        plan.push(PlannedFetch {
            topic: category.tag(),
            source,
            words,
            category,
            outlets: OUTLETS
                .iter()
                .filter(|(_, c)| *c == category)
                .map(|(n, _)| *n)
                .collect(),
        });
    }
    plan
}

// ---- the cloud label (step 2) ----

/// Most stories sent in the one call of a refresh.
pub const CLASSIFY_MAX: usize = 40;
/// One short label per story.
pub const CLASSIFY_MAX_TOKENS: u32 = 400;
pub const CLASSIFY_BUDGET: Budget = Budget {
    ttft: Duration::from_millis(3000),
    total: Duration::from_secs(8),
};
const MAX_TITLE_CHARS: usize = 200;

/// The one classification request: headline and outlet per story, numbered;
/// the answer is one closed label per line.
pub fn classify_prompt(items: &[(String, Option<String>)]) -> (String, String) {
    let labels: Vec<&str> = Category::ALL.iter().map(|c| c.tag()).collect();
    let system = format!(
        "You sort news headlines into topics. Answer with one line per headline: its number, \
         then exactly one label from this list: {}. A sports or entertainment story is sports or \
         entertainment even when it names a company, a sponsor or the word technology. Use other \
         when none fits. No other text.",
        labels.join(", ")
    );
    let user = items
        .iter()
        .enumerate()
        .map(|(i, (title, outlet))| {
            let t: String = title.chars().take(MAX_TITLE_CHARS).collect();
            match outlet {
                Some(o) => format!("{}. {} ({})", i + 1, t, o.trim()),
                None => format!("{}. {}", i + 1, t),
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    (system, user)
}

/// Read "3. tech" / "3: tech" / "3 - sports" lines; unknown labels and
/// numbers out of range are ignored.
pub fn parse_labels(text: &str, n: usize) -> Vec<Option<Category>> {
    let mut out = vec![None; n];
    for line in text.lines() {
        let line = line.trim();
        let digits: String = line.chars().take_while(|c| c.is_ascii_digit()).collect();
        let Ok(i) = digits.parse::<usize>() else {
            continue;
        };
        if i == 0 || i > n {
            continue;
        }
        let rest = &line[digits.len()..];
        if let Some(c) = rest
            .split(|c: char| !c.is_alphanumeric())
            .find(|w| !w.is_empty())
            .and_then(Category::parse)
        {
            out[i - 1] = Some(c);
        }
    }
    out
}

/// The shipping cloud seam: `(db, system, user) -> answer or fallback tag`,
/// installed once at app setup by `lib.rs` (the BYOM route: mode, key, cap,
/// breaker, daily ledger).
pub type CloudCall =
    dyn Fn(&crate::db::Database, &str, &str) -> Result<String, &'static str> + Send + Sync;

static CLOUD: OnceLock<Box<CloudCall>> = OnceLock::new();

pub fn install_cloud(call: Box<CloudCall>) {
    let _ = CLOUD.set(call);
}

pub fn cloud() -> Option<&'static CloudCall> {
    CLOUD.get().map(|b| b.as_ref())
}

/// Label lookup by url key, as [`super::cache::cloud_labels`] returns it.
pub type Labels = HashMap<String, Category>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::news::rank::tests::{prefs, story, NOW};
    use crate::news::rank::{keyed, rank};

    #[derive(Deserialize)]
    struct Headline {
        title: String,
        outlet: Option<String>,
        source: Source,
    }

    #[derive(Deserialize)]
    struct Label {
        title: String,
        category: Category,
    }

    const HEADLINES: &str = include_str!("../../tests/fixtures/news_headlines_60.json");
    const LABELS: &str = include_str!("../../tests/fixtures/news_headlines_60.labels.json");

    fn fixture() -> Vec<(Story, Category)> {
        let h: Vec<Headline> = serde_json::from_str(HEADLINES).expect("headlines parse");
        let l: Vec<Label> = serde_json::from_str(LABELS).expect("labels parse");
        assert_eq!(h.len(), 60, "fixture holds 60 headlines");
        assert_eq!(l.len(), 60, "answer key holds 60 labels");
        h.into_iter()
            .zip(l)
            .enumerate()
            .map(|(i, (h, l))| {
                assert_eq!(
                    h.title, l.title,
                    "answer key row {i} is for another headline"
                );
                let mut s = story(
                    "tech",
                    &h.title,
                    &format!("https://example.com/{i}"),
                    h.outlet.as_deref(),
                    Some(1),
                );
                s.source = h.source;
                (s, l.category)
            })
            .collect()
    }

    fn wilson() -> NewsPrefs {
        prefs(
            &[
                "developer",
                "stocks",
                "business",
                "tech",
                "world",
                "culture",
                "science",
            ],
            &[],
            &[],
        )
    }

    #[test]
    fn sixty_headlines_match_the_answer_key_with_no_sport_in_tech() {
        let rows = fixture();
        let mut agree = 0usize;
        let mut misses = Vec::new();
        let mut leaks = Vec::new();
        for (s, want) in &rows {
            // Every row was stamped "tech" by its search: the stamp is ignored.
            let got = classify(s, None).category;
            if got == *want {
                agree += 1;
            } else {
                misses.push(format!("{:?} -> {:?} (want {:?})", s.title, got, want));
            }
            if got == Category::Tech && want.is_denied_kind() {
                leaks.push(s.title.clone());
            }
        }
        println!("news classify fixture agree={agree}/60 misses={misses:?}");
        assert!(leaks.is_empty(), "sports/entertainment in Tech: {leaks:?}");
        assert!(agree >= 57, "agreement {agree}/60 below 95%: {misses:?}");
    }

    #[test]
    fn the_three_golf_headlines_are_sports_with_or_without_the_outlet() {
        for (s, _) in fixture().iter().take(3) {
            assert_eq!(classify(s, None).category, Category::Sports, "{}", s.title);
            assert_eq!(keyword_category(&s.title), Category::Sports, "{}", s.title);
            assert!(match_prefs(s, &wilson(), None).is_none(), "{}", s.title);
        }
    }

    #[test]
    fn declared_source_beats_keywords_and_cloud() {
        let mut golf = story(
            "tech",
            "Technology giant sponsors the event",
            "https://pgatour.com/1",
            Some("PGA TOUR"),
            Some(1),
        );
        assert_eq!(keyword_category(&golf.title), Category::Tech);
        let c = classify(&golf, Some(Category::Tech));
        assert_eq!((c.category, c.by), (Category::Sports, ClassifiedBy::Source));
        golf.outlet = Some("Golfweek".into());
        assert_eq!(classify(&golf, None).category, Category::Sports);
        golf.outlet = Some("Golf Monthly".into());
        assert_eq!(classify(&golf, None).category, Category::Sports);
        let hn = Story {
            source: Source::HackerNews,
            outlet: None,
            ..golf.clone()
        };
        assert_eq!(classify(&hn, None).category, Category::Developer);
    }

    #[test]
    fn cloud_label_beats_keywords_for_an_undeclared_outlet() {
        let s = story(
            "tech",
            "Apple unveils new chip",
            "https://a.com/1",
            Some("Reuters"),
            Some(1),
        );
        assert_eq!(classify(&s, None).by, ClassifiedBy::Keywords);
        let c = classify(&s, Some(Category::Business));
        assert_eq!(
            (c.category, c.by),
            (Category::Business, ClassifiedBy::Cloud)
        );
    }

    #[test]
    fn keywords_are_whole_word_and_the_deny_list_vetoes_first() {
        // "world" inside an event name is never a World hit.
        assert_eq!(
            keyword_category("Big names at the World Wide Technology Championship"),
            Category::Sports
        );
        assert_eq!(
            keyword_category("World Series Game 7 tonight"),
            Category::Sports
        );
        assert_eq!(
            keyword_category("World Cup qualifier in Lima"),
            Category::Sports
        );
        assert_eq!(
            keyword_category("Tech billionaire buys NBA team"),
            Category::Sports
        );
        assert_eq!(
            keyword_category("Box office slumps again"),
            Category::Entertainment
        );
        // Whole word: "rustic" is not Rust, "aid" is not AI, "warning" is not war.
        assert_eq!(
            keyword_category("Rustic cabins and warning signs aid"),
            Category::Other
        );
        assert_eq!(keyword_category("Technology stocks fall"), Category::Stocks);
        assert_eq!(keyword_category("The science of sleep"), Category::Science);
    }

    #[test]
    fn off_topic_stories_are_dropped_never_relabelled() {
        let rows = fixture();
        let sports_titles: Vec<String> = rows
            .iter()
            .filter(|(_, want)| want.is_denied_kind())
            .map(|(s, _)| s.title.clone())
            .collect();
        assert!(sports_titles.len() >= 12);
        let items = rows.into_iter().map(|(s, _)| keyed(s)).collect();
        let (out, off_topic) = rank(items, &wilson(), NOW, 100);
        assert!(off_topic >= sports_titles.len(), "off_topic={off_topic}");
        for r in &out {
            assert!(
                !r.category.is_denied_kind(),
                "{} kept as {:?}",
                r.story.title,
                r.category
            );
            assert!(!sports_titles.contains(&r.story.title), "{}", r.story.title);
            // The label is the story's own category, never the search stamp.
            assert_eq!(r.story.topic, r.category.tag(), "{}", r.story.title);
        }
        assert!(out.iter().any(|r| r.category == Category::Tech));
    }

    #[test]
    fn own_words_keep_a_story_whatever_its_category() {
        let s = story(
            "lakers",
            "Lakers win in overtime",
            "https://espn.com/1",
            Some("ESPN"),
            Some(1),
        );
        assert!(match_prefs(&s, &wilson(), None).is_none());
        let p = prefs(&["tech"], &[], &["lakers"]);
        let m = match_prefs(&s, &p, None).expect("own word matches");
        assert!(m.own_word);
        assert_eq!(m.category, Category::Sports);
        assert_eq!(
            why_line(&s, &m),
            "From ESPN (Sports) \u{b7} matches \u{201c}lakers\u{201d}"
        );
    }

    #[test]
    fn why_line_names_source_category_and_matched_topic() {
        let s = story(
            "tech",
            "Apple ships a new laptop",
            "https://arstechnica.com/1",
            Some("Ars Technica"),
            Some(1),
        );
        let m = match_prefs(&s, &wilson(), None).expect("tech matches");
        assert_eq!(
            why_line(&s, &m),
            "From Ars Technica (Tech) \u{b7} matches Tech"
        );
    }

    #[test]
    fn fetch_plan_lists_sports_only_when_sports_is_picked() {
        let topics: Vec<String> = wilson().topics;
        let plan = fetch_plan(&topics);
        assert_eq!(plan.len(), 7);
        for p in &plan {
            assert!(!p.category.is_denied_kind(), "{:?}", p.topic);
            for o in &p.outlets {
                assert!(
                    !outlet_category(o).is_some_and(Category::is_denied_kind),
                    "{o} in the {} plan",
                    p.topic
                );
            }
        }
        assert_eq!(
            plan.iter()
                .find(|p| p.topic == "developer")
                .map(|p| p.source),
            Some(Source::HackerNews)
        );
        let mut with_sports = topics.clone();
        with_sports.push("sports".into());
        let plan = fetch_plan(&with_sports);
        let sports = plan.iter().find(|p| p.topic == "sports").expect("sports");
        assert!(sports.outlets.contains(&"espn"));
        assert!(!plan.iter().any(|p| p.category == Category::Entertainment));
    }

    // ---- fix-l3b3-4-news-new-chips-fetch-and-match ----

    #[test]
    fn every_topic_chip_gets_exactly_one_planned_fetch() {
        use crate::news_prefs::{LEGACY_TOPIC_CHIPS, TOPIC_CHIPS};
        for id in TOPIC_CHIPS.iter().chain(LEGACY_TOPIC_CHIPS.iter()) {
            let plan = fetch_plan(&[id.to_string()]);
            assert_eq!(plan.len(), 1, "chip {id} planned {plan:?}");
            assert_eq!(plan[0].topic, *id);
            assert_eq!(plan[0].category.tag(), *id);
            if *id == "developer" {
                assert_eq!(plan[0].source, Source::HackerNews);
            } else {
                assert_eq!(plan[0].source, Source::GoogleNews, "chip {id}");
                assert_eq!(plan[0].words, google_query(id), "chip {id}");
                assert!(plan[0].words.is_some(), "chip {id} has no search words");
            }
        }
        // All ten current chips together: ten fetches, in the user's order.
        let all: Vec<String> = TOPIC_CHIPS.iter().map(|s| s.to_string()).collect();
        let plan = fetch_plan(&all);
        assert_eq!(
            plan.iter().map(|p| p.topic).collect::<Vec<_>>(),
            TOPIC_CHIPS.to_vec()
        );
        // The legacy picks still plan alongside the new ones.
        let legacy = fetch_plan(&[
            "tech".to_string(),
            "world".to_string(),
            "culture".to_string(),
            "ai".to_string(),
        ]);
        assert_eq!(
            legacy.iter().map(|p| p.topic).collect::<Vec<_>>(),
            vec!["tech", "world", "culture", "ai"]
        );
        // A non-chip id plans nothing (and is logged as topic_unplanned).
        assert!(fetch_plan(&["entertainment".to_string(), "banana".to_string()]).is_empty());
    }

    #[test]
    fn each_new_chip_keeps_a_plainly_matching_headline() {
        let cases = [
            ("ai", "OpenAI ships a new model"),
            ("design", "Figma unveils a redesign of its design tools"),
            (
                "startups",
                "Fintech startup raises seed round led by Y Combinator",
            ),
            ("crypto", "Bitcoin passes 100k"),
            ("local", "Austin city council approves budget"),
        ];
        for (i, (chip, title)) in cases.into_iter().enumerate() {
            let s = story(
                chip,
                title,
                &format!("https://example.com/new-chip/{i}"),
                Some("Reuters"),
                Some(1),
            );
            let p = prefs(&[chip], &[], &[]);
            let m = match_prefs(&s, &p, None).unwrap_or_else(|| panic!("{chip}: {title} dropped"));
            assert_eq!(m.topic, chip, "{title}");
            assert!(!m.own_word, "{title}");
            assert_eq!(m.category.tag(), chip, "{title}");
            assert_eq!(
                why_line(&s, &m),
                format!(
                    "From Reuters ({0}) \u{b7} matches {0}",
                    Category::from_topic(chip).expect("chip category").label()
                )
            );
            // Only the picked chip keeps it: Developer alone drops it.
            assert!(
                match_prefs(&s, &prefs(&["developer"], &[], &[]), None).is_none(),
                "{title} kept for developer"
            );
        }
        // A cloud label of the new chip itself matches directly.
        let s = story(
            "ai",
            "A quiet headline",
            "https://example.com/c",
            None,
            Some(1),
        );
        let m = match_prefs(&s, &prefs(&["ai"], &[], &[]), Some(Category::Ai)).expect("cloud ai");
        assert_eq!((m.topic.as_str(), m.by), ("ai", ClassifiedBy::Cloud));
    }

    #[test]
    fn a_narrow_label_reaches_the_broad_chip_and_the_veto_stands() {
        // Labelled AI, the user picked only Tech: shown as Tech.
        let s = story(
            "tech",
            "OpenAI ships a new model",
            "https://example.com/n1",
            None,
            Some(1),
        );
        assert_eq!(classify(&s, None).category, Category::Ai);
        let m = match_prefs(&s, &prefs(&["tech"], &[], &[]), None).expect("tech parent");
        assert_eq!((m.topic.as_str(), m.category), ("tech", Category::Tech));
        assert_eq!(Category::Crypto.parent(), Some(Category::Stocks));
        assert_eq!(Category::Startups.parent(), Some(Category::Business));
        assert_eq!(Category::Local.parent(), None);
        // A sports story naming AI is still sports: AI alone drops it.
        let golf = story(
            "ai",
            "Golf tour adopts AI line judges",
            "https://example.com/n2",
            None,
            Some(1),
        );
        assert_eq!(classify(&golf, None).category, Category::Sports);
        assert!(match_prefs(&golf, &prefs(&["ai", "tech"], &[], &[]), None).is_none());
    }

    #[test]
    fn classify_prompt_lists_the_new_chips_and_ends_with_other() {
        assert_eq!(Category::ALL.last(), Some(&Category::Other));
        let (system, _) = classify_prompt(&[("x".to_string(), None)]);
        assert!(
            system.contains(
                "culture, ai, design, startups, crypto, local, sports, entertainment, other."
            ),
            "{system}"
        );
        assert_eq!(
            parse_labels("1. ai\n2. Design\n3: startups\n4 crypto\n5 - local", 5),
            vec![
                Some(Category::Ai),
                Some(Category::Design),
                Some(Category::Startups),
                Some(Category::Crypto),
                Some(Category::Local)
            ]
        );
        for c in Category::NARROW {
            assert_eq!(Category::from_topic(c.tag()), Some(c));
            assert!(crate::news_prefs::is_topic_chip(c.tag()));
        }
    }

    #[test]
    fn parses_labels_and_builds_a_closed_prompt() {
        let (system, user) = classify_prompt(&[
            ("Chip stocks rally".to_string(), Some("Reuters".to_string())),
            ("Putt wins it".to_string(), None),
        ]);
        assert!(system.contains("sports") && system.contains("other"));
        assert_eq!(user, "1. Chip stocks rally (Reuters)\n2. Putt wins it");
        assert_eq!(
            parse_labels("1. stocks\n2: Sports\n3 tech\nnoise", 2),
            vec![Some(Category::Stocks), Some(Category::Sports)]
        );
        assert_eq!(parse_labels("2 - banana", 2), vec![None, None]);
    }
}
