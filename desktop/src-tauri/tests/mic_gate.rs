//! PERM-C — the standing guard on WHERE the microphone gate lives, what ORDER
//! it is asked in, and what it decides.
//!
//! The defect this item closes: `start_recording` is documented as "the ONE
//! gate. Every way to begin a new dictation … funnels into `start_recording`",
//! and until PERM-C the microphone was not checked there at all. Opening the
//! capture stream was treated as permission enough — but a DENIED stream opens
//! and delivers silence, so a revoked grant produced a cheerful, perfectly
//! normal-looking recording of nothing.
//!
//! Two of these tests read `lib.rs` at COMPILE time, the way `license_gate.rs`
//! does, because the property they defend is structural: a second call site, or
//! the two gates swapped, compiles and passes clippy and ships. The third is a
//! real behavioural assertion against the pure decision function the gate is
//! built on — a denied status can never come back "proceed".

use wilson_voice_lib::mic_auth::{gate_decision, MicAuth, MicGate};

const LIB_RS: &str = include_str!("../src/lib.rs");

/// Walk `lib.rs` tracking which top-level `fn` each line belongs to. Same crude
/// attribution as `license_gate.rs`: nearest preceding column-0/4-indented `fn`.
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

/// Code only — a doc comment naming the gate is not a call to it.
fn code_of(line: &str) -> &str {
    line.split("//").next().unwrap_or("")
}

/// ONE call site. The whole promise of the choke point is that there is exactly
/// one place a new dictation can begin; a mic check bolted onto a second path
/// (a tray item, a new hotkey, a command) means the first one can be removed
/// later without anything going red.
#[test]
fn microphone_check_lives_only_in_start_recording() {
    let mut call_sites: Vec<String> = Vec::new();
    for (func, line) in enclosing_fn(LIB_RS) {
        let code = code_of(line);
        if !code.contains("microphone_allows_new_dictation") {
            continue;
        }
        // The definition itself is not a call site.
        if code
            .trim_start()
            .starts_with("fn microphone_allows_new_dictation")
        {
            continue;
        }
        call_sites.push(format!("{func}: {}", line.trim()));
    }
    assert_eq!(
        call_sites.len(),
        1,
        "the microphone gate must be called from exactly one place — \
         `start_recording`, the ONE dictation-start choke point. Found:\n  {}",
        call_sites.join("\n  ")
    );
    assert!(
        call_sites[0].starts_with("start_recording:"),
        "the microphone gate's one call site must be `start_recording`, not `{}`",
        call_sites[0]
    );
}

/// ORDER. "Yap cannot hear you" is the truer message than "buy a license", and a
/// user with no microphone grant must never be shown a purchase prompt. A
/// refactor that reorders two adjacent `if !gate { return; }` lines looks
/// harmless in review and inverts exactly that.
#[test]
fn microphone_is_checked_before_license() {
    let body: Vec<&str> = enclosing_fn(LIB_RS)
        .into_iter()
        .filter(|(f, _)| f == "start_recording")
        .map(|(_, l)| l)
        .collect();
    assert!(!body.is_empty(), "start_recording is gone from lib.rs");
    let mic_at = body
        .iter()
        .position(|l| code_of(l).contains("microphone_allows_new_dictation"))
        .expect("start_recording must consult the microphone gate");
    let license_at = body
        .iter()
        .position(|l| code_of(l).contains("license_allows_new_dictation"))
        .expect("start_recording must consult the license gate");
    assert!(
        mic_at < license_at,
        "the microphone must be checked BEFORE the license — a user with no mic \
         grant must never be shown a purchase prompt (mic at line {mic_at}, \
         license at line {license_at} of start_recording)"
    );
}

/// A denied microphone can never reach the recorder.
///
/// Two halves, because one alone proves nothing. The DECISION half is real
/// behaviour: `gate_decision` is the pure function the gate branches on, and
/// with a stubbed Denied/Restricted status it must answer `Refuse` — never
/// `Proceed`, and never `Prompt` (macOS does not re-prompt after a denial, so a
/// request there is a no-op that makes the hotkey feel dead). The STRUCTURAL
/// half checks that `Refuse` is actually wired to an early return: the gate is
/// consulted, and `record::start_recording` — the call that opens the mic —
/// comes after it, so a false answer returns before a recorder can exist.
#[test]
fn denied_microphone_never_starts_a_recorder() {
    assert_eq!(gate_decision(MicAuth::Denied), MicGate::Refuse);
    assert_eq!(gate_decision(MicAuth::Restricted), MicGate::Refuse);
    assert_eq!(gate_decision(MicAuth::NotDetermined), MicGate::Prompt);
    assert_eq!(gate_decision(MicAuth::Authorized), MicGate::Proceed);
    // An unreadable AVAuthorizationStatus decodes to Restricted, so it refuses.
    assert_eq!(gate_decision(MicAuth::from_raw(97)), MicGate::Refuse);

    let body: Vec<&str> = enclosing_fn(LIB_RS)
        .into_iter()
        .filter(|(f, _)| f == "start_recording")
        .map(|(_, l)| l)
        .collect();
    let guard_at = body
        .iter()
        .position(|l| code_of(l).contains("!microphone_allows_new_dictation"))
        .expect("the microphone gate must be used as a REFUSING guard (`if !…`)");
    assert!(
        body[guard_at + 1..guard_at + 3]
            .iter()
            .any(|l| code_of(l).trim() == "return;"),
        "a false microphone gate must return immediately, not fall through"
    );
    let open_at = body
        .iter()
        .position(|l| code_of(l).contains("record::start_recording"))
        .expect("start_recording opens the capture stream");
    assert!(
        guard_at < open_at,
        "the microphone check must come BEFORE the capture stream is opened"
    );
}
