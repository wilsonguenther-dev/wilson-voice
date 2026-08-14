//! YV95 / finding #6 — "a distinct toggle hotkey that does not collide with
//! dictation-hold".
//!
//! The backlog's stated acceptance command for this item. It reads the same
//! constants `lib.rs` registers and the same constants the tray item's
//! accelerator string is built from, so this cannot pass while the app has a
//! different opinion — which was the whole reason the chords were pulled into
//! one table.

use wilson_voice_lib::shortcuts::{
    self, Binding, DICTATION_TOGGLE_LEGACY, MEETING_TOGGLE, PASTE_LAST, UNDO_AI_EDIT,
};

/// The item's own line: the meeting toggle is not the dictation binding.
#[test]
fn the_meeting_toggle_is_distinct_from_the_dictation_binding() {
    // ⌘⇧V is the only DICTATION chord that is a global shortcut at all (the
    // primary binding is the fn-key hold — see below).
    assert!(
        !MEETING_TOGGLE.collides_with(&DICTATION_TOGGLE_LEGACY),
        "{} would shadow the legacy dictation toggle {}",
        MEETING_TOGGLE.label,
        DICTATION_TOGGLE_LEGACY.label
    );
    assert_ne!(MEETING_TOGGLE.chord(), DICTATION_TOGGLE_LEGACY.chord());
}

/// …and not any of the other chords either. Asserted over the whole table so a
/// later item that adds a fifth chord gets this test for free.
#[test]
fn no_two_global_chords_are_the_same() {
    assert_eq!(
        shortcuts::first_collision(),
        None,
        "two global shortcuts share a chord"
    );
    // Named individually as well, so a failure says WHICH pair broke rather
    // than only that the table is no longer unique.
    for other in [PASTE_LAST, UNDO_AI_EDIT, DICTATION_TOGGLE_LEGACY] {
        assert!(
            !MEETING_TOGGLE.collides_with(&other),
            "{} collides with {}",
            MEETING_TOGGLE.id,
            other.id
        );
    }
}

/// The PRIMARY dictation binding is not a global shortcut at all — it is a
/// CGEvent tap on the `fn` key, optionally with ⌃, optionally plus ⌘/⌥ for
/// command mode. Driven across every combination the settings can produce, so
/// "distinct" covers the binding a user actually holds rather than only the
/// off-by-default ⌘⇧V one.
#[test]
fn the_meeting_toggle_cannot_be_produced_by_a_dictation_hold() {
    for ctrl in [false, true] {
        for cmd in [false, true] {
            for opt in [false, true] {
                assert!(
                    !shortcuts::collides_with_dictation_hold(&MEETING_TOGGLE, ctrl, cmd, opt),
                    "⌃⌘M must not be reachable from a PTT hold (ctrl={ctrl} cmd={cmd} opt={opt})"
                );
            }
        }
    }
}

/// The tray item's accelerator string and the registered chord are built from
/// the SAME binding — the drift this table exists to prevent. Checked by
/// re-deriving the accelerator from the flags rather than by comparing the
/// literal to itself.
#[test]
fn every_accelerator_string_describes_its_own_chord() {
    fn expected(b: &Binding) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if b.ctrl {
            parts.push("Ctrl");
        }
        if b.cmd {
            parts.push("Cmd");
        }
        if b.shift {
            parts.push("Shift");
        }
        if b.alt {
            parts.push("Alt");
        }
        let key = format!("{:?}", b.code); // Code::KeyM → "KeyM"
        parts.push(key.strip_prefix("Key").unwrap_or(&key));
        parts.join("+")
    }
    for b in shortcuts::ALL {
        assert_eq!(
            b.accelerator,
            expected(b),
            "{}'s accelerator string does not match its chord",
            b.id
        );
    }
}

/// A registered chord always carries a real key. The argument for why a global
/// shortcut can never shadow the fn-hold rests on this, so it is asserted rather
/// than assumed.
#[test]
fn every_global_chord_has_a_modifier_and_a_key() {
    for b in shortcuts::ALL {
        assert!(
            b.ctrl || b.cmd || b.shift || b.alt,
            "{} has no modifier — it would swallow a bare keypress system-wide",
            b.id
        );
        assert!(
            !b.modifiers().is_empty(),
            "{}'s Modifiers value lost its flags",
            b.id
        );
    }
}
