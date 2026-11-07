//! Yappy News, the fetch side. What the user asked for lives in
//! [`crate::news_prefs`]; this module turns those prefs into requests to
//! public feeds ([`feeds`]). Nothing here runs unless the cloud option is on
//! and at least one topic is set ([`feeds::gate`]).

pub mod feeds;
/// yap-1106-fb-14-news-quote-key — the optional own quote-feed key (Keychain).
pub mod quote_key;
