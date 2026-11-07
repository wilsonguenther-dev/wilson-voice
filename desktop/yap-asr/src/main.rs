//! `yap-asr` — the one process that holds Yap's speech model
//! (yap-1106-engine-owner).
//!
//! The app's engine owner (`src-tauri/src/engine_owner.rs`) is the only thing
//! that spawns this binary. It loads one model on request, decodes one take at
//! a time, and can be told to stop a decode early. If a decode hangs anyway,
//! the owner kills this process by its PID and macOS frees every byte of GPU
//! memory it held — something an engine inside the app could never do.
//!
//! Protocol: `asr_protocol.rs`, compiled into both binaries from one file.
//! stdout carries JSON and nothing else; diagnostics go to stderr.
//!
//! Threads: the main thread reads stdin. Decodes and loads run on one worker
//! thread that owns the engine, so a `cancel` line can be acted on while a
//! decode is in flight. When stdin closes (the app died or let go), the worker
//! frees the model and the process ends with `_exit(0)`.

// The app's end uses the constructors this end never calls (and vice versa),
// so a bin crate sees them as dead. One file, two ends: the allow is the
// price of the two ends never drifting.
#[allow(dead_code)]
#[path = "../../src-tauri/src/asr_protocol.rs"]
mod asr_protocol;

use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, Once};
use std::time::Instant;

use transcribe_cpp::{
    Backend, CancelToken, Model, ModelOptions, RunExtension, RunOptions, Session, TimestampKind,
    WhisperRunOptions,
};

use asr_protocol::{
    samples_from_le_bytes, AsrReady, AsrRequest, AsrResponse, WireCaps, WireSpan, WireTimed,
    KIND_CANCEL, KIND_LOAD, KIND_SHUTDOWN, KIND_TRANSCRIBE,
};

static BACKEND_INIT: Once = Once::new();

struct Engine {
    session: Session,
    arch: String,
    /// A clone of the token installed on `session`; reset before each decode.
    cancel: CancelToken,
}

/// What the main thread hands the worker.
enum Work {
    Request(AsrRequest),
    /// stdin closed: free the model and end.
    Eof,
}

fn main() {
    if let Some(unexpected) = std::env::args().nth(1) {
        eprintln!("yap-asr: unknown argument '{unexpected}'");
        eprintln!("usage: yap-asr   (the model is loaded by request, not by argv)");
        std::process::exit(2);
    }
    let code = match run() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("yap-asr: {e}");
            1
        }
    };
    hard_exit(code);
}

/// End without running C++ static destructors. Called only after the engine
/// has been dropped on the worker thread, so nothing is leaked that the OS
/// does not reclaim with the process.
fn hard_exit(code: i32) -> ! {
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    // SAFETY: `_exit` is async-signal-safe and takes a plain int.
    unsafe { libc::_exit(code) }
}

fn write_line(out: &Mutex<std::io::Stdout>, line: &str) -> Result<(), String> {
    let guard = out.lock().map_err(|_| "stdout lock poisoned".to_string())?;
    let mut lock = guard.lock();
    writeln!(lock, "{line}").map_err(|e| format!("stdout: {e}"))?;
    lock.flush().map_err(|e| format!("stdout flush: {e}"))
}

fn respond(out: &Mutex<std::io::Stdout>, response: &AsrResponse) -> Result<(), String> {
    let encoded = serde_json::to_string(response).map_err(|e| format!("encode: {e}"))?;
    write_line(out, &encoded)
}

fn run() -> Result<(), String> {
    let out = Arc::new(Mutex::new(std::io::stdout()));
    let ready = serde_json::to_string(&AsrReady::now(std::process::id()))
        .map_err(|e| format!("encode ready: {e}"))?;
    write_line(&out, &ready)?;
    eprintln!("yap-asr ready pid={}", std::process::id());

    // The cancel token of the CURRENT engine, reachable from the main thread.
    let cancel: Arc<Mutex<Option<CancelToken>>> = Arc::new(Mutex::new(None));
    let (tx, rx) = mpsc::channel::<Work>();

    let worker = {
        let out = out.clone();
        let cancel = cancel.clone();
        std::thread::Builder::new()
            .name("yap-asr-engine".into())
            .spawn(move || worker_loop(rx, out, cancel))
            .map_err(|e| format!("spawn worker: {e}"))?
    };

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let req = match serde_json::from_str::<AsrRequest>(&line) {
            Ok(req) => req,
            Err(_) => continue,
        };
        if req.kind == KIND_CANCEL {
            if let Ok(slot) = cancel.lock() {
                if let Some(token) = slot.as_ref() {
                    token.cancel();
                    eprintln!("yap-asr cancel requested");
                }
            }
            continue;
        }
        let is_shutdown = req.kind == KIND_SHUTDOWN;
        if tx.send(Work::Request(req)).is_err() {
            break;
        }
        if is_shutdown {
            break;
        }
    }
    let _ = tx.send(Work::Eof);
    let _ = worker.join();
    Ok(())
}

fn worker_loop(
    rx: mpsc::Receiver<Work>,
    out: Arc<Mutex<std::io::Stdout>>,
    cancel: Arc<Mutex<Option<CancelToken>>>,
) {
    let mut engine: Option<Engine> = None;
    while let Ok(work) = rx.recv() {
        let req = match work {
            Work::Request(req) => req,
            Work::Eof => break,
        };
        let response = match req.kind.as_str() {
            KIND_LOAD => handle_load(&req, &mut engine, &cancel),
            KIND_TRANSCRIBE => handle_transcribe(&req, engine.as_mut()),
            KIND_SHUTDOWN => {
                let freed = u32::from(engine.is_some());
                if let Ok(mut slot) = cancel.lock() {
                    *slot = None;
                }
                drop(engine.take());
                eprintln!("yap-asr shutdown freed={freed}");
                let mut r = AsrResponse::ok(req.id);
                r.freed = Some(freed);
                let _ = respond(&out, &r);
                return;
            }
            _ => AsrResponse::fail(req.id, "unsupported request kind"),
        };
        if respond(&out, &response).is_err() {
            break;
        }
    }
    if let Ok(mut slot) = cancel.lock() {
        *slot = None;
    }
    drop(engine.take());
}

fn handle_load(
    req: &AsrRequest,
    engine: &mut Option<Engine>,
    cancel: &Mutex<Option<CancelToken>>,
) -> AsrResponse {
    let Some(path) = req.model_path.as_deref() else {
        return AsrResponse::fail(req.id, "load needs a model path");
    };
    if engine.is_some() {
        // One model per helper, ever. A second load is a bug on the app side;
        // refusing keeps the "at most one copy" promise inside this process.
        return AsrResponse::fail(req.id, "a model is already loaded in this helper");
    }
    let started = Instant::now();
    BACKEND_INIT.call_once(|| {
        transcribe_cpp::init_logging();
        if let Err(e) = transcribe_cpp::init_backends_default() {
            eprintln!("yap-asr backend init failed: {e}");
        }
    });
    let options = ModelOptions {
        backend: Backend::Auto,
        gpu_device: 0,
    };
    let model = match Model::load_with(Path::new(path), &options) {
        Ok(m) => m,
        Err(e) => return AsrResponse::fail(req.id, format!("failed to load model: {e}")),
    };
    let caps = {
        let c = model.capabilities();
        WireCaps {
            arch: model.arch(),
            backend: model.backend(),
            native_sample_rate: c.native_sample_rate,
            languages: c.languages.clone(),
            max_timestamp_kind: kind_str(c.max_timestamp_kind).to_string(),
            max_audio_ms: c.max_audio_ms,
            supports_language_detect: c.supports_language_detect,
            supports_streaming: c.supports_streaming,
        }
    };
    let arch = model.arch();
    let mut session = match model.session() {
        Ok(s) => s,
        Err(e) => return AsrResponse::fail(req.id, format!("failed to create session: {e}")),
    };
    let token = CancelToken::new();
    session.set_cancel_token(&token);
    if let Ok(mut slot) = cancel.lock() {
        *slot = Some(token.clone());
    }
    eprintln!(
        "yap-asr loaded arch={} backend={} ms={}",
        caps.arch,
        caps.backend,
        started.elapsed().as_millis()
    );
    *engine = Some(Engine {
        session,
        arch,
        cancel: token,
    });
    let mut r = AsrResponse::ok(req.id);
    r.caps = Some(caps);
    r
}

fn kind_str(kind: TimestampKind) -> &'static str {
    match kind {
        TimestampKind::Segment => "segment",
        TimestampKind::Word => "word",
        TimestampKind::Token => "token",
        TimestampKind::None | TimestampKind::Auto => "none",
    }
}

fn span(t0_ms: i64, t1_ms: i64, text: &str) -> WireSpan {
    WireSpan {
        start_seconds: t0_ms as f64 / 1000.0,
        end_seconds: t1_ms as f64 / 1000.0,
        text: text.trim().to_string(),
    }
}

fn handle_transcribe(req: &AsrRequest, engine: Option<&mut Engine>) -> AsrResponse {
    let Some(engine) = engine else {
        return AsrResponse::fail(req.id, "no model is loaded");
    };
    let Some(path) = req.samples_path.as_deref() else {
        return AsrResponse::fail(req.id, "transcribe needs a samples file");
    };
    let samples = match std::fs::read(path)
        .map_err(|e| format!("read samples: {e}"))
        .and_then(|b| samples_from_le_bytes(&b))
    {
        Ok(s) => s,
        Err(e) => return AsrResponse::fail(req.id, e),
    };
    let family = req
        .bias_prompt
        .as_deref()
        .filter(|p| !p.is_empty() && engine.arch == "whisper")
        .map(|p| {
            RunExtension::Whisper(WhisperRunOptions {
                initial_prompt: Some(p.to_string()),
                ..WhisperRunOptions::default()
            })
        });
    let mut options = RunOptions {
        language: req.language.clone(),
        family,
        ..RunOptions::default()
    };
    if req.timed {
        options.timestamps = TimestampKind::Auto;
    }
    // A cancel from a PREVIOUS decode must not abort this one: the token is
    // sticky (see the app's `asr_engine::reset_cancel`).
    engine.cancel.reset();
    let started = Instant::now();
    let result = engine.session.run(samples.as_slice(), &options);
    eprintln!(
        "yap-asr decode samples={} ms={} ok={}",
        samples.len(),
        started.elapsed().as_millis(),
        result.is_ok()
    );
    match result {
        Ok(t) => {
            let timed = WireTimed {
                text: if req.timed {
                    t.text.trim().to_string()
                } else {
                    t.text.clone()
                },
                kind: if req.timed {
                    kind_str(t.timestamp_kind).to_string()
                } else {
                    "none".to_string()
                },
                segments: if req.timed {
                    t.segments
                        .iter()
                        .map(|s| span(s.t0_ms, s.t1_ms, &s.text))
                        .collect()
                } else {
                    Vec::new()
                },
                words: if req.timed {
                    t.words
                        .iter()
                        .map(|w| span(w.t0_ms, w.t1_ms, &w.text))
                        .collect()
                } else {
                    Vec::new()
                },
            };
            let mut r = AsrResponse::ok(req.id);
            r.language = t.language.clone();
            r.timed = Some(timed);
            r
        }
        Err(e) => AsrResponse::fail(req.id, format!("transcription failed: {e}")),
    }
}

#[cfg(test)]
mod tests {
    /// stdout is the protocol stream: no `println!`/`print!` may appear in
    /// this binary's source (diagnostics go to stderr).
    #[test]
    fn stdout_carries_json_only() {
        let src = include_str!("main.rs");
        let needle = ["print", "ln!("].concat();
        let bare = ["\n    ", "print!("].concat();
        assert!(!src.contains(&format!(" {needle}")), "println! in yap-asr");
        assert!(!src.contains(&bare), "print! in yap-asr");
    }
}
