//! YP2 — the standing guard on how NARROW the license gate is allowed to be.
//!
//! Yap's promise to a trial user whose fortnight ran out is specific: the only
//! thing that stops is starting a NEW dictation. History, search, export,
//! settings, the dictionary, snippets, the scratchpad, model management, crash
//! reports and permissions keep working forever, because the words already
//! spoken are theirs and a paid app that holds them hostage deserves the
//! chargeback it will get.
//!
//! That promise is one careless `if !licensed { return }` away from being false,
//! and nothing else in the build would notice — a gate added to `export_history`
//! compiles, passes clippy, and ships. So this test reads the command layer's
//! own source at COMPILE time and asserts the gate appears in exactly the two
//! places it is allowed to: the single choke point every dictation start funnels
//! through, and the command that a UI button calls.
//!
//! If you are here because this test failed: that is the point. Either move your
//! check onto the dictation-start path, or come and change this list on purpose.

const LIB_RS: &str = include_str!("../src/lib.rs");
const LICENSE_RS: &str = include_str!("../src/license.rs");

/// Functions in `lib.rs` allowed to consult the license.
const ALLOWED_CALLERS: &[&str] = &[
    // The gate itself.
    "license_allows_new_dictation",
    // PERM-C, added on purpose. The MICROPHONE gate sits beside the license gate
    // on the same choke point and its name ends in the same words, so its own
    // `fn` line matches `allows_new_dictation` here. It gates nothing on the
    // license — `tests/mic_gate.rs` owns it — but it must be named, not
    // pattern-matched away, or the next rename hides a real escape.
    "microphone_allows_new_dictation",
    // The ONE choke point: hotkey, hands-free, tray, pill, Home button and
    // onboarding calibration all reach capture through here.
    "start_recording",
    // The UI-facing command, so a button can answer with a typed
    // `license_required` instead of doing nothing.
    "manual_toggle",
    // Read-only surfaces + activation. These report or change the license; they
    // do not gate a feature on it.
    "license_status",
    "activate_license",
    "deactivate_license",
    "spawn_revocation_refresh",
    // The typed error's own constructor — it names the code, it does not decide
    // anything.
    "license_required",
];

/// Every way this file asks "may we?" — matched as substrings of a line.
const GATE_CALLS: &[&str] = &[
    "allows_new_dictation",
    "license_allows_new_dictation",
    "LICENSE_REQUIRED",
    "license_required()",
];

/// Walk `lib.rs` tracking which top-level `fn` each line belongs to. Crude on
/// purpose: it only needs to attribute a line to the nearest preceding
/// column-0/4-indented `fn`, which is exactly how this file is written.
fn enclosing_fn(source: &str) -> Vec<(String, &str)> {
    let mut current = String::from("<file scope>");
    let mut out = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if indent <= 4 {
            if let Some(rest) = trimmed
                .strip_prefix("fn ")
                .or_else(|| trimmed.strip_prefix("pub fn "))
                .or_else(|| trimmed.strip_prefix("async fn "))
                .or_else(|| trimmed.strip_prefix("pub async fn "))
            {
                current = rest
                    .split(|c: char| c == '(' || c == '<' || c.is_whitespace())
                    .next()
                    .unwrap_or("")
                    .to_string();
            }
        }
        out.push((current.clone(), line));
    }
    out
}

#[test]
fn license_gate_lives_only_on_the_new_dictation_path() {
    let mut offenders: Vec<String> = Vec::new();
    for (func, line) in enclosing_fn(LIB_RS) {
        let code = line.split("//").next().unwrap_or("");
        if !GATE_CALLS.iter().any(|needle| code.contains(needle)) {
            continue;
        }
        if ALLOWED_CALLERS.contains(&func.as_str()) {
            continue;
        }
        offenders.push(format!("{func}: {}", line.trim()));
    }
    assert!(
        offenders.is_empty(),
        "the license gate escaped the dictation-start path — a trial user must keep \
         history, exports and settings forever. Offending call sites:\n  {}",
        offenders.join("\n  ")
    );
}

/// The commands that must NEVER be able to fail because of a license. Named
/// individually so deleting one from the app is a deliberate act.
const UNGATED_COMMANDS: &[&str] = &[
    "get_history",
    "export_history",
    "get_settings",
    "save_settings",
    "copy_entry",
    "paste_entry",
    "delete_entry",
    "clear_history",
    "list_snippets",
    "list_dictionary",
    "list_scratch",
    "save_scratch",
    "get_insights",
    "daily_series",
    "list_models",
    "download_model",
    "select_model",
    "get_status",
    "get_permissions",
    "open_data_dir",
];

#[test]
fn license_free_surfaces_stay_registered_and_ungated() {
    let by_fn = enclosing_fn(LIB_RS);
    for command in UNGATED_COMMANDS {
        assert!(
            by_fn.iter().any(|(f, _)| f == command),
            "`{command}` is gone from lib.rs — if it was renamed, rename it here too"
        );
        for (func, line) in &by_fn {
            if func != command {
                continue;
            }
            let code = line.split("//").next().unwrap_or("");
            assert!(
                !GATE_CALLS.iter().any(|n| code.contains(n)),
                "`{command}` must never consult the license: {}",
                line.trim()
            );
        }
    }
}

/// The gate is only as good as the claim that everything funnels through
/// `start_recording`. If a new code path ever opens the capture stream
/// directly — a new hotkey, a shortcut, an AppleScript hook — it walks straight
/// past the license, and the gate above would still pass because it is looking
/// at the wrong end. So assert the choke point itself: `record::start_recording`
/// (the call that actually opens the mic) may only be reached from the one
/// wrapper that asks the license first.
#[test]
fn capture_can_only_be_opened_through_the_gated_wrapper() {
    let mut callers: Vec<String> = Vec::new();
    for (func, line) in enclosing_fn(LIB_RS) {
        let code = line.split("//").next().unwrap_or("");
        if code.contains("record::start_recording") {
            callers.push(func);
        }
    }
    assert_eq!(
        callers,
        vec!["start_recording".to_string()],
        "the mic may only be opened from the gated `start_recording` wrapper, \
         but it is also opened from: {callers:?}"
    );

    // …and that wrapper must ask the license before it does anything else that
    // can reach the stream.
    let body: Vec<&str> = enclosing_fn(LIB_RS)
        .into_iter()
        .filter(|(f, _)| f == "start_recording")
        .map(|(_, l)| l)
        .collect();
    let gate_at = body
        .iter()
        .position(|l| l.contains("license_allows_new_dictation"))
        .expect("start_recording must consult the license");
    let open_at = body
        .iter()
        .position(|l| l.contains("record::start_recording"))
        .expect("start_recording opens the capture stream");
    assert!(
        gate_at < open_at,
        "the license check must come BEFORE the capture stream is opened"
    );
}

/// The pinned issuer key is a PUBLIC key and a private one must never end up
/// beside it. PKCS#8/PEM markers in the license module would mean exactly that.
#[test]
fn license_module_holds_no_private_key_material() {
    // The markers are assembled rather than written out, so that *this file*
    // does not itself contain the string it is looking for — a repo-wide
    // "no private key material anywhere" grep has to come back empty, and a
    // guard that trips its own scanner is a guard nobody can automate around.
    for marker in [
        concat!("BEGIN ", "PRIVATE KEY"),
        concat!("BEGIN ", "OPENSSH PRIVATE KEY"),
        concat!("BEGIN ", "EC PRIVATE KEY"),
        "SigningKey::from_bytes(&[0",
    ] {
        assert!(
            !LICENSE_RS.contains(marker),
            "license.rs must never carry private key material (`{marker}`)"
        );
    }
    // The signing half lives on the issuer box; only verification lives here.
    let production_code = LICENSE_RS
        .split("mod tests {")
        .next()
        .expect("license.rs has a production half");
    for signing_symbol in ["SigningKey::", "ed25519_dalek::Signer", ".sign("] {
        assert!(
            !production_code.contains(signing_symbol),
            "the shipped half of license.rs must only ever VERIFY, never sign (`{signing_symbol}`)"
        );
    }
}

/// LIC-A — the issuer moved INTO this repository, so the "no private key
/// material" guard has to move with it. `supabase/functions/**` is signing
/// code: it imports a PKCS#8 key from the environment at cold start, and the
/// day somebody pastes that key into the file for a quick local test is the day
/// it ends up in a public GitHub repo.
///
/// This walks the real directory rather than `include_str!`-ing known files, so
/// a NEW file added to the issuer is covered the moment it exists.
#[test]
fn the_issuer_sources_hold_no_private_key_material() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../supabase")
        .canonicalize()
        .expect("supabase/ is part of this repository");

    let mut stack = vec![root.clone()];
    let mut checked = 0usize;
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("readable issuer directory") {
            let path = entry.expect("readable entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue; // not text; nothing a PEM hides in
            };
            checked += 1;
            for marker in [
                concat!("BEGIN ", "PRIVATE KEY"),
                concat!("BEGIN ", "OPENSSH PRIVATE KEY"),
                concat!("BEGIN ", "EC PRIVATE KEY"),
                "whsec_live",
                "sk_live_",
                "re_live_",
                // The Drivia project. Yap's issuer is a DEDICATED project on
                // purpose: a licensing outage caused by an unrelated product's
                // free-tier usage is the worst possible coupling.
                // Split so that grepping this repository for the ref finds
                // ZERO files: the guard must not be the one hit that makes a
                // "is the Drivia ref anywhere in the tree?" search look dirty.
                concat!("vlfrzdbq", "wsnrosmcygca"),
            ] {
                assert!(
                    !source.contains(marker),
                    "{}: the issuer sources must never carry a secret (`{marker}`)",
                    path.display()
                );
            }
        }
    }
    assert!(
        checked > 0,
        "found no issuer sources to check — did the path move?"
    );
}
