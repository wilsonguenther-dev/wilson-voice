//! yap-1106-byom-cloud-fast-path — the "Cloud model (your key)" client: a
//! native Messages-API client (SSE streaming) beside the OpenAI-compatible
//! one in `model_providers.rs`.
//!
//! Facts checked against the API reference on 2026-11-06 (not from memory):
//! `POST {base}/v1/messages`, header `anthropic-version: 2026-10-21`, key in
//! `x-api-key`, `"stream": true`; events `message_start` (carries
//! `message.usage.input_tokens`), `content_block_delta` with
//! `delta.type == "text_delta"`, `message_delta` (carries
//! `usage.output_tokens`), `message_stop`; `error` events carry
//! `error.type` (`overloaded_error` is also HTTP 529). Prices live in
//! `cloud_route::price_for` with their date stamp.
//!
//! The key is held for the call and never formatted or logged. Transcript
//! text is never logged either — only token counts and timings.
//! The HTTP work is `http_client::post_stream` (the one reqwest seam).

use std::time::Duration;

use crate::cloud_route::{Budget, CloudError, CloudReply, Job, CLEANUP_MODEL, NOTES_MODEL_BEST};
use crate::http_client::{self, HttpClient};
use crate::summary_jobs::CancelToken;

/// The default endpoint of the hosted model. A user can point a provider at a
/// compatible gateway by editing the address in Settings (Diagnostics shows
/// the host; the UI never names the vendor).
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
/// `anthropic-version` — checked 2026-11-06.
pub const API_VERSION: &str = "2026-10-21";
/// The provider id Wilson's install gets when his key is entered.
pub const DEFAULT_PROVIDER_ID: &str = "cloud-key";

/// Which hosted model a job uses (`best` only for notes, by the user's pick).
pub fn model_for(job: Job, notes_best: bool) -> &'static str {
    match job {
        Job::Polish | Job::Format => CLEANUP_MODEL,
        Job::Notes => {
            if notes_best {
                NOTES_MODEL_BEST
            } else {
                CLEANUP_MODEL
            }
        }
    }
}

/// The system turn for dictation cleanup: the whole answer is the rewritten
/// text, nothing else, so the local validator can judge it like a sidecar
/// answer.
pub const CLEANUP_SYSTEM: &str = "You clean up dictated text. Fix punctuation, casing, filler words and obvious \
self-corrections (keep the corrected word, drop the one the speaker replaced). Keep every fact, name, number \
and the speaker's wording and language. Output ONLY the cleaned text — no preamble, no quotes, no notes.";

/// The system turn for meeting notes.
pub const NOTES_SYSTEM: &str = "You write concise meeting notes from a transcript: a one-paragraph summary, then \
'Decisions', 'Action items' (owner — item) and 'Open questions' as short bullet lists. Use only what the transcript \
says; never invent names or facts. Output markdown only.";

// ── SSE parsing (pure) ───────────────────────────────────────────────────────

/// One thing the stream said that we act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseEvent {
    InputTokens(u64),
    Text(String),
    OutputTokens(u64),
    Stop,
    /// `error` event: the API's `error.type`.
    Error(String),
}

/// Feeds bytes, yields parsed events. Tolerates chunk boundaries anywhere.
#[derive(Debug, Default)]
pub struct SseParser {
    buf: String,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push bytes; returns the events completed by them.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<SseEvent> {
        self.buf.push_str(&String::from_utf8_lossy(bytes));
        let mut out = Vec::new();
        while let Some(end) = self.buf.find("\n\n") {
            let block = self.buf[..end].to_string();
            self.buf.drain(..end + 2);
            for line in block.lines() {
                let line = line.trim_end_matches('\r');
                if let Some(data) = line.strip_prefix("data:") {
                    if let Some(ev) = parse_data(data.trim()) {
                        out.push(ev);
                    }
                }
            }
        }
        out
    }
}

/// One `data:` payload → an event we act on (`None` for the ones we don't).
pub fn parse_data(data: &str) -> Option<SseEvent> {
    let v: serde_json::Value = serde_json::from_str(data).ok()?;
    match v["type"].as_str()? {
        "message_start" => v["message"]["usage"]["input_tokens"]
            .as_u64()
            .map(SseEvent::InputTokens),
        "content_block_delta" => {
            if v["delta"]["type"].as_str() == Some("text_delta") {
                v["delta"]["text"]
                    .as_str()
                    .map(|t| SseEvent::Text(t.to_string()))
            } else {
                None
            }
        }
        "message_delta" => v["usage"]["output_tokens"]
            .as_u64()
            .map(SseEvent::OutputTokens),
        "message_stop" => Some(SseEvent::Stop),
        "error" => Some(SseEvent::Error(
            v["error"]["type"]
                .as_str()
                .unwrap_or("api_error")
                .to_string(),
        )),
        _ => None,
    }
}

// ── The client ───────────────────────────────────────────────────────────────

/// A hosted-model client bound to one provider config and its key.
pub struct CloudKeyClient {
    base_url: String,
    key: String,
    client: HttpClient,
}

impl CloudKeyClient {
    pub fn new(base_url: &str, key: String, client: HttpClient) -> Self {
        let base = base_url.trim().trim_end_matches('/');
        Self {
            base_url: if base.is_empty() {
                DEFAULT_BASE_URL.to_string()
            } else {
                base.to_string()
            },
            key,
            client,
        }
    }

    pub fn endpoint(&self) -> String {
        format!("{}/v1/messages", self.base_url)
    }

    /// The request body for `job` (checked against the reference: thinking is
    /// turned down for the latency-bound cleanup jobs, left adaptive for
    /// notes).
    pub fn body(model: &str, job: Job, system: &str, user: &str, max_tokens: u32) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": model,
            "max_tokens": max_tokens,
            "stream": true,
            "system": system,
            "messages": [{"role": "user", "content": user}],
        });
        if matches!(job, Job::Polish | Job::Format) {
            body["output_config"] = serde_json::json!({"effort": "low"});
            body["thinking"] = serde_json::json!({"type": "between_tools"});
        }
        body
    }

    /// Stream one completion inside `budget`. `on_delta` sees every text
    /// delta (notes render progressively); the returned reply is the complete
    /// text — a dictation caller pastes only that, never a partial.
    #[allow(clippy::too_many_arguments)]
    pub fn complete_streaming(
        &self,
        model: &str,
        job: Job,
        system: &str,
        user: &str,
        max_tokens: u32,
        budget: Budget,
        cancel: &CancelToken,
        on_delta: &mut (dyn FnMut(&str) + Send),
    ) -> Result<CloudReply, CloudError> {
        if cancel.is_cancelled() {
            return Err(CloudError::Cancelled);
        }
        let body = Self::body(model, job, system, user, max_tokens);
        let headers: [(&str, &str); 4] = [
            ("x-api-key", self.key.as_str()),
            ("anthropic-version", API_VERSION),
            ("accept", "text/event-stream"),
            ("content-type", "application/json"),
        ];
        let mut parser = SseParser::new();
        let mut text = String::new();
        let mut tokens_in = 0u64;
        let mut tokens_out = 0u64;
        let mut api_error: Option<String> = None;
        let mut stopped = false;
        let cancelled = || cancel.is_cancelled();
        let stats = {
            let mut on_chunk = |bytes: &[u8]| {
                for ev in parser.push(bytes) {
                    match ev {
                        SseEvent::InputTokens(n) => tokens_in = n,
                        SseEvent::Text(t) => {
                            on_delta(&t);
                            text.push_str(&t);
                        }
                        SseEvent::OutputTokens(n) => tokens_out = n,
                        SseEvent::Stop => stopped = true,
                        SseEvent::Error(kind) => api_error = Some(kind),
                    }
                }
            };
            http_client::post_stream_blocking(
                &self.client,
                &self.endpoint(),
                &headers,
                &body,
                budget.ttft,
                budget.total,
                &cancelled,
                &mut on_chunk,
            )?
        };
        if let Some(kind) = api_error {
            log::warn!("cloud_client stream_error kind={kind}");
            return Err(if kind == "overloaded_error" {
                CloudError::Overloaded { retry_after: None }
            } else if kind == "authentication_error" || kind == "permission_error" {
                CloudError::Status {
                    code: 401,
                    retry_after: None,
                }
            } else {
                CloudError::Protocol
            });
        }
        if !stopped && text.is_empty() {
            log::warn!("cloud_client stream_incomplete tokens_out={tokens_out}");
            return Err(CloudError::Protocol);
        }
        log::info!(
            "cloud_client complete job={} tokens_in={tokens_in} tokens_out={tokens_out} ttft_ms={} total_ms={}",
            job.as_str(),
            stats.ttft_ms,
            stats.total_ms
        );
        Ok(CloudReply {
            text,
            tokens_in,
            tokens_out,
            ttft_ms: stats.ttft_ms,
            total_ms: stats.total_ms,
        })
    }
}

/// The whole-request client timeout a cloud provider's `HttpClient` is built
/// with: the notes ceiling plus a margin (per-job budgets are tighter).
pub const CLIENT_TIMEOUT: Duration = Duration::from_secs(120);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_parser_survives_chunk_boundaries_and_reads_usage() {
        let stream = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"usage\":{\"input_tokens\":42,\"output_tokens\":1}}}\n\n",
            "event: content_block_start\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hel\"}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":12}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        let bytes = stream.as_bytes();
        let mut p = SseParser::new();
        let mut events = Vec::new();
        // Feed in awkward 7-byte pieces.
        for piece in bytes.chunks(7) {
            events.extend(p.push(piece));
        }
        assert_eq!(
            events,
            vec![
                SseEvent::InputTokens(42),
                SseEvent::Text("Hel".into()),
                SseEvent::Text("lo".into()),
                SseEvent::OutputTokens(12),
                SseEvent::Stop,
            ]
        );
        assert_eq!(
            parse_data("{\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\"}}"),
            Some(SseEvent::Error("overloaded_error".into()))
        );
        assert_eq!(parse_data("not json"), None);
    }

    #[test]
    fn body_names_the_model_and_streams() {
        let b = CloudKeyClient::body(CLEANUP_MODEL, Job::Polish, "sys", "hi", 64);
        assert_eq!(b["stream"], true);
        assert_eq!(b["model"], CLEANUP_MODEL);
        assert_eq!(b["thinking"]["type"], "between_tools");
        let n = CloudKeyClient::body(NOTES_MODEL_BEST, Job::Notes, "sys", "hi", 64);
        assert!(n.get("thinking").is_none());
        assert_eq!(model_for(Job::Notes, true), NOTES_MODEL_BEST);
        assert_eq!(model_for(Job::Notes, false), CLEANUP_MODEL);
        let c = CloudKeyClient::new("", "k".into(), http_client::build_client(CLIENT_TIMEOUT).unwrap());
        assert_eq!(c.endpoint(), format!("{DEFAULT_BASE_URL}/v1/messages"));
    }
}
