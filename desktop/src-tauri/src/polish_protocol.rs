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
//! // → stdin
//! {"id":7,"mode":"email","style":"default","max_out":96,"deadline_ms":1200,
//!  "text":"hey jordan quick update…","topic":null}
//! // ← stdout
//! {"id":7,"ok":true,"text":"Hey Jordan,\n\nQuick update…","out_tokens":61,"ms":540}
//! {"id":7,"ok":false,"err":"deadline"}
//! ```
//!
//! **This file is the single source of truth for both sides.** `yap-polish`
//! compiles it directly (`#[path = "../../src-tauri/src/polish_protocol.rs"]`)
//! rather than keeping a second copy, so the two ends cannot drift. That is why
//! it depends on nothing but `serde` + `std` — no Tauri, no crate-local types.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// One rewrite request. Serialized as a single line on the sidecar's stdin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolishRequest {
    /// Monotonic per-process request id. Responses that do not carry it are
    /// stale (a previous dictation the parent stopped waiting for) and MUST be
    /// discarded — see [`parse_response_for`].
    pub id: u64,
    /// Dictation mode, lowercased: `email` `document` `notes` `chat` `plain`
    /// `list`. `code` never reaches the model at all.
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
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polish_protocol_roundtrip_serializes_and_parses() {
        let req = PolishRequest {
            id: 7,
            mode: "email".to_string(),
            style: "default".to_string(),
            max_out: 96,
            deadline_ms: 1200,
            text: "hey jordan quick update the build is green".to_string(),
            topic: None,
        };
        let line = serde_json::to_string(&req).expect("request serializes");
        assert!(!line.contains('\n'), "a request is exactly one line: {line}");
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
        assert_eq!(back.into_text().as_deref(), Some("Hey Jordan,\n\nQuick update…"));

        // The failure shape carries no text and yields none.
        let failed = PolishResponse::err(7, "deadline");
        let line = serde_json::to_string(&failed).expect("failure serializes");
        assert_eq!(line, r#"{"id":7,"ok":false,"err":"deadline"}"#);
        assert_eq!(failed.into_text(), None);

        // The documented wire lines parse as written in the spec.
        let spec_req = r#"{"id":7,"mode":"email","style":"default","max_out":96,"deadline_ms":1200,"text":"hey jordan","topic":null}"#;
        assert_eq!(
            serde_json::from_str::<PolishRequest>(spec_req)
                .expect("spec request parses")
                .id,
            7
        );
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
