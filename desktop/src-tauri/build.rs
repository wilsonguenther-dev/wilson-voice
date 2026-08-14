// YV98: the redaction corpus is extracted HERE, not embedded as source.
//
// `vocab.rs` guarantees that every word surviving a packed log line is a word
// Yap compiled into itself. Its first cut got that corpus by `include_str!`ing
// all 33 files under `src/` — which shipped ~1.4 MB of this crate's own Rust,
// comments and all, inside the signed binary. Only the *parser* skipped
// comments; the *embedding* did not, so `strings Yap` printed the offline
// Ed25519 licensing implementation, the trial and revocation logic, and every
// internal note in the codebase.
//
// The corpus is also LITERALS THAT SHIP: `#[cfg(test)]` items are skipped, so
// the ~6,000 words of English prose this crate's test fixtures carry — an
// allowlist entry every one of them, and none of them in a release build — do
// not decide what may leave a user's machine in a support bundle.
//
// The corpus only ever needed the string literals. So the same extractor runs
// at build time (`src/vocab_extract.rs` is `include!`d below and compiled a
// second time as part of the crate), and only its output — deduplicated, one
// literal per line — is written to `OUT_DIR` for `vocab.rs` to `include_str!`.
// Identical guarantee, no comments, no non-literal source, a fraction of the
// bytes.
//
// The file list is a directory glob rather than a hand-kept array, and the
// names it globbed are written alongside the corpus so
// `vocabulary_covers_every_source_file` can assert a new module still cannot
// silently escape the corpus.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

include!("src/vocab_extract.rs");

fn main() {
    build_vocab_corpus();
    weak_link_coreaudio();
    tauri_build::build()
}

/// YV101 (plan finding OS-11) — CoreAudio is **weak**-linked, and it has to be.
///
/// `tauri.conf.json` sets `minimumSystemVersion: "12.0"` and §2.1 refuses to
/// raise it. But this binary's dependency graph already imports CoreAudio
/// symbols that do not exist that far back — `cpal 0.18`'s macOS loopback
/// module names `AudioHardwareCreateProcessTap` / `AudioHardwareDestroyProcessTap`
/// (macOS 14.2+) and `AudioHardwareCreateAggregateDevice` /
/// `AudioHardwareDestroyAggregateDevice` (macOS 13+), from an object that is
/// reachable from the microphone path and therefore not dead-strippable.
///
/// A **hard** import of a symbol the running OS does not have is not a disabled
/// feature: dyld fails the load and the ENTIRE app refuses to launch, on every
/// macOS 12/13 machine at once, with no in-app error because there is no
/// in-app. `-weak_framework CoreAudio` makes every symbol resolved from that
/// framework a weak import, so a missing one binds to NULL and the process
/// starts normally.
///
/// NULL-bound is not the same as safe: *calling* one would crash. That is what
/// `src/os_version_gate.rs` is for — no process-tap entry point runs without
/// passing the runtime 14.4 check first. This flag stops the launch failure;
/// the gate stops the call.
///
/// Weak-linking the whole framework rather than the four symbols is a
/// deliberate blunt instrument: there is no stable-Rust way to mark an
/// individual `extern` import weak, and the flag is scoped to one framework.
/// Symbols that DO exist on the running OS still bind normally, so the
/// microphone path is unaffected on every OS Yap supports, and a genuinely
/// misspelled symbol still fails at LINK time because ld must still find it in
/// the SDK. `scripts/assert-weak-linked-14_4-symbols.sh` is the standing proof
/// that this flag is still doing its job, run in CI against the release binary.
fn weak_link_coreaudio() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-weak_framework,CoreAudio");
    }
}

fn build_vocab_corpus() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let src = Path::new(&manifest).join("src");
    // Cargo watches a directory argument recursively, so a new module — or an
    // edited literal in an existing one — rebuilds the corpus.
    println!("cargo:rerun-if-changed={}", src.display());

    let mut names: Vec<String> = std::fs::read_dir(&src)
        .expect("read src/")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                return None;
            }
            Some(path.file_name()?.to_string_lossy().into_owned())
        })
        .collect();
    names.sort();
    assert!(!names.is_empty(), "no .rs files under {}", src.display());

    let mut seen: HashSet<String> = HashSet::new();
    let mut corpus = String::new();
    for name in &names {
        let text = std::fs::read_to_string(src.join(name))
            .unwrap_or_else(|e| panic!("read src/{name}: {e}"));
        for lit in string_literals(&text) {
            for line in corpus_lines(&lit) {
                if !carries_a_word(&line) || !seen.insert(line.clone()) {
                    continue;
                }
                corpus.push_str(&line);
                corpus.push('\n');
            }
        }
    }

    let out: PathBuf = std::env::var("OUT_DIR").expect("OUT_DIR").into();
    std::fs::write(out.join("vocab-corpus.txt"), corpus).expect("write vocab-corpus.txt");
    std::fs::write(out.join("vocab-sources.txt"), names.join("\n") + "\n")
        .expect("write vocab-sources.txt");
}
