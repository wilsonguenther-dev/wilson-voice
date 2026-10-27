//! Y8-A — Wispr's four shortcut rules, one pure test per rule, no app.
//!
//! The rules live in `shortcuts::validate`; `hotkey_table::chord_for_action`
//! runs rules 1–3 on every Settings rebind of a Carbon action and the hotkey
//! table raises rule 4 on the Dictate (hold) row. The last tests here drive
//! that real rebind path, so a rule that is defined but never called fails.

use tauri_plugin_global_shortcut::Code;
use wilson_voice_lib::hotkey_table::{chord_for_action, stored_chord_refusal, BindingAction};
use wilson_voice_lib::hotkeys::ChordSetting;
use wilson_voice_lib::shortcuts::validate::{
    self as rules, key, left, left_and_right_variants_are_one_key, m, overlap_with_ptt, ptt_chords,
    ptt_self_overlap, right, Modifier, OverlapKind, Violation,
};

/// Rule 1 — a chord is at most three keys.
#[test]
fn at_most_three_keys() {
    let three = [m(Modifier::Control), m(Modifier::Command), key(Code::KeyH)];
    assert_eq!(rules::at_most_three_keys(&three), Ok(()));
    let four = [
        m(Modifier::Control),
        m(Modifier::Option),
        m(Modifier::Command),
        key(Code::KeyH),
    ];
    match rules::at_most_three_keys(&four) {
        Err(Violation::TooManyKeys { count, label }) => {
            assert_eq!(count, 4);
            assert_eq!(label, "⌃⌥⌘H");
        }
        other => panic!("four keys must be refused, got {other:?}"),
    }
    assert_eq!(rules::at_most_three_keys(&[]), Err(Violation::Empty));
}

/// Rule 2 — fn, ⌃, ⌥ or ⌘ is required; ⇧ alone types a capital, so it is
/// not enough.
#[test]
fn a_modifier_is_required() {
    assert!(matches!(
        rules::a_modifier_is_required(&[key(Code::KeyH)]),
        Err(Violation::NoModifier { .. })
    ));
    assert!(matches!(
        rules::a_modifier_is_required(&[m(Modifier::Shift), key(Code::KeyH)]),
        Err(Violation::NoModifier { .. })
    ));
    for md in [
        Modifier::Fn,
        Modifier::Control,
        Modifier::Option,
        Modifier::Command,
    ] {
        assert_eq!(
            rules::a_modifier_is_required(&[m(md), key(Code::KeyH)]),
            Ok(()),
            "{md:?}"
        );
    }
}

/// Rule 3 — left ⌘ and right ⌘ are one key, not two.
#[test]
fn left_and_right_variants_are_not_two_keys() {
    let both_cmds = [
        left(Modifier::Command),
        right(Modifier::Command),
        key(Code::KeyK),
    ];
    match left_and_right_variants_are_one_key(&both_cmds) {
        Err(Violation::LeftRightDuplicate { modifier, .. }) => {
            assert_eq!(modifier, Modifier::Command)
        }
        other => panic!("left+right ⌘ must be refused, got {other:?}"),
    }
    // Two DIFFERENT modifiers on different sides are fine.
    let ok = [
        left(Modifier::Command),
        right(Modifier::Option),
        key(Code::KeyK),
    ];
    assert_eq!(left_and_right_variants_are_one_key(&ok), Ok(()));
    // The message names the duplicate, so the user can see what to drop.
    let msg = left_and_right_variants_are_one_key(&both_cmds)
        .unwrap_err()
        .to_string();
    assert!(msg.contains("left and right ⌘"), "{msg}");
}

/// Rule 4 — the overlap Yap's own default pair produces: fn vs fn⌃ WARNS, in
/// both directions, and `ptt_binding: "both"` (which arms both) raises it.
#[test]
fn fn_versus_fn_control_warns_of_overlap() {
    let fn_only = [m(Modifier::Fn)];
    let fn_ctrl = [m(Modifier::Fn), m(Modifier::Control)];

    let o = overlap_with_ptt(&fn_ctrl, &fn_only).expect("fn⌃ over a fn PTT must warn");
    assert_eq!(o.kind, OverlapKind::ContainsPtt);
    assert_eq!((o.candidate.as_str(), o.ptt.as_str()), ("fn⌃", "fn"));
    assert!(o.warning().contains("overlaps"), "{}", o.warning());

    let o = overlap_with_ptt(&fn_only, &fn_ctrl).expect("fn under a fn⌃ PTT must warn");
    assert_eq!(o.kind, OverlapKind::InsidePtt);

    // Left/right does not hide an overlap.
    let left_ctrl = [m(Modifier::Fn), left(Modifier::Control)];
    assert_eq!(
        overlap_with_ptt(&left_ctrl, &fn_ctrl).map(|o| o.kind),
        Some(OverlapKind::Same)
    );

    // A Carbon chord shares no keys with the fn hold: no warning.
    let meeting = [m(Modifier::Control), m(Modifier::Command), key(Code::KeyM)];
    assert_eq!(overlap_with_ptt(&meeting, &fn_ctrl), None);

    // The settings the user actually picks: the default and "fn" arm one hold
    // key and warn of nothing; "both" arms fn AND fn⌃, which is the pair.
    assert_eq!(ptt_chords("fn_control"), vec![fn_ctrl.to_vec()]);
    assert_eq!(ptt_self_overlap("fn_control"), None);
    assert_eq!(ptt_self_overlap("fn"), None);
    let w = ptt_self_overlap("both").expect("fn + fn⌃ must warn");
    assert!(w.contains("fn⌃") && w.contains("fn"), "{w}");
}

fn chord(code: &str, ctrl: bool, alt: bool, shift: bool, cmd: bool) -> ChordSetting {
    ChordSetting {
        code: code.into(),
        ctrl,
        alt,
        shift,
        cmd,
    }
}

/// The rules are LIVE: the real rebind path refuses a four-key chord and a
/// bare key for every Y8-A action, accepts each default, and still lets the
/// Cancel key be a bare Esc.
#[test]
fn the_rebind_path_applies_the_rules_to_every_new_action() {
    for a in [
        BindingAction::HandsFreeToggle,
        BindingAction::CopyLast,
        BindingAction::OpenScratchpad,
        BindingAction::RecordMeeting,
    ] {
        let four = chord("KeyK", true, true, false, true);
        let err = chord_for_action(a, &four).expect_err("4 keys refused");
        assert!(err.contains("at most 3"), "{a:?}: {err}");
        let bare = chord("KeyK", false, false, false, false);
        assert!(
            chord_for_action(a, &bare).is_err(),
            "{a:?}: bare key accepted"
        );
        let shift_only = chord("KeyK", false, false, true, false);
        assert!(
            chord_for_action(a, &shift_only).is_err(),
            "{a:?}: ⇧K accepted"
        );
        // A stored 4-key chord is re-validated at launch and named on its row.
        let why = stored_chord_refusal(a, Some(&four)).expect("refusal note");
        assert!(why.contains("at most 3"), "{why}");
    }
    let ok = [
        (
            BindingAction::HandsFreeToggle,
            chord("KeyH", true, false, false, true),
            "⌃⌘H",
        ),
        (
            BindingAction::CopyLast,
            chord("KeyC", true, false, false, true),
            "⌃⌘C",
        ),
        (
            BindingAction::OpenScratchpad,
            chord("KeyJ", true, false, false, true),
            "⌃⌘J",
        ),
    ];
    for (a, c, label) in ok {
        assert_eq!(chord_for_action(a, &c).expect("default accepted").1, label);
    }
    // Cancel keeps its one exemption: a bare Esc.
    assert!(chord_for_action(
        BindingAction::Cancel,
        &chord("Escape", false, false, false, false)
    )
    .is_ok());
}
