//! WF-18 — the Shortcuts editor that cannot conflict, driven without an app.
//!
//! Acceptance: unit tests on the reserved-combo list and the overlap check,
//! and every action keeps at least one binding (no delete on the last one).
//! Everything here calls `shortcuts::editor`, the same functions the Settings
//! save path and the launch registration call.

use wilson_voice_lib::hotkey_table::{self, check_distinct, BindingAction, RESERVED_CHORDS};
use wilson_voice_lib::hotkeys::ChordSetting;
use wilson_voice_lib::shortcuts::editor::{
    self, bindings_of, check_add, default_chord, hands_free_line, remove_at, reserved_combo,
    reset_to_default, split, validate_extras, ExtraBinding, Verdict, EDITABLE,
    LAST_BINDING_REFUSAL, MAX_BINDINGS_PER_ACTION,
};

fn chord(code: &str, ctrl: bool, alt: bool, shift: bool, cmd: bool) -> ChordSetting {
    ChordSetting {
        code: code.into(),
        ctrl,
        alt,
        cmd,
        shift,
    }
}

fn cmd(code: &str) -> ChordSetting {
    chord(code, false, false, false, true)
}

/// Every editable action with its default bindings plus `extras`.
fn table(extras: &[ExtraBinding]) -> Vec<(BindingAction, Vec<ChordSetting>)> {
    EDITABLE
        .iter()
        .map(|a| (*a, bindings_of(*a, None, extras)))
        .collect()
}

// ── the reserved-combo list ─────────────────────────────────────────────────

#[test]
fn reserved_combo_list_names_the_macos_function() {
    assert_eq!(reserved_combo(&cmd("KeyC")), Some("Copy"));
    assert_eq!(reserved_combo(&cmd("KeyV")), Some("Paste"));
    assert_eq!(reserved_combo(&cmd("Tab")), Some("the app switcher"));
    assert_eq!(reserved_combo(&cmd("KeyQ")), Some("Quit"));
    assert_eq!(reserved_combo(&cmd("Space")), Some("Spotlight"));
    assert_eq!(
        reserved_combo(&chord("KeyQ", true, false, false, true)),
        Some("Lock Screen")
    );
    // Yap's own family is not reserved.
    assert_eq!(
        reserved_combo(&chord("KeyM", true, false, false, true)),
        None
    );
    assert_eq!(
        reserved_combo(&chord("KeyR", true, true, false, false)),
        None
    );
}

#[test]
fn reserved_combo_list_covers_the_wispr_named_combos() {
    for (c, what) in [
        (cmd("KeyC"), "Copy"),
        (cmd("KeyV"), "Paste"),
        (cmd("KeyX"), "Cut"),
        (cmd("KeyZ"), "Undo"),
        (cmd("KeyA"), "Select All"),
        (cmd("KeyQ"), "Quit"),
        (cmd("KeyW"), "Close Window"),
        (cmd("Tab"), "the app switcher"),
    ] {
        assert_eq!(
            reserved_combo(&c),
            Some(what),
            "{} must be reserved",
            c.code
        );
    }
}

#[test]
fn every_reserved_combo_is_refused_as_a_new_binding() {
    let t = table(&[]);
    for (ctrl, alt, shift, cmd_, code, what) in RESERVED_CHORDS {
        let c = chord(code, *ctrl, *alt, *shift, *cmd_);
        assert_eq!(reserved_combo(&c), Some(*what));
        for a in BindingAction::ALWAYS_GLOBAL_REBINDABLE {
            let current = bindings_of(a, None, &[]);
            match check_add(a, &c, &current, &t) {
                Verdict::Refused(why) => assert!(!why.is_empty()),
                other => panic!("{code} ({what}) on {a:?} gave {other:?}"),
            }
        }
    }
}

#[test]
fn reserved_combo_refusal_names_what_macos_uses_it_for() {
    let a = BindingAction::RecordMeeting;
    let current = bindings_of(a, None, &[]);
    match check_add(a, &cmd("KeyC"), &current, &table(&[])) {
        Verdict::Refused(why) => assert!(why.contains("Copy"), "{why}"),
        other => panic!("⌘C gave {other:?}"),
    }
}

// ── the overlap check ───────────────────────────────────────────────────────

#[test]
fn overlap_with_another_actions_primary_warns() {
    let a = BindingAction::RecordMeeting;
    let current = bindings_of(a, None, &[]);
    // ⌃⌘H is the Hands-free (toggle) default.
    let hands_free = chord("KeyH", true, false, false, true);
    match check_add(a, &hands_free, &current, &table(&[])) {
        Verdict::Overlaps(w) => {
            assert!(w.contains("⌃⌘H"), "{w}");
            assert!(w.contains("Hands-free (toggle)"), "{w}");
        }
        other => panic!("⌃⌘H on Record meeting gave {other:?}"),
    }
}

#[test]
fn overlap_with_another_actions_extra_warns() {
    let extra = chord("KeyK", true, true, false, false);
    let extras = vec![ExtraBinding {
        action: BindingAction::CopyLast,
        chord: extra.clone(),
    }];
    let a = BindingAction::OpenScratchpad;
    let current = bindings_of(a, None, &extras);
    match check_add(a, &extra, &current, &table(&extras)) {
        Verdict::Overlaps(w) => assert!(w.contains("Copy last transcript"), "{w}"),
        other => panic!("{other:?}"),
    }
    // The overlap names the right action and nothing else.
    assert_eq!(
        editor::overlapping_action(a, &extra, &table(&extras)),
        Some(BindingAction::CopyLast)
    );
}

#[test]
fn overlap_with_the_dictate_toggle_and_cancel_warns() {
    let t = table(&[]);
    let a = BindingAction::CopyLast;
    let current = bindings_of(a, None, &[]);
    assert_eq!(
        editor::overlapping_action(a, &default_chord(BindingAction::Cancel).unwrap(), &t),
        Some(BindingAction::Cancel)
    );
    assert_eq!(
        editor::overlapping_action(a, &default_chord(BindingAction::DictateToggle).unwrap(), &t),
        Some(BindingAction::DictateToggle)
    );
    // A free chord does not overlap and may be added.
    let free = chord("KeyR", true, true, false, false);
    assert_eq!(editor::overlapping_action(a, &free, &t), None);
    assert_eq!(check_add(a, &free, &current, &t), Verdict::Ok);
}

#[test]
fn a_fixed_yap_chord_is_refused_not_just_warned() {
    let a = BindingAction::HandsFreeToggle;
    let current = bindings_of(a, None, &[]);
    // ⌃⌘V is paste-last — not rebindable, so it is a refusal.
    let paste_last = chord("KeyV", true, false, false, true);
    assert!(matches!(
        check_add(a, &paste_last, &current, &table(&[])),
        Verdict::Refused(_)
    ));
}

#[test]
fn save_path_refuses_an_extra_another_action_holds() {
    // validate_extras + check_distinct is the save_settings gate.
    let extras = vec![ExtraBinding {
        action: BindingAction::RecordMeeting,
        chord: chord("KeyJ", true, false, false, true), // Open Scratchpad's default
    }];
    let primaries: Vec<(BindingAction, Option<&ChordSetting>)> =
        BindingAction::ALWAYS_GLOBAL_REBINDABLE
            .iter()
            .map(|a| (*a, None))
            .collect();
    let mut all: Vec<_> = BindingAction::ALWAYS_GLOBAL_REBINDABLE
        .iter()
        .map(|a| {
            let (sc, l) = hotkey_table::resolve(*a, None);
            (*a, sc, l)
        })
        .collect();
    all.extend(validate_extras(&primaries, &extras).expect("⌃⌘J passes the rule gate"));
    let err = check_distinct(&all).expect_err("two actions on ⌃⌘J");
    assert!(
        err.contains("Open Scratchpad") && err.contains("Record meeting"),
        "{err}"
    );
}

#[test]
fn save_path_refuses_reserved_duplicate_and_over_cap_extras() {
    let primaries: Vec<(BindingAction, Option<&ChordSetting>)> =
        BindingAction::ALWAYS_GLOBAL_REBINDABLE
            .iter()
            .map(|a| (*a, None))
            .collect();
    let x = |c: ChordSetting| ExtraBinding {
        action: BindingAction::RecordMeeting,
        chord: c,
    };
    assert!(validate_extras(&primaries, &[x(cmd("KeyV"))]).is_err());
    let dup = chord("KeyR", true, true, false, false);
    assert!(validate_extras(&primaries, &[x(dup.clone()), x(dup)]).is_err());
    let three = [
        x(chord("KeyR", true, true, false, false)),
        x(chord("KeyT", true, true, false, false)),
        x(chord("KeyY", true, true, false, false)),
    ];
    assert_eq!(validate_extras(&primaries, &three).map(|v| v.len()), Ok(3));
    let mut four = three.to_vec();
    four.push(x(chord("KeyU", true, true, false, false)));
    assert!(validate_extras(&primaries, &four).is_err(), "a 5th binding");
    // Cancel takes no extras.
    let cancel_extra = ExtraBinding {
        action: BindingAction::Cancel,
        chord: chord("F13", false, false, false, false),
    };
    assert!(validate_extras(&primaries, &[cancel_extra]).is_err());
}

#[test]
fn launch_drops_a_bad_stored_extra_and_keeps_the_rest() {
    let primaries: Vec<(BindingAction, Option<&ChordSetting>)> =
        BindingAction::ALWAYS_GLOBAL_REBINDABLE
            .iter()
            .map(|a| (*a, None))
            .collect();
    let extras = vec![
        ExtraBinding {
            action: BindingAction::CopyLast,
            chord: cmd("KeyC"), // reserved: Copy
        },
        ExtraBinding {
            action: BindingAction::CopyLast,
            chord: chord("KeyR", true, true, false, false),
        },
    ];
    let kept = editor::resolve_extras_lenient(&primaries, &extras);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].0, BindingAction::CopyLast);
    assert_eq!(kept[0].2, "⌃⌥R");
}

// ── several bindings per action, and never zero ─────────────────────────────

#[test]
fn an_action_takes_up_to_four_bindings() {
    let a = BindingAction::RecordMeeting;
    let mut extras: Vec<ExtraBinding> = Vec::new();
    for code in ["KeyR", "KeyT", "KeyY"] {
        let c = chord(code, true, true, false, false);
        let current = bindings_of(a, None, &extras);
        assert_eq!(check_add(a, &c, &current, &table(&extras)), Verdict::Ok);
        extras.push(ExtraBinding {
            action: a,
            chord: c,
        });
    }
    let current = bindings_of(a, None, &extras);
    assert_eq!(current.len(), MAX_BINDINGS_PER_ACTION);
    match check_add(
        a,
        &chord("KeyU", true, true, false, false),
        &current,
        &table(&extras),
    ) {
        Verdict::Refused(why) => assert!(why.contains("4 shortcuts"), "{why}"),
        other => panic!("a 5th gave {other:?}"),
    }
}

#[test]
fn single_binding_actions_refuse_a_second() {
    for a in [BindingAction::DictateToggle, BindingAction::Cancel] {
        let current = bindings_of(a, None, &[]);
        assert_eq!(current.len(), 1);
        assert!(matches!(
            check_add(
                a,
                &chord("KeyR", true, true, false, false),
                &current,
                &table(&[])
            ),
            Verdict::Refused(_)
        ));
    }
}

#[test]
fn a_duplicate_inside_one_action_is_refused() {
    let a = BindingAction::RecordMeeting;
    let current = bindings_of(a, None, &[]);
    let own = default_chord(a).unwrap();
    assert!(matches!(
        check_add(a, &own, &current, &table(&[])),
        Verdict::Refused(_)
    ));
}

#[test]
fn the_last_binding_cannot_be_removed() {
    for a in EDITABLE {
        let only = bindings_of(a, None, &[]);
        assert_eq!(only.len(), 1, "{a:?} starts with one binding");
        assert_eq!(remove_at(&only, 0), Err(LAST_BINDING_REFUSAL.to_string()));
    }
}

#[test]
fn removing_one_of_several_keeps_the_rest() {
    let a = BindingAction::HandsFreeToggle;
    let extra = chord("KeyR", true, true, false, false);
    let list = bindings_of(
        a,
        None,
        &[ExtraBinding {
            action: a,
            chord: extra.clone(),
        }],
    );
    assert_eq!(list.len(), 2);
    assert_eq!(remove_at(&list, 0), Ok(vec![extra.clone()]));
    assert_eq!(remove_at(&list, 1), Ok(vec![default_chord(a).unwrap()]));
    assert!(remove_at(&list, 2).is_err());
    // and the remaining one is then the last
    assert_eq!(
        remove_at(&[extra], 0),
        Err(LAST_BINDING_REFUSAL.to_string())
    );
}

#[test]
fn every_action_keeps_at_least_one_binding() {
    for a in EDITABLE {
        assert!(!bindings_of(a, None, &[]).is_empty(), "{a:?}");
        let reset = reset_to_default(a);
        assert_eq!(reset.len(), 1, "{a:?}");
        assert_eq!(Some(reset[0].clone()), default_chord(a));
    }
}

#[test]
fn reset_to_default_and_split_round_trip() {
    let a = BindingAction::CopyLast;
    // Reset stores as "no primary, no extras" — the shipped chord.
    assert_eq!(split(a, &reset_to_default(a)), (None, Vec::new()));
    let custom = chord("KeyK", true, true, false, false);
    let extra = chord("KeyR", true, true, false, false);
    let (p, e) = split(a, &[custom.clone(), extra.clone()]);
    assert_eq!(p, Some(custom.clone()));
    assert_eq!(
        e,
        vec![ExtraBinding {
            action: a,
            chord: extra.clone()
        }]
    );
    assert_eq!(bindings_of(a, p.as_ref(), &e), vec![custom, extra]);
}

#[test]
fn default_chords_match_the_shortcut_table() {
    assert_eq!(
        default_chord(BindingAction::RecordMeeting),
        Some(chord("KeyM", true, false, false, true))
    );
    assert_eq!(
        default_chord(BindingAction::Cancel),
        Some(chord("Escape", false, false, false, false))
    );
    assert_eq!(
        default_chord(BindingAction::DictateToggle),
        Some(chord("Space", true, true, false, true))
    );
    assert_eq!(default_chord(BindingAction::DictateHold), None);
}

// ── the live hands-free line ────────────────────────────────────────────────

#[test]
fn hands_free_line_names_the_live_hold_key() {
    assert_eq!(hands_free_line("fn"), "Double-tap fn for hands-free");
    assert_eq!(
        hands_free_line("fn_control"),
        "Double-tap fn⌃ for hands-free"
    );
    assert_eq!(
        hands_free_line("both"),
        "Double-tap fn or fn⌃ for hands-free"
    );
}

// ── runtime: extra chords route to their action ─────────────────────────────

#[test]
fn alias_diff_registers_new_and_drops_removed() {
    let (a, _) = hotkey_table::resolve(BindingAction::RecordMeeting, None);
    let (b, _) = hotkey_table::resolve(BindingAction::CopyLast, None);
    let old = vec![(BindingAction::RecordMeeting, a, "⌃⌘M".to_string())];
    let new = vec![(BindingAction::CopyLast, b, "⌃⌘C".to_string())];
    let (reg, unreg) = hotkey_table::alias_diff(&old, &new);
    assert_eq!(reg, vec![b]);
    assert_eq!(unreg, vec![a]);
    let (reg, unreg) = hotkey_table::alias_diff(&new, &new);
    assert!(reg.is_empty() && unreg.is_empty());
}

#[test]
fn an_extra_chord_routes_to_its_action() {
    let primaries: Vec<(BindingAction, Option<&ChordSetting>)> =
        BindingAction::ALWAYS_GLOBAL_REBINDABLE
            .iter()
            .map(|a| (*a, None))
            .collect();
    let extras = vec![ExtraBinding {
        action: BindingAction::OpenScratchpad,
        chord: chord("KeyK", true, true, false, false),
    }];
    let resolved = validate_extras(&primaries, &extras).unwrap();
    let sc = resolved[0].1;
    hotkey_table::set_active_aliases(resolved);
    assert_eq!(
        hotkey_table::alias_action(&sc),
        Some(BindingAction::OpenScratchpad)
    );
    hotkey_table::set_active_aliases(Vec::new());
    assert_eq!(hotkey_table::alias_action(&sc), None);
}
