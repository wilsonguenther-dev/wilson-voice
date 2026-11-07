//! yap-1107-use-original-take-managed-state — every `#[tauri::command]` asks
//! for state the app actually manages.
//!
//! Tauri resolves `State<'_, T>` by EXACT type at invoke time. A command that
//! asks for `State<'_, AppState>` while the builder only `.manage()`s an
//! `Arc<AppState>` compiles fine and then fails every call with "state not
//! managed for field `state` on command ...". That shipped twice on
//! 2026-11-07 (`use_original_take`, `set_take_feedback`).
//!
//! This guard source-parses `src/` (every `.rs` file, not a hard-coded list):
//! it collects every command's `State<'_, T>` / `tauri::State<'_, T>` parameter
//! (multi-line signatures included), resolves the type of every `.manage(..)`
//! / `app.manage(..)` argument (`let state = Arc::new(AppState { .. })` then
//! `.manage(state.clone())` is `Arc<AppState>`), and fails naming each command
//! whose `T` is never managed. Source-parse rather than `tauri::test`: the real
//! manage calls live inside `run()` and its setup hook, which need a running
//! app. Comments and string literals are blanked first, so a doc comment that
//! mentions `#[tauri::command]` never counts.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Blank comments and the insides of string / char literals (newlines kept, so
/// byte offsets still give line numbers). The result is ASCII only, so a byte
/// offset is a char offset.
fn clean(src: &str) -> String {
    let c: Vec<char> = src.chars().collect();
    let n = c.len();
    let mut out = String::with_capacity(n);
    let is_ident = |ch: char| ch.is_ascii_alphanumeric() || ch == '_';
    let blank = |out: &mut String, ch: char| out.push(if ch == '\n' { '\n' } else { ' ' });
    let mut i = 0;
    while i < n {
        let ch = c[i];
        let next = c.get(i + 1).copied();
        if ch == '/' && next == Some('/') {
            while i < n && c[i] != '\n' {
                out.push(' ');
                i += 1;
            }
            continue;
        }
        if ch == '/' && next == Some('*') {
            let mut depth = 0usize;
            while i < n {
                if c[i] == '/' && c.get(i + 1) == Some(&'*') {
                    depth += 1;
                    out.push_str("  ");
                    i += 2;
                } else if c[i] == '*' && c.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    out.push_str("  ");
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    blank(&mut out, c[i]);
                    i += 1;
                }
            }
            continue;
        }
        // Raw strings: r"..", r#".."#, br#".."# (not the raw identifier r#type).
        let prev = if i > 0 { Some(c[i - 1]) } else { None };
        let prev2 = if i > 1 { Some(c[i - 2]) } else { None };
        let raw_start = ch == 'r'
            && (prev.map_or(true, |p| !is_ident(p))
                || (prev == Some('b') && prev2.map_or(true, |p| !is_ident(p))));
        if raw_start {
            let mut j = i + 1;
            let mut hashes = 0usize;
            while j < n && c[j] == '#' {
                hashes += 1;
                j += 1;
            }
            if j < n && c[j] == '"' {
                out.push('r');
                for _ in 0..hashes {
                    out.push('#');
                }
                out.push('"');
                j += 1;
                loop {
                    if j >= n {
                        break;
                    }
                    if c[j] == '"' && (0..hashes).all(|k| c.get(j + 1 + k) == Some(&'#')) {
                        out.push('"');
                        for _ in 0..hashes {
                            out.push('#');
                        }
                        j += 1 + hashes;
                        break;
                    }
                    blank(&mut out, c[j]);
                    j += 1;
                }
                i = j;
                continue;
            }
        }
        if ch == '"' {
            out.push('"');
            i += 1;
            while i < n && c[i] != '"' {
                if c[i] == '\\' {
                    out.push(' ');
                    i += 1;
                    if i < n {
                        blank(&mut out, c[i]);
                        i += 1;
                    }
                    continue;
                }
                blank(&mut out, c[i]);
                i += 1;
            }
            if i < n {
                out.push('"');
                i += 1;
            }
            continue;
        }
        if ch == '\'' {
            // '\n', '\'', '\u{..}' — an escaped char literal.
            if next == Some('\\') {
                let mut j = i + 3;
                while j < n && c[j] != '\'' {
                    j += 1;
                }
                out.push_str("'x'");
                for k in (i + 3)..j.min(n) {
                    blank(&mut out, c[k]);
                }
                i = (j + 1).min(n);
                continue;
            }
            // 'x' (also '"') — a plain char literal; otherwise a lifetime.
            if c.get(i + 2) == Some(&'\'') {
                out.push_str("'x'");
                i += 3;
                continue;
            }
        }
        out.push(if ch.is_ascii() { ch } else { ' ' });
        i += 1;
    }
    out
}

fn line_of(s: &str, at: usize) -> usize {
    s.as_bytes()[..at].iter().filter(|&&b| b == b'\n').count() + 1
}

/// Index of the bracket closing the one at `open` (same kind). `<`/`>` skip
/// the `>` of `->`.
fn matching(b: &[u8], open: usize) -> Option<usize> {
    let (o, c) = match b[open] {
        b'(' => (b'(', b')'),
        b'[' => (b'[', b']'),
        b'{' => (b'{', b'}'),
        b'<' => (b'<', b'>'),
        _ => return None,
    };
    let mut depth = 0i64;
    for (i, &ch) in b.iter().enumerate().skip(open) {
        if ch == o {
            depth += 1;
        } else if ch == c && !(c == b'>' && i > 0 && b[i - 1] == b'-') {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// Split at `sep` outside every bracket pair.
fn split_top(s: &str, sep: u8) -> Vec<String> {
    let b = s.as_bytes();
    let mut depth = 0i64;
    let mut parts = Vec::new();
    let mut start = 0;
    for (i, &ch) in b.iter().enumerate() {
        match ch {
            b'(' | b'[' | b'{' | b'<' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'>' if !(i > 0 && b[i - 1] == b'-') => depth -= 1,
            _ if ch == sep && depth == 0 => {
                parts.push(s[start..i].trim().to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    let last = s[start..].trim();
    if !last.is_empty() {
        parts.push(last.to_string());
    }
    parts
}

fn squash(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

/// One spelling per type: no whitespace, no `crate::` / `self::` / leading
/// `::`, `std::sync::Arc` as `Arc`.
fn normalize(ty: &str) -> String {
    let t = squash(ty);
    let t = t.trim_start_matches("::").to_string();
    t.replace("std::sync::Arc<", "Arc<")
        .replace("crate::", "")
        .replace("self::", "")
}

/// `T` of a `State<'_, T>` / `tauri::State<'_, T>` parameter type.
fn state_inner(ty: &str) -> Option<String> {
    let t = squash(ty);
    let t = t.trim_start_matches("::");
    let t = t.strip_prefix("tauri::").unwrap_or(t);
    let inner = t.strip_prefix("State<")?.strip_suffix('>')?;
    let args = split_top(inner, b',');
    let first = args.first()?;
    let ty = if first.starts_with('\'') {
        args.get(1)?
    } else {
        first
    };
    Some(normalize(ty))
}

#[derive(Debug, Clone)]
struct Command {
    name: String,
    at: String,
    states: Vec<String>,
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The next whole-word `word` at or after `from`.
fn find_word(s: &str, from: usize, word: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = from;
    while let Some(off) = s.get(i..)?.find(word) {
        let at = i + off;
        let end = at + word.len();
        let before_ok = at == 0 || !is_ident_byte(b[at - 1]);
        let after_ok = end >= b.len() || !is_ident_byte(b[end]);
        if before_ok && after_ok {
            return Some(at);
        }
        i = at + 1;
    }
    None
}

fn read_ident(b: &[u8], mut i: usize) -> (String, usize) {
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    let start = i;
    while i < b.len() && is_ident_byte(b[i]) {
        i += 1;
    }
    (String::from_utf8_lossy(&b[start..i]).into_owned(), i)
}

/// Every `#[tauri::command]` / `#[tauri::command(..)]` fn in cleaned source.
fn commands_in(s: &str, file: &str) -> Vec<Command> {
    const ATTR: &str = "#[tauri::command";
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(off) = s[from..].find(ATTR) {
        let at = from + off;
        from = at + 1;
        let after = at + ATTR.len();
        if !matches!(b.get(after), Some(b']') | Some(b'(')) {
            continue;
        }
        let Some(attr_end) = matching(b, at + 1) else {
            continue;
        };
        let Some(fn_at) = find_word(s, attr_end + 1, "fn") else {
            continue;
        };
        let (name, mut i) = read_ident(b, fn_at + 2);
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if b.get(i) == Some(&b'<') {
            if let Some(g) = matching(b, i) {
                i = g + 1;
            }
        }
        let Some(open) = s[i..].find('(').map(|o| i + o) else {
            continue;
        };
        let Some(close) = matching(b, open) else {
            continue;
        };
        let mut states = Vec::new();
        for param in split_top(&s[open + 1..close], b',') {
            // `pat: Type` — the first lone ':' (not part of `::`).
            let pb = param.as_bytes();
            let colon = (0..pb.len()).find(|&k| {
                pb[k] == b':' && pb.get(k + 1) != Some(&b':') && (k == 0 || pb[k - 1] != b':')
            });
            if let Some(k) = colon {
                if let Some(t) = state_inner(&param[k + 1..]) {
                    states.push(t);
                }
            }
        }
        out.push(Command {
            name,
            at: format!("{file}:{}", line_of(s, fn_at)),
            states,
        });
    }
    out
}

/// The type of a `.manage(expr)` argument, read from the source before `pos`.
fn resolve(s: &str, pos: usize, expr: &str, depth: usize) -> Result<String, String> {
    if depth > 64 {
        return Err(format!("binding chain too deep for {expr:?}"));
    }
    let mut e = expr.trim().trim_start_matches('&').trim().to_string();
    while let Some(rest) = e.strip_suffix(".clone()") {
        e = rest.trim_end().to_string();
    }
    let sq = squash(&e);
    for arc in [
        "Arc::new(",
        "std::sync::Arc::new(",
        "::std::sync::Arc::new(",
    ] {
        if e.starts_with(arc) {
            let open = arc.len() - 1;
            if matching(e.as_bytes(), open) == Some(e.len() - 1) {
                let inner = resolve(s, pos, &e[open + 1..e.len() - 1], depth + 1)?;
                return Ok(format!("Arc<{inner}>"));
            }
        }
    }
    if !sq.is_empty() && sq.bytes().all(is_ident_byte) {
        return resolve_binding(s, pos, &sq, depth);
    }
    let eb = e.as_bytes();
    let mut i = 0;
    while i < eb.len() && (is_ident_byte(eb[i]) || eb[i] == b':') {
        i += 1;
    }
    let path = e[..i].trim_end_matches(':').to_string();
    let rest = e[i..].trim_start();
    let last_upper = |p: &str| {
        p.rsplit("::")
            .next()
            .and_then(|seg| seg.chars().next())
            .map_or(false, |c| c.is_ascii_uppercase())
    };
    if rest.starts_with('{') && last_upper(path.as_str()) {
        return Ok(normalize(&path));
    }
    if rest.starts_with('(') {
        if last_upper(path.as_str()) {
            return Ok(normalize(&path)); // tuple struct
        }
        if let Some((ty, _ctor)) = path.rsplit_once("::") {
            if last_upper(ty) {
                return Ok(normalize(ty)); // Type::new(..) / Type::with_x(..)
            }
        }
    }
    Err(format!("cannot read the type of .manage({expr})"))
}

/// The nearest `let [mut] name [: T] = init;` before `pos`.
fn resolve_binding(s: &str, pos: usize, name: &str, depth: usize) -> Result<String, String> {
    let b = s.as_bytes();
    let mut best: Option<(usize, usize)> = None; // (let_at, after_name)
    let mut from = 0;
    while let Some(let_at) = find_word(s, from, "let") {
        if let_at >= pos {
            break;
        }
        from = let_at + 3;
        let (mut word, mut i) = read_ident(b, let_at + 3);
        if word == "mut" {
            (word, i) = read_ident(b, i);
        }
        if word == name {
            best = Some((let_at, i));
        }
    }
    let Some((let_at, mut i)) = best else {
        return Err(format!("no `let {name}` before its .manage()"));
    };
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    let tail = &s[i..];
    let init_end = |from: usize| -> usize {
        // the top-level ';' ending the statement
        let mut depth = 0i64;
        for (k, &ch) in b.iter().enumerate().skip(from) {
            match ch {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                b';' if depth == 0 => return k,
                _ => {}
            }
        }
        b.len()
    };
    if tail.starts_with(':') && !tail.starts_with("::") {
        let eq = split_top(&s[i + 1..init_end(i + 1)], b'=');
        let ty = eq
            .first()
            .ok_or_else(|| format!("empty type on `let {name}`"))?;
        return Ok(normalize(ty));
    }
    if tail.starts_with('=') {
        let end = init_end(i + 1);
        return resolve(s, let_at, &s[i + 1..end], depth + 1);
    }
    Err(format!("cannot read `let {name}`"))
}

/// Every type `.manage(..)`d in cleaned source, with where.
fn managed_in(s: &str, file: &str) -> Result<Vec<(String, String)>, String> {
    const CALL: &str = ".manage(";
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(off) = s[from..].find(CALL) {
        let at = from + off;
        from = at + 1;
        let open = at + CALL.len() - 1;
        let close = matching(b, open)
            .ok_or_else(|| format!("{file}:{} unclosed .manage(", line_of(s, at)))?;
        let ty = resolve(s, at, &s[open + 1..close], 0)
            .map_err(|e| format!("{file}:{} {e}", line_of(s, at)))?;
        out.push((ty, format!("{file}:{}", line_of(s, at))));
    }
    Ok(out)
}

/// `"name (file:line) asks for State<'_, T>"` for each unmanaged `T`.
fn offenders(commands: &[Command], managed: &BTreeSet<String>) -> Vec<String> {
    let mut bad = Vec::new();
    for c in commands {
        for t in &c.states {
            if !managed.contains(t) {
                bad.push(format!("{} ({}) asks for State<'_, {t}>", c.name, c.at));
            }
        }
    }
    bad
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

#[test]
fn every_tauri_command_asks_for_state_the_app_manages() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(!files.is_empty(), "no .rs files under {}", src.display());

    let mut commands = Vec::new();
    let mut managed = BTreeSet::new();
    let mut manage_sites = Vec::new();
    for f in &files {
        let text =
            std::fs::read_to_string(f).unwrap_or_else(|e| panic!("read {}: {e}", f.display()));
        let rel = f
            .strip_prefix(&src)
            .unwrap_or(f.as_path())
            .display()
            .to_string();
        let s = clean(&text);
        commands.extend(commands_in(&s, &rel));
        for (ty, at) in managed_in(&s, &rel).unwrap_or_else(|e| panic!("{e}")) {
            manage_sites.push(format!("{ty} at {at}"));
            managed.insert(ty);
        }
    }

    // The parser must really see the app, or the guard passes vacuously.
    assert!(
        commands.len() >= 100,
        "found only {} #[tauri::command] fns",
        commands.len()
    );
    let with_state = commands.iter().filter(|c| !c.states.is_empty()).count();
    assert!(
        with_state >= 50,
        "found only {with_state} commands taking State<'_, T>"
    );
    assert!(
        managed.contains("Arc<AppState>"),
        "Arc<AppState> is not managed? manage sites: {manage_sites:?}"
    );

    let bad = offenders(&commands, &managed);
    assert!(
        bad.is_empty(),
        "{} command(s) ask for state the app never .manage()s — Tauri rejects every call with \
         \"state not managed\". Managed: {manage_sites:?}\n{}",
        bad.len(),
        bad.join("\n")
    );
}

#[test]
fn parser_reads_multiline_signatures_and_both_spellings() {
    let src = r##"
/// #[tauri::command] fn ghost(s: State<'_, Ghost>) {} — a doc comment, never counted
/* #[tauri::command]
   fn ghost2(s: tauri::State<'_, Ghost>) {} */
const S: &str = "#[tauri::command] fn ghost3(s: State<'_, Ghost>)";
const C: char = '"';
#[tauri::command]
pub async fn spread(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    pairs: Vec<(u8, u8)>,
    cb: Option<Box<dyn Fn(u8) -> u8>>,
) -> Result<String, String> { Ok(String::new()) }

#[tauri::command(async)]
fn bare(lic: State<'_, Arc<crate::license_client::LicenseClient>>, s: State<'_, AppState>) {}

#[tauri::command]
fn plain(id: String) {}
"##;
    let s = clean(src);
    let cmds = commands_in(&s, "x.rs");
    let names: Vec<&str> = cmds.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["spread", "bare", "plain"]);
    assert_eq!(cmds[0].states, ["Arc<AppState>"]);
    assert_eq!(
        cmds[1].states,
        ["Arc<license_client::LicenseClient>", "AppState"]
    );
    assert!(cmds[2].states.is_empty());
}

#[test]
fn resolver_reads_the_managed_types_through_bindings() {
    let src = r#"
fn run() {
    let state = Arc::new(AppState {
        db: Db::open("x}"),
        n: 1,
    });
    let builder = tauri::Builder::default();
    {
        let state = state.clone();
        let builder = builder.manage(state.clone());
    }
    let lic_client = Arc::new(license_client::LicenseClient::new(
        state.license.clone(),
        Arc::new(license_client::HttpTransport),
    ));
    app.manage(lic_client.clone());
    let typed: std::sync::Arc<Other> = make();
    app.manage(typed);
}
"#;
    let s = clean(src);
    let got: BTreeSet<String> = managed_in(&s, "x.rs")
        .unwrap()
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    let want: BTreeSet<String> = [
        "Arc<AppState>",
        "Arc<license_client::LicenseClient>",
        "Arc<Other>",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(got, want);
}

#[test]
fn a_command_asking_for_bare_app_state_is_named() {
    // The 2026-11-07 shape: the app manages Arc<AppState>, two commands ask
    // for AppState.
    let src = r#"
#[tauri::command]
async fn use_original_take(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<String, String> { todo!() }
#[tauri::command]
fn set_take_feedback(state: tauri::State<'_, AppState>, id: String, feedback: Option<i64>) {}
#[tauri::command]
fn fine(state: State<'_, Arc<AppState>>) {}
fn run() {
    let state = Arc::new(AppState { n: 1 });
    tauri::Builder::default().manage(state.clone());
}
"#;
    let s = clean(src);
    let cmds = commands_in(&s, "lib.rs");
    let managed: BTreeSet<String> = managed_in(&s, "lib.rs")
        .unwrap()
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    let bad = offenders(&cmds, &managed);
    assert_eq!(bad.len(), 2, "{bad:?}");
    assert!(
        bad[0].starts_with("use_original_take (lib.rs:3)"),
        "{bad:?}"
    );
    assert!(
        bad[1].starts_with("set_take_feedback (lib.rs:9)"),
        "{bad:?}"
    );
}
