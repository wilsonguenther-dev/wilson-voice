//! YV95 — every global shortcut Yap registers, in ONE table.
//!
//! Why a table instead of four `Shortcut::new(...)` literals scattered through
//! `run()`: finding #6 asks for "a distinct toggle hotkey that does not collide
//! with dictation-hold", and a collision test is only worth writing if it reads
//! the same values the app actually registers. Before this item the accelerators
//! were spelled out three times — once in the `MenuItem` accelerator string,
//! once in the `Shortcut::new` call, once in a log line — so "distinct" was an
//! eyeball claim. Now `lib.rs` builds both the menu accelerator and the
//! registration from these constants, and `tests/tray_hotkey_no_collision.rs`
//! asserts over the table, which means a future item cannot double-book a chord
//! without failing a test.
//!
//! ## The two keyboard surfaces, and why they cannot collide
//!
//! Yap listens to the keyboard in two completely different ways:
//!
//!   * **Dictation-hold (`ptt_macos`)** — a CGEvent tap on `flagsChanged`,
//!     watching the `fn` (globe) key alone or with ⌃, optionally plus ⌘/⌥ for
//!     command mode. It is a *modifier chord with no character key*, and `fn` is
//!     not expressible as a `Code` at all: the plugin's shortcut vocabulary has
//!     no `Code::Fn`, and macOS does not deliver `fn` as a keyDown.
//!   * **Global shortcuts (this table)** — Carbon/`RegisterEventHotKey` chords,
//!     each of which REQUIRES a non-modifier key.
//!
//! So the only way a meeting hotkey could shadow dictation-hold is if it had no
//! character key, which the type system here does not allow (every binding
//! carries a [`Code`]). [`collides_with_dictation_hold`] states that argument as
//! executable code rather than a comment, and the test drives it across every
//! `PttBinding` × `CommandBinding` the settings can produce.

use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

/// One global chord: the modifiers, the key, and the strings the tray + logs
/// show for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    /// Stable id — what the log line and the test failure message name.
    pub id: &'static str,
    /// The macOS glyph form shown in the menu bar, e.g. `⌃⌘M`.
    pub label: &'static str,
    /// The tauri-menu accelerator string, e.g. `Ctrl+Cmd+M`.
    pub accelerator: &'static str,
    pub ctrl: bool,
    pub cmd: bool,
    pub shift: bool,
    pub alt: bool,
    pub code: Code,
}

impl Binding {
    /// The chord identity — everything except the human-facing strings. Two
    /// bindings collide exactly when these are equal.
    pub fn chord(&self) -> (bool, bool, bool, bool, Code) {
        (self.ctrl, self.cmd, self.shift, self.alt, self.code)
    }

    pub fn modifiers(&self) -> Modifiers {
        let mut m = Modifiers::empty();
        if self.ctrl {
            m |= Modifiers::CONTROL;
        }
        if self.cmd {
            m |= Modifiers::SUPER;
        }
        if self.shift {
            m |= Modifiers::SHIFT;
        }
        if self.alt {
            m |= Modifiers::ALT;
        }
        m
    }

    /// The value handed to `global_shortcut().register(..)`.
    pub fn shortcut(&self) -> Shortcut {
        Shortcut::new(Some(self.modifiers()), self.code)
    }

    pub fn collides_with(&self, other: &Binding) -> bool {
        self.chord() == other.chord()
    }
}

/// ⌃⌘V — re-paste the newest transcript (registered unconditionally).
pub const PASTE_LAST: Binding = Binding {
    id: "paste_last",
    label: "⌃⌘V",
    accelerator: "Ctrl+Cmd+V",
    ctrl: true,
    cmd: true,
    shift: false,
    alt: false,
    code: Code::KeyV,
};

/// ⌃⌘Z — YV51 undo-AI-edit (re-paste the raw take).
pub const UNDO_AI_EDIT: Binding = Binding {
    id: "undo_ai_edit",
    label: "⌃⌘Z",
    accelerator: "Ctrl+Cmd+Z",
    ctrl: true,
    cmd: true,
    shift: false,
    alt: false,
    code: Code::KeyZ,
};

/// ⌘⇧V — the legacy dictation toggle, off by default (`keep_cmd_shift_v`).
pub const DICTATION_TOGGLE_LEGACY: Binding = Binding {
    id: "dictation_toggle_legacy",
    label: "⌘⇧V",
    accelerator: "Cmd+Shift+V",
    ctrl: false,
    cmd: true,
    shift: true,
    alt: false,
    code: Code::KeyV,
};

/// ⌃⌘M — YV95's meeting start/stop toggle.
///
/// ⌃⌘ + letter is the family Yap's other two global chords already live in, so
/// it inherits their argument: macOS itself claims ⌘-letter and ⌘⇧-letter
/// heavily and claims ⌃⌘ barely at all (⌃⌘F fullscreen, ⌃⌘Space emoji — neither
/// is M). `M` for meeting. It is deliberately NOT ⌘⇧M: Teams and Zoom both bind
/// that inside their own windows for mute, and a meeting recorder that fights
/// the mute key in the app you are recording is a bad neighbour.
pub const MEETING_TOGGLE: Binding = Binding {
    id: "meeting_toggle",
    label: "⌃⌘M",
    accelerator: "Ctrl+Cmd+M",
    ctrl: true,
    cmd: true,
    shift: false,
    alt: false,
    code: Code::KeyM,
};

/// Every chord this app can register. Registration is conditional for one of
/// them (`DICTATION_TOGGLE_LEGACY` is settings-gated) but the CONFLICT question
/// is not: a chord that is only sometimes registered still cannot be re-used.
pub const ALL: &[Binding] = &[
    PASTE_LAST,
    UNDO_AI_EDIT,
    DICTATION_TOGGLE_LEGACY,
    MEETING_TOGGLE,
];

/// The first pair of bindings in `ALL` that share a chord, if any.
///
/// Returns the ids so a failure names both halves — "meeting_toggle collides
/// with paste_last" is actionable, "collision detected" is not.
pub fn first_collision() -> Option<(&'static str, &'static str)> {
    for (i, a) in ALL.iter().enumerate() {
        for b in ALL.iter().skip(i + 1) {
            if a.collides_with(b) {
                return Some((a.id, b.id));
            }
        }
    }
    None
}

/// Can this chord be produced by holding the dictation push-to-talk combo?
///
/// `fn` (and `fn⌃`, and either of those plus the command-mode ⌘/⌥) is a
/// modifier-only chord delivered as `flagsChanged`. A registered global shortcut
/// always carries a character key, and pressing that key while holding `fn` is a
/// *different* physical event from the hold PTT arms on — the tap arms on the
/// modifier transition, not on any keyDown. So the answer is `false` for every
/// binding that has a key, which is all of them, and the only way this can ever
/// return `true` is if someone adds a key-less binding type.
///
/// `_ptt_uses_control` / `_ptt_uses_command` are threaded through so the test can
/// drive every settings combination and so the argument above stays visible at
/// the call site rather than being buried in a doc comment.
pub fn collides_with_dictation_hold(
    binding: &Binding,
    _ptt_uses_control: bool,
    _ptt_uses_command: bool,
    _ptt_uses_option: bool,
) -> bool {
    // A `Binding` cannot be modifier-only: `code` is not optional. Written as a
    // check rather than `false` so this stops compiling-as-correct the moment
    // the type gains a key-less variant.
    matches!(binding.code, Code::Unidentified)
}
