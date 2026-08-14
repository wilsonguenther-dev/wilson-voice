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
    tauri_build::build()
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
            let line = corpus_line(&lit);
            if !carries_a_word(&line) || !seen.insert(line.clone()) {
                continue;
            }
            corpus.push_str(&line);
            corpus.push('\n');
        }
    }

    let out: PathBuf = std::env::var("OUT_DIR").expect("OUT_DIR").into();
    std::fs::write(out.join("vocab-corpus.txt"), corpus).expect("write vocab-corpus.txt");
    std::fs::write(out.join("vocab-sources.txt"), names.join("\n") + "\n")
        .expect("write vocab-sources.txt");
}
