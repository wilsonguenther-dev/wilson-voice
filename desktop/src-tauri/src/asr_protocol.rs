//! Wire protocol for the `yap-asr` speech helper (yap-1106-engine-owner).
//!
//! Why the speech model lives in its own process: when a decode hung, the app
//! used to abandon the stuck engine (still running on the GPU) and load
//! another copy, so GPU memory filled until window drawing froze, and quitting
//! with a model still resident aborted inside ggml-metal's static destructors.
//! A helper process fixes both at the root: the owner (`engine_owner.rs`) is
//! the only code that spawns it, a hung helper is killed BY ITS PID (macOS
//! frees its GPU memory with the process), and the app process itself never
//! reaches `exit()` holding Metal/ggml state.
//!
//! Same shape as `diarize_protocol.rs`: newline-delimited JSON on the child's
//! stdin/stdout, one request per line, one response per line, compiled into
//! both binaries from this one file (the helper includes it with `#[path]`) so
//! the two ends cannot drift. stdout carries JSON and nothing else.
//!
//! ```jsonc
//! // <- stdout, once, as soon as the child is up (no model loaded yet)
//! {"type":"ready","version":"0.1.0","pid":4242}
//! // -> stdin
//! {"id":1,"kind":"load","model_path":"/…/model.gguf"}
//! // <- stdout
//! {"id":1,"ok":true,"caps":{"arch":"whisper","backend":"metal",…}}
//! // -> stdin: samples are raw little-endian f32, 16 kHz mono, in a 0600 temp
//! //    file the APP writes and deletes; audio never rides the pipe as text.
//! {"id":2,"kind":"transcribe","samples_path":"/…/yap-asr-1-2.f32","language":null,"bias_prompt":null,"timed":true}
//! // <- stdout
//! {"id":2,"ok":true,"timed":{"text":"…","kind":"segment","segments":[…],"words":[]},"language":"en"}
//! // -> stdin: ends the in-flight decode early. Answered by the decode's own
//! //    response (an error), never by a line of its own.
//! {"id":3,"kind":"cancel"}
//! // -> stdin: free the model, answer, and end the process.
//! {"id":4,"kind":"shutdown"}
//! // <- stdout
//! {"id":4,"ok":true,"freed":1}
//! ```
//!
//! This file must not name anything from either crate: serde only.

use serde::{Deserialize, Serialize};

/// Protocol version the helper announces on its ready line.
pub const ASR_PROTOCOL_VERSION: &str = "0.1.0";

pub const KIND_LOAD: &str = "load";
pub const KIND_TRANSCRIBE: &str = "transcribe";
pub const KIND_CANCEL: &str = "cancel";
pub const KIND_SHUTDOWN: &str = "shutdown";

/// The first stdout line of a healthy helper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AsrReady {
    #[serde(rename = "type")]
    pub kind: String,
    pub version: String,
    pub pid: u32,
}

impl AsrReady {
    pub fn now(pid: u32) -> AsrReady {
        AsrReady {
            kind: "ready".to_string(),
            version: ASR_PROTOCOL_VERSION.to_string(),
            pid,
        }
    }

    pub fn is_ready(&self) -> bool {
        self.kind == "ready"
    }
}

/// One request line. Fields a kind does not use stay `None`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AsrRequest {
    pub id: u64,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub samples_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bias_prompt: Option<String>,
    #[serde(default)]
    pub timed: bool,
}

impl AsrRequest {
    pub fn load(id: u64, model_path: &str) -> AsrRequest {
        AsrRequest {
            id,
            kind: KIND_LOAD.to_string(),
            model_path: Some(model_path.to_string()),
            ..AsrRequest::default()
        }
    }

    pub fn transcribe(
        id: u64,
        samples_path: &str,
        language: Option<&str>,
        bias_prompt: Option<&str>,
        timed: bool,
    ) -> AsrRequest {
        AsrRequest {
            id,
            kind: KIND_TRANSCRIBE.to_string(),
            samples_path: Some(samples_path.to_string()),
            language: language.map(str::to_string),
            bias_prompt: bias_prompt.map(str::to_string),
            timed,
            ..AsrRequest::default()
        }
    }

    pub fn cancel(id: u64) -> AsrRequest {
        AsrRequest {
            id,
            kind: KIND_CANCEL.to_string(),
            ..AsrRequest::default()
        }
    }

    pub fn shutdown(id: u64) -> AsrRequest {
        AsrRequest {
            id,
            kind: KIND_SHUTDOWN.to_string(),
            ..AsrRequest::default()
        }
    }
}

/// What the loaded model can do, as measured by the helper. Mirrors the app's
/// `asr_engine::ModelCapabilities`; `max_timestamp_kind` is the lowercase
/// `TimedKind` string ("none" | "segment" | "word" | "token").
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct WireCaps {
    pub arch: String,
    pub backend: String,
    pub native_sample_rate: i32,
    pub languages: Vec<String>,
    pub max_timestamp_kind: String,
    pub max_audio_ms: i64,
    pub supports_language_detect: bool,
    pub supports_streaming: bool,
}

/// One timed row, seconds from the start of the decoded buffer.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct WireSpan {
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub text: String,
}

/// A decode result. `kind` is the lowercase timestamp kind; a plain
/// (untimed) decode answers `"none"` with both lists empty.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct WireTimed {
    pub text: String,
    pub kind: String,
    pub segments: Vec<WireSpan>,
    pub words: Vec<WireSpan>,
}

/// One response line. `ok == false` carries `error`, one plain sentence.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AsrResponse {
    pub id: u64,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caps: Option<WireCaps>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timed: Option<WireTimed>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freed: Option<u32>,
}

impl AsrResponse {
    pub fn fail(id: u64, error: impl Into<String>) -> AsrResponse {
        AsrResponse {
            id,
            ok: false,
            error: Some(error.into()),
            ..AsrResponse::default()
        }
    }

    pub fn ok(id: u64) -> AsrResponse {
        AsrResponse {
            id,
            ok: true,
            ..AsrResponse::default()
        }
    }
}

/// Encode samples as the raw little-endian f32 bytes the samples file holds.
pub fn samples_to_le_bytes(samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 4);
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Decode a samples file's bytes. A trailing partial sample is an error, not
/// silently dropped: it means the file was truncated.
pub fn samples_from_le_bytes(bytes: &[u8]) -> Result<Vec<f32>, String> {
    if !bytes.len().is_multiple_of(4) {
        return Err(format!(
            "samples file is {} bytes, not a whole number of samples",
            bytes.len()
        ));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

#[cfg(test)]
mod asr_protocol_tests {
    use super::*;

    #[test]
    fn requests_round_trip_and_omit_unused_fields() {
        let line = serde_json::to_string(&AsrRequest::cancel(3)).unwrap();
        assert_eq!(line, r#"{"id":3,"kind":"cancel","timed":false}"#);
        let req = AsrRequest::transcribe(2, "/tmp/x.f32", Some("en"), None, true);
        let back: AsrRequest = serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn ready_line_parses() {
        let r: AsrReady =
            serde_json::from_str(r#"{"type":"ready","version":"0.1.0","pid":7}"#).unwrap();
        assert!(r.is_ready());
        assert_eq!(r.pid, 7);
    }

    #[test]
    fn samples_round_trip_and_reject_truncation() {
        let s = vec![0.0f32, -1.0, 0.5, 1.25];
        let bytes = samples_to_le_bytes(&s);
        assert_eq!(samples_from_le_bytes(&bytes).unwrap(), s);
        assert!(samples_from_le_bytes(&bytes[..5]).is_err());
    }
}
