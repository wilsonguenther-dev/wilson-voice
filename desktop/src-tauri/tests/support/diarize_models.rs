//! Shared model-directory resolution for YV122's two model-gated smoke tests.
//!
//! Included with `#[path]` by `sherpa_load_smoke.rs` and `diarize_embed_smoke.rs`
//! rather than duplicated, for the same reason `diarize_protocol.rs` is shared
//! across the process boundary: two copies of "where are the models and is this
//! machine allowed to skip" drift, and the one that drifts is the one that
//! stops skipping in CI.
//!
//! ## Why this is a skip and not a hard failure
//!
//! Same posture as `meeting_eval.rs`'s corpus gate. These files are ~36 MB of
//! model weights that YV123 has not vendored yet, so a machine without them
//! runs the file green in milliseconds and says so on stderr. The moment
//! YV123's `download_diarize_model_with` lands, the catalog path replaces the
//! env var below and this gate stops being reachable on a configured machine.

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use wilson_voice_lib::diarize_protocol::{
    parse_ready, parse_response_for, DiarizeRequest, DiarizeResponse,
};

/// Override for the model directory.
pub const MODELS_ENV: &str = "YAP_DIARIZE_MODELS";
/// The durable location, relative to `$HOME`. Deliberately outside the repo and
/// outside any scratchpad — the standing durable-data rule.
pub const MODELS_HOME_RELATIVE: &str = "yap-diarize-models";
/// The one line a model-gated test prints when there is nothing to load.
/// Asserted verbatim by the acceptance criteria, so it is a constant.
pub const MODELS_ABSENT: &str = "diarization models not found at ~/yap-diarize-models, skipping";

/// pyannote-segmentation-3.0, as it lands out of the upstream `.tar.bz2`
/// (YV123 is what extracts it from the vendored archive).
pub const SEGMENTATION_RELATIVE: &str = "sherpa-onnx-pyannote-segmentation-3-0/model.onnx";
/// wespeaker_en_voxceleb_CAM++, the recommended embedding model.
pub const EMBEDDING_RELATIVE: &str = "wespeaker_en_voxceleb_CAM++.onnx";
/// The ResNet34 alternative from the same upstream release. **Optional**, and
/// present only to serve as the non-vacuity control in
/// `sherpa_load_smoke`: it is a different width, so a sidecar that reported a
/// constant would have to report the wrong one for it.
pub const EMBEDDING_ALT_RELATIVE: &str = "wespeaker_en_voxceleb_resnet34.onnx";

pub fn models_root() -> PathBuf {
    if let Ok(dir) = std::env::var(MODELS_ENV) {
        return PathBuf::from(dir);
    }
    dirs::home_dir()
        .expect("a home directory")
        .join(MODELS_HOME_RELATIVE)
}

/// The segmentation + embedding pair, or `None` after printing [`MODELS_ABSENT`].
pub fn models() -> Option<(PathBuf, PathBuf)> {
    let root = models_root();
    let segmentation = root.join(SEGMENTATION_RELATIVE);
    let embedding = root.join(EMBEDDING_RELATIVE);
    if segmentation.is_file() && embedding.is_file() {
        return Some((segmentation, embedding));
    }
    eprintln!("{MODELS_ABSENT}");
    None
}

/// The built `yap-diarize`, wherever this checkout has one.
///
/// Same resolution order as `diarize_sidecar_handshake.rs`, and non-skippable
/// for the same reason: `bundle.externalBin` makes a staged sidecar a
/// precondition of this crate compiling at all.
pub fn diarize_binary() -> PathBuf {
    if let Ok(named) = std::env::var("YAP_DIARIZE_BIN") {
        let path = PathBuf::from(named);
        assert!(path.is_file(), "YAP_DIARIZE_BIN is not a file: {path:?}");
        return path;
    }
    if let Some(sibling) = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.parent()?.join("yap-diarize")))
        .filter(|p| p.is_file())
    {
        return sibling;
    }
    let staged = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");
    std::fs::read_dir(&staged)
        .unwrap_or_else(|e| panic!("no staged sidecars at {staged:?}: {e}"))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("yap-diarize-"))
        })
        .unwrap_or_else(|| {
            panic!(
                "no yap-diarize binary: build one with `cargo build -p yap-diarize` \
                 or point YAP_DIARIZE_BIN at it"
            )
        })
}

// ── A raw sidecar, with every stdout line kept ──────────────────────────────
//
// `DiarizePool` is the production client and `diarize_sidecar_pool.rs` drives
// it; these tests spawn the child directly instead, because the claim they have
// to make is about the BYTES on stdout, and a client that hands back a parsed
// `DiarizeResponse` has already thrown away the evidence. sherpa-onnx's C++
// core logs (`speaker-embedding-extractor.cc:Validate:40 …`); one of those
// lines on this stream and the parent's de-multiplexer is reading a diagnostic
// as a response.

/// A spawned `yap-diarize` and both ends of its protocol stream, keeping every
/// line stdout has produced.
pub struct Spawned {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    lines: Vec<String>,
}

impl Spawned {
    /// Spawn and consume the readiness line.
    pub fn start() -> Self {
        let mut child = Command::new(diarize_binary())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // stderr is INHERITED, not piped: sherpa's diagnostics and the
            // sidecar's own belong in the test log, and a piped stderr nobody
            // drains is a pipe a chatty child can block on.
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn yap-diarize");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = BufReader::new(child.stdout.take().expect("stdout"));
        let mut spawned = Self {
            child,
            stdin,
            stdout,
            lines: Vec::new(),
        };
        let ready = spawned.read_line();
        assert!(
            parse_ready(&ready).is_some(),
            "the first stdout line must be the readiness announcement, got: {ready}"
        );
        spawned
    }

    fn read_line(&mut self) -> String {
        let mut line = String::new();
        let read = self
            .stdout
            .read_line(&mut line)
            .expect("read the sidecar's stdout");
        assert!(read > 0, "the sidecar closed stdout without answering");
        self.lines.push(line.trim_end().to_string());
        line
    }

    /// Send one request and read its answer.
    pub fn ask(&mut self, req: &DiarizeRequest) -> DiarizeResponse {
        let encoded = serde_json::to_string(req).expect("encode");
        writeln!(self.stdin, "{encoded}").expect("write the request");
        self.stdin.flush().expect("flush");
        let line = self.read_line();
        parse_response_for(&line, req.id)
            .unwrap_or_else(|| panic!("not a response for id {}: {line}", req.id))
    }

    /// Every line stdout has produced, readiness included.
    pub fn stdout_lines(&self) -> &[String] {
        &self.lines
    }

    /// Assert that every line this child has written to stdout is a protocol
    /// message — the standing guard on sherpa's logging.
    pub fn assert_stdout_is_only_protocol(&self) {
        for line in &self.lines {
            let is_protocol = parse_ready(line).is_some()
                || serde_json::from_str::<DiarizeResponse>(line).is_ok();
            assert!(
                is_protocol,
                "stdout carries the protocol and nothing else; this line is \
                 neither a readiness announcement nor a response: {line}"
            );
        }
    }
}

impl Drop for Spawned {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ── Synthetic voices ────────────────────────────────────────────────────────

/// Words per minute for [`synthesize`], the same rate `meeting_eval.rs` renders
/// its corpus at.
pub const WORDS_PER_MINUTE: u32 = 170;

/// Render `text` in the named `say` voice to a mono 16-bit WAV at `rate_hz`.
///
/// The same two macOS built-ins `meeting_eval.rs::synthesize_with` uses, and
/// for the same reason: nothing leaves the machine and no third-party asset is
/// involved.
///
/// **The `status().success()` check below does NOT verify the voice.**
/// `say` exits 0 on a voice this Mac does not have and renders with a fallback
/// instead — see [`assert_voices_are_distinct`], which is the check that does,
/// and which any test whose claim is about WHO is speaking has to run first.
pub fn synthesize(dir: &Path, voice: &str, tag: &str, text: &str, rate_hz: u32) -> PathBuf {
    std::fs::create_dir_all(dir).expect("scratch dir");
    let aiff = dir.join(format!("{voice}-{tag}.aiff"));
    let wav = dir.join(format!("{voice}-{tag}.wav"));
    let _ = std::fs::remove_file(&aiff);
    let _ = std::fs::remove_file(&wav);

    let say = Command::new("say")
        .args(["-v", voice, "-r", &WORDS_PER_MINUTE.to_string(), "-o"])
        .arg(&aiff)
        .arg(text)
        // A non-zero exit still means something real went wrong (no disk, bad
        // path). It just never means "no such voice".
        .status()
        .expect("`say` is a macOS built-in");
    assert!(say.success(), "say -v {voice} failed");

    let convert = Command::new("afconvert")
        .args(["-f", "WAVE", "-d", &format!("LEI16@{rate_hz}"), "-c", "1"])
        .arg(&aiff)
        .arg(&wav)
        .status()
        .expect("`afconvert` is a macOS built-in");
    assert!(convert.success(), "afconvert failed for {voice}");
    wav
}

/// Every voice `say -v '?'` lists on this machine, in its exact spelling.
///
/// The listing's columns are `name`, `locale`, then `# a sample sentence`, and
/// a name may itself contain spaces (`Eddy (English (US))`), so the locale is
/// taken as the last whitespace-separated token before the comment and
/// everything to its left is the name.
pub fn installed_voices() -> Vec<String> {
    let listed = Command::new("say")
        .args(["-v", "?"])
        .output()
        .expect("`say` is a macOS built-in");
    assert!(listed.status.success(), "say -v '?' failed");
    String::from_utf8_lossy(&listed.stdout)
        .lines()
        .filter_map(|line| {
            let columns = line.split('#').next()?.trim_end();
            let (name, _locale) = columns.rsplit_once(char::is_whitespace)?;
            let name = name.trim();
            (!name.is_empty()).then(|| name.to_string())
        })
        .collect()
}

/// The precondition every test whose measured quantity is WHO is speaking has
/// to establish before it renders a single utterance.
///
/// # Why the exit code is not this check
///
/// `say` does **not** fail on a voice this Mac does not have. Measured on the
/// machine this item was developed on:
///
/// ```text
/// $ say -v Bogus  -o /tmp/b1.aiff "hello there" ; echo $?   → 0, empty stderr, 34 KB of audio
/// $ say -v ZzzQqq -o /tmp/b2.aiff "hello there" ; echo $?   → 0, empty stderr, 34 KB of audio
/// $ md5 -q /tmp/b1.aiff /tmp/b2.aiff
/// 01bc7b9486b8dcaa66f203af103e3603
/// 01bc7b9486b8dcaa66f203af103e3603
/// ```
///
/// Two unrelated nonsense names render **byte-identically**: every unknown
/// voice collapses silently onto one fallback. `Fred` and `Ralph` — the pair
/// [`diarize_embed_smoke`] deliberately includes because CAM++ confuses them
/// worst — are legacy MacinTalk voices Apple has been moving to
/// download-on-demand, so "absent on this machine" is a live possibility and
/// not a hypothetical.
///
/// # Why it has to be a precondition and cannot be a property of the numbers
///
/// If two of a six-voice roster collapse into one, their 9 cross-voice pairs
/// stop being impostor pairs and become same-speaker pairs — the EER and the
/// per-voice table are then measured on a roster that does not exist. The
/// dangerous part is the direction: the sweep's overlap tripwire
/// (`impostor_max > genuine_min`) gets *easier* under exactly that collapse,
/// because a collapsed cross pair scores like a genuine one. The assertion
/// written to catch "the substrate changed" is an assertion a substrate change
/// **strengthens**. So the roster is checked before any number exists, rather
/// than inferred from the numbers afterwards.
///
/// # Two checks, because the first one trusts the listing
///
/// 1. every requested voice appears verbatim in `say -v '?'`;
/// 2. one short probe rendered per voice yields pairwise-**distinct** bytes —
///    direct evidence that the name reached the synthesizer, which still holds
///    for a voice that is listed but not actually downloaded.
///
/// `say` is deterministic for a given voice/rate/text (verified: three renders
/// of the same input hash identically), which is what makes (2) a clean
/// equality test rather than an audio comparison.
pub fn assert_voices_are_distinct(dir: &Path, voices: &[&str]) {
    let installed = installed_voices();
    for voice in voices {
        assert!(
            installed.iter().any(|listed| listed == voice),
            "`say` does not have the voice {voice:?} on this machine ({} voices \
             listed), and it will NOT say so: it exits 0 and renders with a \
             fallback. Install it (System Settings › Accessibility › Spoken \
             Content › System Voice › Manage Voices…) or change the roster. Do \
             not let a speaker-identity number be published from a roster that \
             silently collapsed.",
            installed.len()
        );
    }

    let mut rendered: Vec<(&str, Vec<u8>)> = Vec::new();
    for voice in voices {
        let wav = synthesize(dir, voice, "voice-probe", VOICE_PROBE, 16_000);
        let bytes = std::fs::read(&wav).expect("read the probe rendering");
        assert!(!bytes.is_empty(), "{voice}'s probe rendered to nothing");
        rendered.push((voice, bytes));
    }
    for (i, (voice_a, a)) in rendered.iter().enumerate() {
        for (voice_b, b) in rendered.iter().skip(i + 1) {
            // NOT `assert_ne!`: these are ~100 KB vectors and a failure should
            // name the two voices, not print the audio.
            assert!(
                a != b,
                "{voice_a} and {voice_b} render the same probe line to \
                 byte-identical audio — they are ONE voice on this machine. \
                 Every pair between them is a same-speaker pair wearing an \
                 impostor label, so every number measured on this roster is \
                 measured on fewer speakers than it names."
            );
        }
    }
}

/// The line [`assert_voices_are_distinct`] renders to prove a voice is real.
/// Short, because it is rendered once per voice purely for its bytes.
const VOICE_PROBE: &str = "This line exists only to prove which voice rendered it.";

/// A fresh scratch directory under Cargo's per-target temp dir.
pub fn scratch(tag: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("yv122-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}
