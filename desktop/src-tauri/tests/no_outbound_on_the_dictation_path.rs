//! PRIV-A — "Audio and text never leave the Mac" as a test, not a sentence.
//!
//! PRIVACY.md makes a claim that is Yap's sharpest marketing line: the audio
//! you dictate and the text it becomes never leave your Mac, Yap ships no
//! analytics, telemetry or crash-upload SDK, and the network is touched in
//! exactly three places — the model download, the updater check, and (only if
//! the user adds one, yap-byom-0) a model provider they chose. This file
//! is the proof that claim cites by name. Every test here reads SOURCE, so it
//! needs no network, no microphone and no app state, and it fails the moment a
//! change makes the claim false.
//!
//! What is swept, and with which pattern, is spelled out per test:
//!
//! * `the_take_path_has_no_url_literal` — SCOPE: the twelve take-path modules
//!   in [`TAKE_PATH`], non-test code. PATTERN: a string literal holding
//!   `<scheme>://<host-ish char>` (see [`url_in`]).
//! * `the_take_path_has_no_http_client_or_socket` — SCOPE: same. PATTERN: the
//!   identifiers in [`HTTP_CLIENT_CRATES`], [`REMOTE_SOCKET_IDENTS`],
//!   [`LOCAL_SOCKET_IDENTS`] and [`UPDATER_IDENTS`], plus a subprocess literal
//!   that names a network tool ([`NETWORK_TOOLS`]).
//! * `the_take_path_does_not_import_the_http_client_crate` — the dependency
//!   sweep, and the gap it cannot close (read its doc comment).
//! * `only_two_network_call_sites_exist` — SCOPE: every `.rs` file under
//!   `desktop/src-tauri/src`, non-test code. The allowlist is
//!   [`NETWORK_CALL_SITES`], by file and function.
//! * `no_send_path_in_crash_or_support` — `crash.rs` and `support.rs` cannot
//!   upload anything; the support bundle leaves only through the user's own
//!   mail compose window.
//! * `the_webview_cannot_connect_out` and `the_sidecars_have_no_network_surface`
//!   close the two doors a Rust-only sweep would leave open.
//!
//! Source is lexed (see [`lex`]) rather than grepped raw: comments are dropped
//! and string literals are separated from code, so a doc comment that merely
//! MENTIONS a client cannot fail the sweep, and a client hidden in code cannot
//! hide behind a comment. `#[cfg(test)]` items are removed, because a unit test
//! that builds a URL to check a parser is not the shipped binary talking.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

// ── Scope ───────────────────────────────────────────────────────────────────

/// The modules a dictation ("take") flows through, from the key going down to
/// the text landing in the target app: capture, voice detection, ASR, the take
/// orchestration, local polish (and its sidecar wire format), paste and its
/// pasteboard transaction, focus capture, snippets, and the local history DB.
const TAKE_PATH: &[&str] = &[
    "record",
    "vad",
    "transcription",
    "asr_engine",
    "dictation",
    "polish",
    "polish_protocol",
    "paste",
    "paste_tx",
    "focus",
    "snippets",
    "db",
];

/// HTTP / websocket client crates, as the identifier a `use` or a path names.
const HTTP_CLIENT_CRATES: &[&str] = &[
    "reqwest",
    "ureq",
    "hyper",
    "isahc",
    "attohttpc",
    "minreq",
    "curl",
    "tungstenite",
    "tokio_tungstenite",
    "async_tungstenite",
    "lettre",
];

/// Anything that opens a socket to another host, in Rust or through AppKit.
const REMOTE_SOCKET_IDENTS: &[&str] = &[
    "TcpStream",
    "TcpListener",
    "UdpSocket",
    "to_socket_addrs",
    "std::net",
    "tokio::net",
    "async_std::net",
    "NSURLSession",
    "NSURLConnection",
    "NSURLSessionTask",
    "CFSocket",
    "CFStream",
    "nw_connection",
];

/// Unix-domain sockets are LOCAL IPC — no host, no route off the machine
/// (`lifecycle.rs` uses one to hand a second launch's argv to the first). They
/// are not network call sites, so the crate-wide allowlist ignores them; the
/// take path and the crash/support modules may not open one all the same.
const LOCAL_SOCKET_IDENTS: &[&str] = &["UnixStream", "UnixListener", "UnixDatagram"];

/// The Tauri updater plugin: the second of the two allowlisted call sites.
const UPDATER_IDENTS: &[&str] = &[
    "tauri_plugin_updater",
    "UpdaterExt",
    "updater",
    "download_and_install",
];

/// A subprocess that can talk to the network. `vad.rs` fetched the Silero
/// model through `Command::new("curl")` until PRIV-A moved that fetch into
/// `models.rs` — a raw grep for `reqwest` never saw it, which is why this list
/// exists.
const NETWORK_TOOLS: &[&str] = &[
    "curl", "wget", "nc", "ncat", "ssh", "scp", "sftp", "rsync", "ftp", "telnet", "nscurl",
];

/// THE ALLOWLIST. Every network call site in the whole crate, by the purpose
/// it serves and then by `(file, function)`. A new site anywhere — a telemetry
/// ping, a license or revocation fetch re-added in `lib.rs` or a new module —
/// is a `(file, fn)` pair missing from this table and fails
/// `only_two_network_call_sites_exist`; an entry that no longer matches code
/// fails it too, so the list cannot rot into a wildcard.
///
/// The revocation-list refresh was DELETED by yap-lic-00-kill-forge-fetch
/// (SECURITY) and is deliberately absent. Licensing returns to the network
/// only through the yap-lic-01..06 items, which must edit this table.
const NETWORK_CALL_SITES: &[(&str, &[(&str, &str)])] = &[
    (
        "model download",
        &[
            // `download_file` delegates to its cancellable form, which is
            // where the request is made.
            ("models.rs", "download_file_cancellable"),
            ("models.rs", "download_attempt"),
            ("models.rs", "ensure_silero_vad_model"),
        ],
    ),
    (
        "updater check",
        &[
            ("lib.rs", "use"),
            ("lib.rs", "check_every_endpoint"),
            ("lib.rs", "check_for_update"),
            ("lib.rs", "install_update"),
            ("lib.rs", "run"),
        ],
    ),
    (
        // yap-byom-0 — a model provider the USER adds and picks (off by
        // default; Bundled stays the default). One client factory, no
        // redirects followed; every other file reaches it through this one.
        "model provider you choose",
        &[("http_client.rs", "use")],
    ),
];

// ── A small Rust lexer ──────────────────────────────────────────────────────

struct Scan {
    /// The source as chars, with every comment and every string/char literal
    /// replaced by spaces (newlines kept, so line numbers survive).
    code: Vec<char>,
    /// `(1-based line, body)` of every string literal (plain, byte, raw).
    strings: Vec<(usize, String)>,
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn blank(c: char, code: &mut Vec<char>) {
    code.push(if c == '\n' { '\n' } else { ' ' });
}

fn lex(src: &str) -> Scan {
    let cs: Vec<char> = src.chars().collect();
    let mut code = Vec::with_capacity(cs.len());
    let mut strings = Vec::new();
    let mut line = 1usize;
    let mut i = 0usize;
    while i < cs.len() {
        let c = cs[i];
        // `//`, `///`, `//!` comments.
        if c == '/' && cs.get(i + 1) == Some(&'/') {
            while i < cs.len() && cs[i] != '\n' {
                code.push(' ');
                i += 1;
            }
            continue;
        }
        // `/* */`, nested.
        if c == '/' && cs.get(i + 1) == Some(&'*') {
            let mut depth = 0usize;
            while i < cs.len() {
                if cs[i] == '/' && cs.get(i + 1) == Some(&'*') {
                    depth += 1;
                    code.extend([' ', ' ']);
                    i += 2;
                    continue;
                }
                if cs[i] == '*' && cs.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    code.extend([' ', ' ']);
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                    continue;
                }
                if cs[i] == '\n' {
                    line += 1;
                }
                blank(cs[i], &mut code);
                i += 1;
            }
            continue;
        }
        // Raw strings: r"…", r#"…"#, br#"…"# (the `b` stays plain code).
        let prefixed =
            i > 0 && is_ident(cs[i - 1]) && !(cs[i - 1] == 'b' && (i < 2 || !is_ident(cs[i - 2])));
        if c == 'r' && !prefixed {
            let mut j = i + 1;
            let mut hashes = 0usize;
            while cs.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            if cs.get(j) == Some(&'"') {
                let start = line;
                let mut k = j + 1;
                let mut body = String::new();
                while k < cs.len() {
                    if cs[k] == '"' && (0..hashes).all(|h| cs.get(k + 1 + h) == Some(&'#')) {
                        break;
                    }
                    if cs[k] == '\n' {
                        line += 1;
                    }
                    body.push(cs[k]);
                    blank(cs[k], &mut code);
                    k += 1;
                }
                code.extend(std::iter::repeat_n(' ', j - i + 2 + hashes));
                strings.push((start, body));
                i = k + 1 + hashes;
                continue;
            }
        }
        // Plain and byte strings.
        if c == '"' {
            let start = line;
            let mut k = i + 1;
            let mut body = String::new();
            code.push(' ');
            while k < cs.len() && cs[k] != '"' {
                if cs[k] == '\\' && k + 1 < cs.len() {
                    for d in [cs[k], cs[k + 1]] {
                        if d == '\n' {
                            line += 1;
                        }
                        body.push(d);
                        blank(d, &mut code);
                    }
                    k += 2;
                    continue;
                }
                if cs[k] == '\n' {
                    line += 1;
                }
                body.push(cs[k]);
                blank(cs[k], &mut code);
                k += 1;
            }
            code.push(' ');
            strings.push((start, body));
            i = k + 1;
            continue;
        }
        // Char literals ('x', '\n', '\'', '\u{..}') vs lifetimes ('a).
        if c == '\'' {
            if cs.get(i + 1) == Some(&'\\') {
                let mut k = i + 3;
                while k < cs.len() && cs[k] != '\'' {
                    k += 1;
                }
                code.extend(std::iter::repeat_n(' ', k + 1 - i));
                i = k + 1;
                continue;
            }
            if cs.get(i + 2) == Some(&'\'') {
                code.extend([' ', ' ', ' ']);
                i += 3;
                continue;
            }
        }
        if c == '\n' {
            line += 1;
        }
        code.push(c);
        i += 1;
    }
    let mut scan = Scan { code, strings };
    strip_cfg_test(&mut scan);
    scan
}

fn line_of(code: &[char], pos: usize) -> usize {
    1 + code[..pos.min(code.len())]
        .iter()
        .filter(|c| **c == '\n')
        .count()
}

fn find_all(code: &[char], needle: &str) -> Vec<usize> {
    let n: Vec<char> = needle.chars().collect();
    let mut out = Vec::new();
    if n.is_empty() || code.len() < n.len() {
        return out;
    }
    for i in 0..=code.len() - n.len() {
        if code[i..i + n.len()] == n[..] {
            out.push(i);
        }
    }
    out
}

/// Whole-identifier occurrences (`curl` is not `curly`, `std::net` is not
/// `std::network`).
fn find_word(code: &[char], word: &str) -> Vec<usize> {
    let len = word.chars().count();
    find_all(code, word)
        .into_iter()
        .filter(|&p| {
            (p == 0 || !is_ident(code[p - 1])) && code.get(p + len).is_none_or(|c| !is_ident(*c))
        })
        .collect()
}

/// Blank every `#[cfg(test)]` item (a `mod tests { … }`, a test-only fn or
/// `use`) and drop the string literals inside it.
fn strip_cfg_test(scan: &mut Scan) {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for at in find_all(&scan.code, "#[cfg(test)]") {
        let mut k = at;
        let mut end = None;
        while k < scan.code.len() {
            match scan.code[k] {
                ';' => {
                    end = Some(k);
                    break;
                }
                '{' => {
                    let mut depth = 0usize;
                    while k < scan.code.len() {
                        match scan.code[k] {
                            '{' => depth += 1,
                            '}' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        k += 1;
                    }
                    end = Some(k.min(scan.code.len() - 1));
                    break;
                }
                _ => k += 1,
            }
        }
        if let Some(e) = end {
            ranges.push((line_of(&scan.code, at), line_of(&scan.code, e)));
            for c in &mut scan.code[at..=e] {
                if *c != '\n' {
                    *c = ' ';
                }
            }
        }
    }
    scan.strings
        .retain(|(l, _)| !ranges.iter().any(|(a, b)| l >= a && l <= b));
}

/// PATTERN for a URL literal: `<scheme>://` where the scheme is
/// `[a-z][a-z0-9+.-]*` and the next character is one a host can start with —
/// a letter, a digit, `[` (IPv6) or `{` (a `format!` placeholder for one).
/// A bare `"https://"` (a prefix test like `token.starts_with("https://")`
/// in polish/vocab, which recognises URLs the USER typed) names no host and
/// is not a URL literal.
fn url_in(s: &str) -> Option<String> {
    let cs: Vec<char> = s.chars().collect();
    for p in find_all(&cs, "://") {
        let mut a = p;
        while a > 0 && (cs[a - 1].is_ascii_alphanumeric() || matches!(cs[a - 1], '+' | '.' | '-')) {
            a -= 1;
        }
        let scheme_ok = a < p && cs[a].is_ascii_alphabetic();
        let host_ok = cs
            .get(p + 3)
            .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '[' | '{'));
        if scheme_ok && host_ok {
            return Some(cs[a..].iter().take(80).collect());
        }
    }
    None
}

fn network_tool(s: &str) -> bool {
    let base = s.trim().rsplit('/').next().unwrap_or_default();
    NETWORK_TOOLS.contains(&base)
}

/// The function a position sits in: the nearest preceding `fn <name>`, or
/// `use` for a module-level import line.
fn enclosing_fn(code: &[char], pos: usize) -> String {
    let line_start = code[..pos]
        .iter()
        .rposition(|c| *c == '\n')
        .map_or(0, |p| p + 1);
    let line: String = code[line_start..]
        .iter()
        .take_while(|c| **c != '\n')
        .collect();
    let t = line.trim_start();
    if t.starts_with("use ") || t.starts_with("pub use ") || t.starts_with("pub(crate) use ") {
        return "use".into();
    }
    let Some(f) = find_word(&code[..pos], "fn").into_iter().last() else {
        return "<module>".into();
    };
    code[f + 2..]
        .iter()
        .skip_while(|c| c.is_whitespace())
        .take_while(|c| is_ident(**c))
        .collect()
}

// ── Files ───────────────────────────────────────────────────────────────────

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap_or_else(|e| panic!("read {}: {e}", d.display())) {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn scan_file(p: &Path) -> Scan {
    lex(&std::fs::read_to_string(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display())))
}

fn take_path_files() -> Vec<(String, PathBuf)> {
    TAKE_PATH
        .iter()
        .map(|m| {
            let p = manifest().join("src").join(format!("{m}.rs"));
            assert!(
                p.is_file(),
                "take-path module {m} moved or was renamed — update TAKE_PATH, \
                 never let the sweep's scope shrink silently ({})",
                p.display()
            );
            (format!("{m}.rs"), p)
        })
        .collect()
}

/// Every network-surface hit in one scanned file: `(what, line, fn)`.
fn network_hits(scan: &Scan, idents: &[&[&str]]) -> Vec<(String, usize, String)> {
    let mut out = Vec::new();
    for list in idents {
        for w in *list {
            for p in find_word(&scan.code, w) {
                out.push((
                    w.to_string(),
                    line_of(&scan.code, p),
                    enclosing_fn(&scan.code, p),
                ));
            }
        }
    }
    for (l, s) in &scan.strings {
        if network_tool(s) {
            // A subprocess literal: attribute it to the fn on that line.
            let pos = scan
                .code
                .iter()
                .enumerate()
                .filter(|(_, c)| **c == '\n')
                .nth(l.saturating_sub(1))
                .map_or(0, |(i, _)| i);
            out.push((
                format!("subprocess {s:?}"),
                *l,
                enclosing_fn(&scan.code, pos),
            ));
        }
    }
    out
}

// ── The pattern can match what we fear ──────────────────────────────────────

#[test]
fn the_sweep_patterns_catch_the_shapes_they_exist_for() {
    let fake = r####"
        use reqwest::Client;
        // reqwest in a comment is not code
        /* std::net::TcpStream in a block comment is not code */
        fn ping() {
            let _ = std::process::Command::new("/usr/bin/curl").arg("x");
            let u = "https://telemetry.example.com/v1/events";
            let f = format!("https://{}/resolve", host);
            let r = r#"wss://socket.example.com"#;
            let ok = token.starts_with("https://");
            let c = '"'; let s = "not a url";
            let _ = TcpStream::connect(addr);
        }
        #[cfg(test)]
        mod tests { fn t() { let _ = "https://only.in.tests.example"; let _ = UdpSocket::bind("x"); } }
    "####;
    let scan = lex(fake);
    let urls: Vec<String> = scan.strings.iter().filter_map(|(_, s)| url_in(s)).collect();
    assert_eq!(
        urls.len(),
        3,
        "a plain, a format! and a raw-string URL must all match, the bare prefix must not, \
         and the cfg(test) one must be stripped: {urls:?}"
    );
    let hits = network_hits(&scan, &[HTTP_CLIENT_CRATES, REMOTE_SOCKET_IDENTS]);
    let what: BTreeSet<String> = hits.iter().map(|(w, _, _)| w.clone()).collect();
    assert!(what.contains("reqwest"), "{hits:?}");
    assert!(what.contains("TcpStream"), "{hits:?}");
    assert!(what.contains("subprocess \"/usr/bin/curl\""), "{hits:?}");
    assert!(
        !what.contains("UdpSocket"),
        "cfg(test) code must be stripped: {hits:?}"
    );
    assert!(
        !what.contains("std::net"),
        "a comment is not code: {hits:?}"
    );
    assert_eq!(
        hits.iter().filter(|(w, _, _)| w == "reqwest").count(),
        1,
        "{hits:?}"
    );
    assert!(
        hits.iter().all(|(w, _, f)| w == "reqwest" || f == "ping"),
        "{hits:?}"
    );
}

// ── The take path ───────────────────────────────────────────────────────────

/// SCOPE: [`TAKE_PATH`], non-test code. PATTERN: [`url_in`] over every string
/// literal. A take-path module that holds a URL is one refactor away from
/// fetching it — the Silero VAD model URL lived in `vad.rs` until PRIV-A.
#[test]
fn the_take_path_has_no_url_literal() {
    let mut found = Vec::new();
    for (name, path) in take_path_files() {
        for (line, s) in scan_file(&path).strings {
            if let Some(u) = url_in(&s) {
                found.push(format!("src/{name}:{line}: {u}"));
            }
        }
    }
    assert!(
        found.is_empty(),
        "URL literals on the dictation path (move them to the model-download or updater \
         code, which are the only places the network is allowed):\n{}",
        found.join("\n")
    );
}

/// SCOPE: [`TAKE_PATH`], non-test code. PATTERN: an HTTP client crate, a
/// remote or local socket, the updater, or a network-tool subprocess.
#[test]
fn the_take_path_has_no_http_client_or_socket() {
    let mut found = Vec::new();
    for (name, path) in take_path_files() {
        let scan = scan_file(&path);
        for (what, line, f) in network_hits(
            &scan,
            &[
                HTTP_CLIENT_CRATES,
                REMOTE_SOCKET_IDENTS,
                LOCAL_SOCKET_IDENTS,
                UPDATER_IDENTS,
            ],
        ) {
            found.push(format!("src/{name}:{line} in fn {f}: {what}"));
        }
    }
    assert!(
        found.is_empty(),
        "the dictation path opened a door to the network:\n{}",
        found.join("\n")
    );
}

/// The DEPENDENCY sweep. `reqwest` is a dependency of the `wilson-voice`
/// crate, and Cargo links one crate graph into one binary — so "the take path
/// does not depend on the HTTP client" is NOT provable from Cargo.toml or the
/// lockfile: the module boundary does not exist at that level. That is the gap,
/// named.
///
/// The narrower thing this test DOES prove, over the whole of each take-path
/// file (tests included): no take-path module names the client crate in a
/// `use`, a path or a type, and none calls one of the allowlisted network
/// functions directly. What it cannot see is an indirect call — a take-path
/// module calling some non-take-path helper that in turn reaches a network
/// function. `only_two_network_call_sites_exist` bounds that: such a helper is
/// itself a call site, and only the two allowlisted ones exist.
#[test]
fn the_take_path_does_not_import_the_http_client_crate() {
    let network_fns: BTreeSet<&str> = NETWORK_CALL_SITES
        .iter()
        .flat_map(|(_, sites)| sites.iter().map(|(_, f)| *f))
        .filter(|f| *f != "use" && *f != "run")
        .chain([
            "download_model",
            "download_model_with",
            "download_diarize_model_with",
        ])
        .collect();
    let mut found = Vec::new();
    for (name, path) in take_path_files() {
        // Tests included: lex without the cfg(test) strip.
        let src = std::fs::read_to_string(&path).unwrap();
        let whole = {
            let mut s = lex(&src.replace("#[cfg(test)]", "#[cfg(tset)]"));
            s.strings.clear();
            s
        };
        for w in HTTP_CLIENT_CRATES {
            for p in find_word(&whole.code, w) {
                found.push(format!(
                    "src/{name}:{}: imports/names {w}",
                    line_of(&whole.code, p)
                ));
            }
        }
        for f in &network_fns {
            for p in find_word(&whole.code, f) {
                found.push(format!(
                    "src/{name}:{}: calls network fn {f}",
                    line_of(&whole.code, p)
                ));
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}

// ── The whole crate ─────────────────────────────────────────────────────────

/// SCOPE: every `.rs` file under `desktop/src-tauri/src` (non-test code).
/// Every hit of an HTTP client, a remote socket, the updater or a network-tool
/// subprocess is mapped to `(file, function)` and must be EXACTLY the
/// allowlist in [`NETWORK_CALL_SITES`]: two purposes, the model download and
/// the updater check. No licensing or revocation fetch exists anywhere.
#[test]
fn only_two_network_call_sites_exist() {
    assert_eq!(
        NETWORK_CALL_SITES.len(),
        3,
        "PRIVACY.md names exactly three network uses (the third, yap-byom-0, only when the \
         user adds a model provider); adding a purpose means editing both"
    );
    let src = manifest().join("src");
    let files = rs_files(&src);
    assert!(
        files.len() > 40,
        "the sweep found too few files: {}",
        files.len()
    );

    let mut found: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for path in &files {
        let rel = path
            .strip_prefix(&src)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let scan = scan_file(path);
        for (what, line, f) in network_hits(
            &scan,
            &[HTTP_CLIENT_CRATES, REMOTE_SOCKET_IDENTS, UPDATER_IDENTS],
        ) {
            found
                .entry((rel.clone(), f))
                .or_default()
                .push(format!("{rel}:{line} {what}"));
        }
    }

    let allowed: BTreeSet<(String, String)> = NETWORK_CALL_SITES
        .iter()
        .flat_map(|(_, sites)| sites.iter().map(|(f, n)| (f.to_string(), n.to_string())))
        .collect();
    let seen: BTreeSet<(String, String)> = found.keys().cloned().collect();

    let unlisted: Vec<String> = seen
        .difference(&allowed)
        .map(|k| format!("{}::{} — {:?}", k.0, k.1, found[k]))
        .collect();
    assert!(
        unlisted.is_empty(),
        "a network call site outside the model download and the updater check. If it is \
         one of those two, add it to NETWORK_CALL_SITES; if it is anything else (telemetry, \
         a licensing or revocation fetch), it breaks the claim PRIVACY.md makes:\n{}",
        unlisted.join("\n")
    );
    let stale: Vec<String> = allowed
        .difference(&seen)
        .map(|k| format!("{}::{}", k.0, k.1))
        .collect();
    assert!(
        stale.is_empty(),
        "allowlist entries that no longer match code — remove them so the list stays exact: {stale:?}"
    );
    assert!(
        !seen.iter().any(|(f, _)| f == "license.rs"),
        "licensing makes no network call (yap-lic-00)"
    );

    // The deleted revocation refresh, by every name it ever had. Built with
    // concat! so this file cannot match itself if it is ever swept.
    let gone = [
        concat!("REVOCATION", "_URL"),
        concat!("ISSUER", "_HOST"),
        concat!("fetch_", "revocations"),
        concat!("spawn_revocation", "_refresh"),
        concat!("revoked", ".json"),
    ];
    for path in &files {
        let text = std::fs::read_to_string(path).unwrap();
        for g in gone {
            assert!(
                !text.contains(g),
                "{} re-adds the revocation fetch ({g}) that yap-lic-00 deleted",
                path.display()
            );
        }
    }
}

// ── Crash capture and the support bundle ────────────────────────────────────

/// `crash.rs` reads crash reports into the local DB; `support.rs` packs a
/// bundle for the USER to send. Neither may upload: no client, no socket
/// (remote or local), no network tool, no URL, no AppleScript (which can tell
/// Mail to send without a click), no SMTP. The bundle's one way out is
/// AppKit's compose-email sharing service, which opens a compose window and
/// cannot send in the background — and that must be the only sharing service.
#[test]
fn no_send_path_in_crash_or_support() {
    for file in ["crash.rs", "support.rs"] {
        let path = manifest().join("src").join(file);
        let scan = scan_file(&path);
        let hits = network_hits(
            &scan,
            &[
                HTTP_CLIENT_CRATES,
                REMOTE_SOCKET_IDENTS,
                LOCAL_SOCKET_IDENTS,
                UPDATER_IDENTS,
            ],
        );
        assert!(hits.is_empty(), "{file} has a network surface: {hits:?}");
        let urls: Vec<String> = scan.strings.iter().filter_map(|(_, s)| url_in(s)).collect();
        assert!(urls.is_empty(), "{file} holds a URL: {urls:?}");
        for w in [
            "NSAppleScript",
            "osascript",
            "smtp",
            "SMTP",
            "mailto",
            "sendmail",
        ] {
            let in_code = !find_word(&scan.code, w).is_empty();
            let in_str = scan.strings.iter().any(|(_, s)| s.contains(w));
            assert!(!in_code && !in_str, "{file} has a send path: {w}");
        }
        let services: BTreeSet<String> = find_all(&scan.code, "NSSharingServiceName")
            .into_iter()
            .map(|p| {
                scan.code[p..]
                    .iter()
                    .take_while(|c| is_ident(**c))
                    .collect()
            })
            .collect();
        let expected: BTreeSet<String> = if file == "support.rs" {
            ["NSSharingServiceNameComposeEmail".to_string()].into()
        } else {
            BTreeSet::new()
        };
        assert_eq!(
            services, expected,
            "{file}: the only sharing service is compose-email"
        );
    }
}

// ── The other two doors ─────────────────────────────────────────────────────

/// The webview is not Rust, so the sweeps above cannot see a `fetch()` from
/// the frontend. The CSP can: `connect-src` must allow only the app itself and
/// Tauri's IPC. And no Tauri plugin that gives JavaScript a network client
/// (http, websocket, upload) is installed on either side.
#[test]
fn the_webview_cannot_connect_out() {
    let conf = std::fs::read_to_string(manifest().join("tauri.conf.json")).unwrap();
    let at = conf
        .find("connect-src")
        .expect("CSP has a connect-src directive");
    let directive: String = conf[at..]
        .chars()
        .take_while(|c| *c != ';' && *c != '"')
        .collect();
    let sources: BTreeSet<&str> = directive.split_whitespace().skip(1).collect();
    let allowed: BTreeSet<&str> = ["'self'", "ipc:", "http://ipc.localhost"].into();
    assert!(
        sources.is_subset(&allowed),
        "the webview may connect to {sources:?}; only {allowed:?} are local"
    );
    let cargo = std::fs::read_to_string(manifest().join("Cargo.toml")).unwrap();
    let pkg = std::fs::read_to_string(manifest().join("..").join("package.json")).unwrap();
    for plugin in ["plugin-http", "plugin-websocket", "plugin-upload"] {
        assert!(
            !cargo.contains(&format!("tauri-{plugin}")),
            "Cargo.toml adds tauri-{plugin}"
        );
        assert!(
            !pkg.contains(&format!("@tauri-apps/{plugin}")),
            "package.json adds {plugin}"
        );
    }
}

/// Polish and diarization run OUT of process, in the `yap-polish` and
/// `yap-diarize` sidecars, over stdio. Transcript text crosses into
/// `yap-polish` on every polished take, so its source is swept with the same
/// patterns, and neither sidecar may declare an HTTP client crate. The gap:
/// this reads each sidecar's own source and Cargo.toml, not the C++ inside
/// llama.cpp / sherpa-onnx (both built without their optional download code).
#[test]
fn the_sidecars_have_no_network_surface() {
    for sidecar in ["yap-polish", "yap-diarize"] {
        let root = manifest().join("..").join(sidecar);
        for path in rs_files(&root.join("src")) {
            let scan = scan_file(&path);
            let hits = network_hits(
                &scan,
                &[
                    HTTP_CLIENT_CRATES,
                    REMOTE_SOCKET_IDENTS,
                    LOCAL_SOCKET_IDENTS,
                    UPDATER_IDENTS,
                ],
            );
            assert!(hits.is_empty(), "{}: {hits:?}", path.display());
            let urls: Vec<String> = scan.strings.iter().filter_map(|(_, s)| url_in(s)).collect();
            assert!(urls.is_empty(), "{}: {urls:?}", path.display());
        }
        let toml = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
        let deps = lex(&toml.replace('#', "//"));
        for c in HTTP_CLIENT_CRATES {
            assert!(
                find_word(&deps.code, c).is_empty(),
                "{sidecar} depends on {c}"
            );
        }
    }
}
