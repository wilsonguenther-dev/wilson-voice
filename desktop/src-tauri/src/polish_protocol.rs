//! Wire protocol for the `yap-polish` sidecar (YV60).
//!
//! The polish stage runs in a SEPARATE PROCESS, not in this binary:
//! `transcribe-cpp-sys` vendors `ggml` and links it statically into the app,
//! and `llama-cpp-sys-2` vendors its own copy — linking both into one binary is
//! a duplicate-symbol failure (llama.cpp #9267/#11303/#11491, whisper.cpp
//! #1887). A second binary sidesteps the linker entirely *and* buys a hard
//! deadline: the parent can drop a late response and kill the child, which an
//! in-process `llama_decode` can never be made to do safely.
//!
//! Transport is newline-delimited JSON on the child's stdin/stdout — one
//! request per line, one response per line. No port, no listener, no network
//! surface (the "no outbound connections" posture stays green).
//!
//! ```jsonc
//! // ← stdout, once, as soon as the model is resident (YV75)
//! {"type":"ready","version":"0.6.0","model_loaded":true}
//! // → stdin
//! {"id":7,"mode":"email","style":"default","max_out":96,"deadline_ms":1200,
//!  "text":"hey jordan quick update…","topic":null}
//! // ← stdout
//! {"id":7,"ok":true,"text":"Hey Jordan,\n\nQuick update…","out_tokens":61,"ms":540}
//! {"id":7,"ok":false,"err":"deadline"}
//! ```
//!
//! YV97 added two more request KINDS on the same line protocol, so the meeting
//! summarizer runs on this same binary and this same wire (it spawns its own
//! short-lived child rather than borrowing the dictation path's warm one — see
//! `summarize::SidecarSession` for why that matters to dictation latency):
//!
//! ```jsonc
//! // → a constrained MAP pass: the grammar rides ON the request, because it
//! //   enumerates the segment ids of THIS chunk and nothing else.
//! {"id":8,"kind":"summarize","mode":"map","style":"default","max_out":288,
//!  "deadline_ms":30000,"text":"seg_0001: …","grammar":"root ::= …"}
//! // → the tokenizer, exposed: chunk sizing measured in the real vocabulary
//! {"id":9,"kind":"count_tokens","mode":"","style":"default","max_out":0,
//!  "deadline_ms":5000,"text":"…"}
//! // ← the count, typed and mirrored into `text`
//! {"id":9,"ok":true,"text":"417","ms":3,"tokens":417}
//! ```
//!
//! **This file is the single source of truth for both sides.** `yap-polish`
//! compiles it directly (`#[path = "../../src-tauri/src/polish_protocol.rs"]`)
//! rather than keeping a second copy, so the two ends cannot drift. That is why
//! it depends on nothing but `serde` + `std` — no Tauri, no crate-local types.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

// ── YV97 · request kinds ────────────────────────────────────────────────────
//
// The sidecar started as a rewriter and one request shape was all it needed.
// The meeting summarizer needs two more things from the SAME warm process — a
// constrained generation, and the tokenizer that already exists inside it — so
// the wire grows a `kind` discriminator rather than a second sidecar.

/// The original rewrite request. The default when a line carries no `kind`, so
/// every request written before YV97 still means what it meant.
pub const KIND_POLISH: &str = "polish";
/// A summarization pass (MAP or REDUCE — see [`PolishRequest::mode`], which
/// carries the stage for this kind).
pub const KIND_SUMMARIZE: &str = "summarize";
/// "How many tokens is this text, in YOUR vocabulary?" — no decode, no
/// generation, no model output. Finding #35: chunk sizing measured with the
/// 1.3-tokens/word proxy runs well under the truth on meeting-shaped text
/// (proper nouns, disfluencies), and the sidecar is the only process that holds
/// the actual vocabulary.
pub const KIND_COUNT_TOKENS: &str = "count_tokens";

/// MAP stage of a summarize request.
pub const SUMMARIZE_MAP: &str = "map";
/// REDUCE stage of a summarize request.
pub const SUMMARIZE_REDUCE: &str = "reduce";

/// Output budget for one MAP pass, in tokens (finding #35).
///
/// Deliberately NOT [`max_out_for`]: that formula is `input × 1.4 + 24`, written
/// for a rewriter where output ≈ input, and handing it to a *summary* gives the
/// summary a larger budget than the thing it summarizes. A per-chunk extraction
/// is a fixed-size object — a handful of actions, decisions and questions — so
/// it gets a fixed budget.
pub const SUMMARY_MAP_MAX_OUT: u32 = 288;
/// Output budget for the REDUCE narrative, in tokens. One paragraph or three,
/// never a re-transcript.
pub const SUMMARY_REDUCE_MAX_OUT: u32 = 384;

/// The GBNF start symbol every generated grammar uses.
pub const GRAMMAR_ROOT: &str = "root";

fn default_kind() -> String {
    KIND_POLISH.to_string()
}

/// One request. Serialized as a single line on the sidecar's stdin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolishRequest {
    /// Monotonic per-process request id. Responses that do not carry it are
    /// stale (a previous dictation the parent stopped waiting for) and MUST be
    /// discarded — see [`parse_response_for`].
    pub id: u64,
    /// [`KIND_POLISH`] (the default, and what a pre-YV97 line deserializes as),
    /// [`KIND_SUMMARIZE`], or [`KIND_COUNT_TOKENS`].
    #[serde(default = "default_kind")]
    pub kind: String,
    /// For [`KIND_POLISH`]: the dictation mode, lowercased — `email` `document`
    /// `notes` `chat` `plain` `list`; `code` never reaches the model at all.
    /// For [`KIND_SUMMARIZE`]: the stage, [`SUMMARIZE_MAP`] or
    /// [`SUMMARIZE_REDUCE`]. Ignored for [`KIND_COUNT_TOKENS`].
    pub mode: String,
    /// Tone dial: `very casual` | `casual` | `default` | `formal`.
    pub style: String,
    /// Hard cap on generated tokens. A rewriter that runs long is looping.
    pub max_out: u32,
    /// Server-side deadline. The sidecar stops decoding and answers
    /// `{"ok":false,"err":"deadline"}`; the parent enforces its own timeout on
    /// top of this, because a wedged child cannot answer at all.
    pub deadline_ms: u64,
    /// The rules-stage text to rewrite.
    pub text: String,
    /// Optional pushed session topic. NEVER the AX cursor context — that steers
    /// decisions in-process and is not sent anywhere, not even locally.
    #[serde(default)]
    pub topic: Option<String>,
    /// GBNF for a constrained decode, rooted at [`GRAMMAR_ROOT`]. Present ⇒ the
    /// sidecar builds a per-request sampler CHAIN with the grammar ahead of
    /// greedy (finding #17); absent ⇒ plain greedy, exactly as before.
    ///
    /// It lives on the request, not on the sidecar, because the summarizer
    /// generates a different grammar for every chunk: the evidence field is an
    /// enum over the segment ids present in THAT chunk (finding #18), which is
    /// what turns provenance from a model behaviour into a mechanical one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grammar: Option<String>,
}

impl PolishRequest {
    /// A rewrite request — the pre-YV97 shape, unconstrained.
    pub fn polish(
        id: u64,
        mode: &str,
        style: &str,
        max_out: u32,
        deadline_ms: u64,
        text: String,
    ) -> Self {
        Self {
            id,
            kind: KIND_POLISH.to_string(),
            mode: mode.to_string(),
            style: style.to_string(),
            max_out,
            deadline_ms,
            text,
            topic: None,
            grammar: None,
        }
    }

    /// One summarization pass. `grammar` constrains the output shape (MAP);
    /// `None` leaves the narrative free text (REDUCE).
    pub fn summarize(
        id: u64,
        stage: &str,
        max_out: u32,
        deadline_ms: u64,
        text: String,
        grammar: Option<String>,
    ) -> Self {
        Self {
            id,
            kind: KIND_SUMMARIZE.to_string(),
            mode: stage.to_string(),
            style: "default".to_string(),
            max_out,
            deadline_ms,
            text,
            topic: None,
            grammar,
        }
    }

    /// Ask the sidecar's tokenizer how long this text is. No decode happens.
    pub fn count_tokens(id: u64, deadline_ms: u64, text: String) -> Self {
        Self {
            id,
            kind: KIND_COUNT_TOKENS.to_string(),
            mode: String::new(),
            style: "default".to_string(),
            max_out: 0,
            deadline_ms,
            text,
            topic: None,
            grammar: None,
        }
    }

    /// The grammar for this request, if it carries a usable one.
    pub fn grammar_text(&self) -> Option<&str> {
        self.grammar
            .as_deref()
            .map(str::trim)
            .filter(|g| !g.is_empty())
    }
}

/// Which sampler the sidecar must build for one request (finding #17).
///
/// Grammar state is PER GRAMMAR: a single `LlamaSampler::greedy()` built once
/// outside the serve loop and `.reset()` per request cannot carry one, and a
/// grammar bolted onto a shared instance would leak one request's constraint
/// into the next. So the sampler is built per request, from this plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SamplerPlan {
    /// `LlamaSampler::greedy()` — a rewriter is deterministic.
    Greedy,
    /// `LlamaSampler::chain_simple([grammar(model, …), greedy()])`. Order is
    /// load-bearing: the grammar masks the logits, greedy then picks from what
    /// survives. Greedy first would select a token the grammar forbids.
    GrammarThenGreedy { grammar: String, root: String },
}

/// The sampler this request needs. Pure, so both ends agree on the rule and the
/// decision is testable without a model resident.
pub fn sampler_plan(req: &PolishRequest) -> SamplerPlan {
    match req.grammar_text() {
        Some(grammar) => SamplerPlan::GrammarThenGreedy {
            grammar: grammar.to_string(),
            root: GRAMMAR_ROOT.to_string(),
        },
        None => SamplerPlan::Greedy,
    }
}

/// The `type` value of the readiness line. The only message on this wire that
/// is not keyed by a request id, which is exactly how the two are told apart.
pub const READY_KIND: &str = "ready";

/// The sidecar's readiness announcement (YV75), written to stdout ONCE, after
/// the model is resident and before any request is served.
///
/// Loading a GGUF is seconds of work, and until it finishes the child cannot
/// answer anything: without this line the parent had no way to know a spawned
/// sidecar was still cold, so every take inside the load window burned its
/// whole deadline waiting for a process that could not reply. The parent reads
/// this off the SAME stdout stream as the responses — a diagnostic on stderr
/// could never serve as a handshake, because stderr is a log, not the protocol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolishReady {
    /// Always [`READY_KIND`]. Present so a readiness line and a response line
    /// can never deserialize as each other.
    #[serde(rename = "type")]
    pub kind: String,
    /// The sidecar binary's version — a stale binary next to a new app is a
    /// real failure mode once the bundle ships an updater.
    pub version: String,
    /// Whether the model is actually resident. `false` means the process is up
    /// but cannot rewrite anything, which is a failure, not readiness.
    pub model_loaded: bool,
}

impl PolishReady {
    pub fn new(version: &str, model_loaded: bool) -> Self {
        Self {
            kind: READY_KIND.to_string(),
            version: version.to_string(),
            model_loaded,
        }
    }
}

/// Parse one stdout line as the readiness announcement, or `None` if it is
/// anything else (a response, a stray log line, half a line from a child that
/// died). Never confuses the two directions: a `PolishResponse` has no `type`
/// field and this has no `id`, so each fails to deserialize as the other.
pub fn parse_ready(line: &str) -> Option<PolishReady> {
    let parsed: PolishReady = serde_json::from_str(line.trim()).ok()?;
    (parsed.kind == READY_KIND).then_some(parsed)
}

/// One response line. `ok` discriminates: `true` carries `text`, `false`
/// carries `err`. Any failure shape at all means the caller keeps its rules
/// output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolishResponse {
    pub id: u64,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub err: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ms: Option<u64>,
    /// [`KIND_COUNT_TOKENS`] answer: the length of the request text in the
    /// model's own vocabulary. Also mirrored into `text` so a caller that only
    /// has the text channel (the existing `exchange`) can read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u32>,
    /// Set when the sidecar had to DROP input to fit the context (finding #35 —
    /// truncate and warn, never a hard `too_long` refusal for a summary). The
    /// answer is real; it is just not over all of the text that was sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
}

impl PolishResponse {
    /// A successful rewrite.
    pub fn ok(id: u64, text: String, out_tokens: u32, ms: u64) -> Self {
        Self {
            id,
            ok: true,
            text: Some(text),
            err: None,
            out_tokens: Some(out_tokens),
            ms: Some(ms),
            tokens: None,
            truncated: None,
        }
    }

    /// A successful generation that had to drop input to fit the context.
    pub fn ok_truncated(id: u64, text: String, out_tokens: u32, ms: u64) -> Self {
        Self {
            truncated: Some(true),
            ..Self::ok(id, text, out_tokens, ms)
        }
    }

    /// A [`KIND_COUNT_TOKENS`] answer. The number is carried twice on purpose:
    /// typed in `tokens`, and as `text` so the parent's existing text-only
    /// response path can read it without a second code path.
    pub fn counted(id: u64, tokens: u32, ms: u64) -> Self {
        Self {
            id,
            ok: true,
            text: Some(tokens.to_string()),
            err: None,
            out_tokens: None,
            ms: Some(ms),
            tokens: Some(tokens),
            truncated: None,
        }
    }

    /// The token count this response carries, from either channel.
    pub fn token_count(&self) -> Option<u32> {
        if !self.ok {
            return None;
        }
        self.tokens
            .or_else(|| self.text.as_deref().and_then(|t| t.trim().parse().ok()))
    }

    /// Whether input was dropped to fit the context.
    pub fn was_truncated(&self) -> bool {
        self.truncated.unwrap_or(false)
    }

    /// A failure. `reason` is a short tag (`deadline`, `decode`, `bad_request`,
    /// …) — never text, so nothing dictated can reach a log through it.
    pub fn err(id: u64, reason: &str) -> Self {
        Self {
            id,
            ok: false,
            text: None,
            err: Some(reason.to_string()),
            out_tokens: None,
            ms: None,
            tokens: None,
            truncated: None,
        }
    }

    /// The rewritten text, if this response is a success carrying a non-empty
    /// body. Validation of the *content* is a separate gate (`validate_polish`).
    pub fn into_text(self) -> Option<String> {
        if !self.ok {
            return None;
        }
        self.text.filter(|t| !t.trim().is_empty())
    }
}

/// Parse one response line, accepting it only when it answers `expect_id`.
///
/// The sidecar is a long-lived process fed by many dictations. A response that
/// arrives after the parent gave up (deadline) is still sitting in the pipe
/// when the NEXT request's read starts; pasting it would put one dictation's
/// text into another's target. Unparseable lines (the child logged something,
/// or died mid-write) are dropped the same way.
pub fn parse_response_for(line: &str, expect_id: u64) -> Option<PolishResponse> {
    let parsed: PolishResponse = serde_json::from_str(line.trim()).ok()?;
    (parsed.id == expect_id).then_some(parsed)
}

/// §2.3 output budget: `ceil(in_tokens * 1.4) + 24`, with words as the
/// token proxy (English GGUF vocabularies run ≈ 1.3 tokens/word, so this is
/// deliberately generous). A rewrite is not a generation — anything past this
/// is a loop, and the cap is what bounds the latency tail.
pub fn max_out_for(text: &str) -> u32 {
    let words = text.split_whitespace().count() as f32;
    let in_tokens = (words * 1.3).ceil();
    (in_tokens * 1.4).ceil() as u32 + 24
}

/// The truncation marker appended to text that had to be cut to fit a budget.
/// Visible on purpose: a silently shortened input is a summary that quietly
/// omits the end of a meeting.
pub const TRUNCATION_MARKER: &str = "[truncated]";

/// Fit `text` into `budget` tokens as measured by `count`, dropping whole lines
/// from the end, then characters if even one line will not fit.
///
/// Returns the kept text and whether anything was dropped — finding #35's
/// "truncate and warn", replacing the sidecar's `too_long` refusal for summarize
/// requests. A refusal is the wrong answer for a summary: the caller has no
/// smaller question to ask, so a hard error turns one oversized chunk into no
/// summary at all.
///
/// `count` is the caller's real tokenizer (the sidecar's vocabulary), which is
/// why this is a callback and not a word-count heuristic. It is called O(log n)
/// times — a binary search over the line count, not a scan.
pub fn fit_to_budget<F>(text: &str, budget: usize, count: &mut F) -> (String, bool)
where
    F: FnMut(&str) -> usize,
{
    if budget == 0 {
        return (String::new(), !text.is_empty());
    }
    if count(text) <= budget {
        return (text.to_string(), false);
    }
    let lines: Vec<&str> = text.lines().collect();
    // Largest prefix of whole lines that fits. `lo` is known-good (0 lines),
    // `hi` known-bad, so the loop always terminates on the boundary.
    let (mut lo, mut hi) = (0usize, lines.len());
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if count(&lines[..mid].join("\n")) <= budget {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    if lo > 0 {
        return (
            format!("{}\n{TRUNCATION_MARKER}", lines[..lo].join("\n")),
            true,
        );
    }
    // Not even the first line fits: cut it by characters, on char boundaries.
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        let candidate: String = chars[..mid].iter().collect();
        if count(&candidate) <= budget {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    (chars[..lo].iter().collect::<String>(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polish_protocol_roundtrip_serializes_and_parses() {
        let req = PolishRequest::polish(
            7,
            "email",
            "default",
            96,
            1200,
            "hey jordan quick update the build is green".to_string(),
        );
        let line = serde_json::to_string(&req).expect("request serializes");
        assert!(
            !line.contains('\n'),
            "a request is exactly one line: {line}"
        );
        let back: PolishRequest = serde_json::from_str(&line).expect("request parses");
        assert_eq!(back, req);

        let resp = PolishResponse::ok(7, "Hey Jordan,\n\nQuick update…".to_string(), 61, 540);
        let line = serde_json::to_string(&resp).expect("response serializes");
        assert!(
            !line.contains('\n'),
            "the newline in the body is escaped, so a response is one line: {line}"
        );
        let back: PolishResponse = serde_json::from_str(&line).expect("response parses");
        assert_eq!(back, resp);
        assert_eq!(
            back.into_text().as_deref(),
            Some("Hey Jordan,\n\nQuick update…")
        );

        // The failure shape carries no text and yields none.
        let failed = PolishResponse::err(7, "deadline");
        let line = serde_json::to_string(&failed).expect("failure serializes");
        assert_eq!(line, r#"{"id":7,"ok":false,"err":"deadline"}"#);
        assert_eq!(failed.into_text(), None);

        // The documented wire lines parse as written in the spec. A line with no
        // `kind` — every request written before YV97 — is still a polish
        // request, which is what keeps the two ends compatible in both
        // directions during a partial upgrade.
        let spec_req = r#"{"id":7,"mode":"email","style":"default","max_out":96,"deadline_ms":1200,"text":"hey jordan","topic":null}"#;
        let parsed = serde_json::from_str::<PolishRequest>(spec_req).expect("spec request parses");
        assert_eq!(parsed.id, 7);
        assert_eq!(parsed.kind, KIND_POLISH);
        assert_eq!(parsed.grammar, None);
    }

    /// YV97 — the summarize/count_tokens kinds ride the SAME line protocol, and
    /// a grammar-carrying request is the only thing that changes the sampler.
    #[test]
    fn polish_protocol_summarize_and_count_kinds_roundtrip() {
        let map = PolishRequest::summarize(
            11,
            SUMMARIZE_MAP,
            SUMMARY_MAP_MAX_OUT,
            30_000,
            "seg_0001: we agreed to ship on friday".to_string(),
            Some("root ::= \"{}\"".to_string()),
        );
        let line = serde_json::to_string(&map).expect("summarize request serializes");
        assert!(!line.contains('\n'), "one request, one line: {line}");
        let back: PolishRequest = serde_json::from_str(&line).expect("summarize request parses");
        assert_eq!(back, map);
        assert_eq!(back.kind, KIND_SUMMARIZE);
        assert_eq!(back.mode, SUMMARIZE_MAP);
        assert_eq!(back.max_out, SUMMARY_MAP_MAX_OUT);

        let count = PolishRequest::count_tokens(12, 5_000, "how long is this".to_string());
        let line = serde_json::to_string(&count).expect("count request serializes");
        let back: PolishRequest = serde_json::from_str(&line).expect("count request parses");
        assert_eq!(back.kind, KIND_COUNT_TOKENS);
        assert_eq!(back.grammar_text(), None);

        // The answer carries the number twice — typed, and in the text channel
        // the parent's existing response path already reads.
        let answer = PolishResponse::counted(12, 417, 3);
        let line = serde_json::to_string(&answer).expect("count answer serializes");
        let back = parse_response_for(&line, 12).expect("count answer parses");
        assert_eq!(back.token_count(), Some(417));
        assert_eq!(back.clone().into_text().as_deref(), Some("417"));
        assert!(!back.was_truncated());
        // A failure carries no count, whatever else is on the line.
        assert_eq!(PolishResponse::err(12, "deadline").token_count(), None);

        let cut = PolishResponse::ok_truncated(13, "summary".to_string(), 40, 900);
        assert!(cut.was_truncated());
        assert!(serde_json::to_string(&cut)
            .expect("truncated answer serializes")
            .contains(r#""truncated":true"#));
    }

    /// Finding #17 — the sampler is chosen PER REQUEST, and the grammar sits
    /// ahead of greedy in the chain. Greedy first would pick a token the grammar
    /// forbids and the constraint would be decorative.
    #[test]
    fn polish_protocol_sampler_plan_is_per_request() {
        let plain = PolishRequest::polish(1, "notes", "default", 64, 1200, "ship it".to_string());
        assert_eq!(sampler_plan(&plain), SamplerPlan::Greedy);

        let grammar = "root ::= \"{\" \"}\"".to_string();
        let constrained = PolishRequest::summarize(
            2,
            SUMMARIZE_MAP,
            SUMMARY_MAP_MAX_OUT,
            30_000,
            "seg_0001: ship it".to_string(),
            Some(grammar.clone()),
        );
        assert_eq!(
            sampler_plan(&constrained),
            SamplerPlan::GrammarThenGreedy {
                grammar,
                root: GRAMMAR_ROOT.to_string(),
            }
        );

        // A blank grammar is not a grammar — it must not produce a chain that
        // would fail to build at request time.
        let blank = PolishRequest::summarize(
            3,
            SUMMARIZE_REDUCE,
            SUMMARY_REDUCE_MAX_OUT,
            30_000,
            "narrate".to_string(),
            Some("   ".to_string()),
        );
        assert_eq!(sampler_plan(&blank), SamplerPlan::Greedy);
    }

    /// Finding #35 — overflow truncates and warns; it never refuses.
    #[test]
    fn polish_protocol_fit_to_budget_truncates_and_warns() {
        // A stand-in vocabulary: one token per whitespace word plus one per
        // capital letter, i.e. proper nouns cost more than the 1.3/word proxy.
        let mut count =
            |t: &str| t.split_whitespace().count() + t.chars().filter(|c| c.is_uppercase()).count();

        let text = "one two three\nfour five six\nseven eight nine";
        // Fits: unchanged, and not marked.
        let (kept, cut) = fit_to_budget(text, 99, &mut count);
        assert_eq!(kept, text);
        assert!(!cut);

        // Does not fit: whole lines are dropped from the END, the marker is
        // added, and what is kept is inside the budget.
        let (kept, cut) = fit_to_budget(text, 7, &mut count);
        assert!(cut, "over budget must report truncation");
        assert!(kept.starts_with("one two three"));
        assert!(kept.ends_with(TRUNCATION_MARKER), "warn marker: {kept:?}");
        assert!(!kept.contains("seven eight nine"));
        assert!(
            count(kept.trim_end_matches(TRUNCATION_MARKER)) <= 7,
            "the kept text is inside the budget: {kept:?}"
        );

        // Not even one line fits: cut by characters rather than refuse. No
        // marker here — the budget is a hard context limit and the marker's own
        // tokens would break it.
        let (kept, cut) = fit_to_budget("alpha beta gamma delta", 2, &mut count);
        assert!(cut);
        assert!(
            count(&kept) <= 2,
            "char-level cut stays in budget: {kept:?}"
        );
        assert!(!kept.is_empty());

        // A zero budget yields nothing, and says so, rather than panicking.
        let (kept, cut) = fit_to_budget("anything", 0, &mut count);
        assert!(kept.is_empty());
        assert!(cut);
    }

    #[test]
    fn polish_protocol_rejects_unknown_id() {
        // A late answer to dictation #6, read while waiting on #7: discarded.
        let stale = r#"{"id":6,"ok":true,"text":"an older dictation","out_tokens":3,"ms":900}"#;
        assert_eq!(parse_response_for(stale, 7), None);
        // The matching id is accepted, and only it.
        let current = r#"{"id":7,"ok":true,"text":"this dictation","out_tokens":2,"ms":410}"#;
        assert_eq!(
            parse_response_for(current, 7).and_then(PolishResponse::into_text),
            Some("this dictation".to_string())
        );
        // Garbage on stdout (a stray log line, or a half-written line from a
        // child that died) is dropped, never parsed as text.
        assert_eq!(parse_response_for("loading model…", 7), None);
        assert_eq!(parse_response_for(r#"{"id":7,"ok":"#, 7), None);
        // An error response for the right id parses, but yields no text.
        assert_eq!(
            parse_response_for(r#"{"id":7,"ok":false,"err":"deadline"}"#, 7)
                .and_then(PolishResponse::into_text),
            None
        );
    }

    #[test]
    fn polish_protocol_ready_line_is_distinct_from_a_response() {
        let ready = PolishReady::new("0.6.0", true);
        let line = serde_json::to_string(&ready).expect("ready serializes");
        assert_eq!(
            line,
            r#"{"type":"ready","version":"0.6.0","model_loaded":true}"#
        );
        assert_eq!(parse_ready(&line), Some(ready));
        // The two directions never parse as each other, which is what lets one
        // reader thread carry both on the same stdout.
        let response = r#"{"id":7,"ok":true,"text":"hi","out_tokens":1,"ms":9}"#;
        assert_eq!(parse_ready(response), None);
        assert_eq!(parse_response_for(&line, 7), None);
        // A stray log line, a half-written line, and a foreign `type` are not
        // readiness either.
        assert_eq!(parse_ready("loading model…"), None);
        assert_eq!(parse_ready(r#"{"type":"ready""#), None);
        assert_eq!(
            parse_ready(r#"{"type":"bye","version":"0.6.0","model_loaded":true}"#),
            None
        );
    }

    #[test]
    fn polish_protocol_max_out_is_bounded_by_input() {
        // Empty input still leaves room for a short answer; a 40-word dictation
        // stays inside the §2.3 decode budget.
        assert_eq!(max_out_for(""), 24);
        let forty_words = "word ".repeat(40);
        let cap = max_out_for(&forty_words);
        assert!((90..=120).contains(&cap), "40 words → {cap} tokens");
        // Monotone in input length — a longer dictation never gets a smaller cap.
        assert!(max_out_for(&"word ".repeat(80)) > cap);
    }
}
