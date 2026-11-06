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
/// WHEN a binding is live system-wide (Y3-D).
///
/// Before Y3-D every chord in this table was registered once at launch and held
/// until quit, so "the table" and "what Yap has taken off the system" were the
/// same list. The cancel key breaks that: its default is a bare `Escape`, and a
/// bare Escape held globally for the life of the process would swallow Escape
/// from every other app on the Mac — closing no dialog, dismissing no sheet,
/// leaving no vim. So scope is declared here, in the same table as the chord,
/// rather than being an implicit property of which `register` call site a
/// binding happens to appear at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Registered at launch and held until quit. Every chord in this family
    /// carries ⌃⌘ or ⌘⇧, so it cannot shadow a bare key another app wants.
    AlwaysGlobal,
    /// Registered only while a take is in flight and unregistered the moment it
    /// settles. This is what makes a bare `Escape` admissible: outside a take
    /// the key is not Yap's, and [`registered_while_idle`] is the executable
    /// form of that claim.
    WhileTakeActive,
}

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
    /// When this chord is live system-wide. See [`Scope`].
    pub scope: Scope,
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

/// ⌃⌥⌘Space — yap-p0-hotkeys-no-permission: THE dictation chord that works
/// with ZERO TCC grants. A Carbon `RegisterEventHotKey` chord needs neither
/// Accessibility nor Input Monitoring and is delivered under Secure Input, so it
/// is registered UNCONDITIONALLY at launch and is the primary way to start and
/// stop a take; the fn / fn⌃ tap is the extra. Press toggles; a press held past
/// `hotkeys::CARBON_HOLD_MS` then released is hold-to-talk.
///
/// Chosen after `defaults read com.apple.symbolichotkeys AppleSymbolicHotKeys`
/// on the dev Mac (2026-10-17): the Space-key entries there are 60 (⌃Space),
/// 61 (⌃⌥Space), 64 (⌘Space) and 65 (⌘⌥Space) — none is ⌃⌥⌘Space, and
/// ⌃⌘Space (the emoji viewer) differs by ⌥. Rebindable in Settings.
pub const DICTATE_TOGGLE: Binding = Binding {
    id: "dictate_toggle",
    label: "⌃⌥⌘Space",
    accelerator: "Ctrl+Cmd+Alt+Space",
    ctrl: true,
    cmd: true,
    shift: false,
    alt: true,
    code: Code::Space,
    scope: Scope::AlwaysGlobal,
};

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
    scope: Scope::AlwaysGlobal,
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
    scope: Scope::AlwaysGlobal,
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
    scope: Scope::AlwaysGlobal,
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
    scope: Scope::AlwaysGlobal,
};

/// Escape — Y3-D's cancel key: abandon the take that is running RIGHT NOW,
/// whether it is still recording or already decoding.
///
/// **Why a bare Escape is safe here and would not be anywhere else.** Escape is
/// the single most contested key on macOS: it closes sheets, dismisses menus,
/// leaves full screen and leaves insert mode. A `RegisterEventHotKey` on it is
/// system-wide and exclusive, so registering it at launch would break Escape in
/// every app for as long as Yap runs — an unshippable regression, and the
/// reason this binding carries [`Scope::WhileTakeActive`] instead of
/// [`Scope::AlwaysGlobal`].
///
/// It is registered when a take arms and unregistered the moment that take
/// settles (including the failure and cancel exits), so the window in which Yap
/// owns Escape is exactly the window in which the user has something to cancel
/// — during which Escape meaning "stop this" is what every other app trained
/// them to expect. `tests/cancel_long_take.rs` asserts the idle half of that
/// (`escape_is_not_a_global_shortcut_while_idle`) against
/// [`registered_while_idle`], which is the same list `lib.rs` registers from.
///
/// The alternative considered and rejected was scoping it to the pill's own
/// window with a JS keydown listener. The pill is a non-activating NSPanel: it
/// never takes key focus, so it never receives a keydown, so an Escape pressed
/// while the user is looking at their editor — which is where they are during
/// every take — would reach nothing at all.
///
/// Rebindable through the YV15 capture control like the others; `code`/`label`/
/// `accelerator` are what that control writes back.
pub const CANCEL: Binding = Binding {
    id: "cancel_take",
    label: "esc",
    accelerator: "Escape",
    ctrl: false,
    cmd: false,
    shift: false,
    alt: false,
    code: Code::Escape,
    scope: Scope::WhileTakeActive,
};

/// ⌃⌘H — Y8-A hands-free toggle: start a take already latched hands-free
/// (keys up, still recording), or finish the hands-free take in flight. The
/// Carbon twin of the tray's Hands-free item and of the double-tap on the fn
/// hold — and, unlike the double-tap, it needs no Input Monitoring grant.
/// Rebindable in Settings → Hotkeys (`hands_free_chord`).
pub const HANDS_FREE: Binding = Binding {
    id: "hands_free_toggle",
    label: "⌃⌘H",
    accelerator: "Ctrl+Cmd+H",
    ctrl: true,
    cmd: true,
    shift: false,
    alt: false,
    code: Code::KeyH,
    scope: Scope::AlwaysGlobal,
};

/// ⌃⌘C — Y8-A copy the newest transcript to the clipboard WITHOUT pasting it:
/// the half of ⌃⌘V for when the cursor is not where the text should go yet.
/// Same family as ⌃⌘V (C beside V). Rebindable (`copy_last_chord`).
pub const COPY_LAST: Binding = Binding {
    id: "copy_last",
    label: "⌃⌘C",
    accelerator: "Ctrl+Cmd+C",
    ctrl: true,
    cmd: true,
    shift: false,
    alt: false,
    code: Code::KeyC,
    scope: Scope::AlwaysGlobal,
};

/// ⌃⌘J — Y8-A bring Yap forward on the Scratchpad. J ("jot"), not S: Finder
/// binds ⌃⌘S to Hide Sidebar, and a system-wide Carbon chord would take it
/// from Finder. Rebindable (`scratchpad_chord`).
///
/// Per-transform shortcuts are deliberately NOT in this table: the transform
/// library is Y9-A, and a shortcut for a transform that does not exist yet is
/// dead config. They join this table, and [`validate`], when it lands.
pub const OPEN_SCRATCHPAD: Binding = Binding {
    id: "open_scratchpad",
    label: "⌃⌘J",
    accelerator: "Ctrl+Cmd+J",
    ctrl: true,
    cmd: true,
    shift: false,
    alt: false,
    code: Code::KeyJ,
    scope: Scope::AlwaysGlobal,
};

/// Every chord this app can register. Registration is conditional for one of
/// them (`DICTATION_TOGGLE_LEGACY` is settings-gated) but the CONFLICT question
/// is not: a chord that is only sometimes registered still cannot be re-used.
pub const ALL: &[Binding] = &[
    DICTATE_TOGGLE,
    PASTE_LAST,
    UNDO_AI_EDIT,
    DICTATION_TOGGLE_LEGACY,
    MEETING_TOGGLE,
    CANCEL,
    HANDS_FREE,
    COPY_LAST,
    OPEN_SCRATCHPAD,
];

/// Every chord Yap holds system-wide while NOTHING is being dictated.
///
/// The executable form of [`CANCEL`]'s argument: a binding scoped to a take is
/// not in this list, so a test can assert that Yap does not own Escape while
/// idle by reading the same table `lib.rs` registers from — rather than by
/// eyeballing the register call sites, which is how a bare key sneaks into a
/// launch-time registration and stays there.
pub fn registered_while_idle() -> Vec<&'static Binding> {
    ALL.iter()
        .filter(|b| matches!(b.scope, Scope::AlwaysGlobal))
        .collect()
}

/// The chords registered only for the duration of a take.
pub fn registered_while_take_active() -> Vec<&'static Binding> {
    ALL.iter()
        .filter(|b| matches!(b.scope, Scope::WhileTakeActive))
        .collect()
}

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

/// Y8-A — Wispr's four shortcut rules, as pure functions over a key list.
///
/// A chord here is the list of PHYSICAL keys held, which — unlike [`Binding`] —
/// can name `fn` and can tell left ⌘ from right ⌘. That is what lets the same
/// rules judge a Carbon chord (⌃⌘M) and the fn-tap hold (fn, fn⌃), which is
/// where the one overlap a Yap user will actually meet lives: `fn` vs `fn⌃`,
/// Yap's own default pair (`ptt_binding: "fn_control"`).
///
/// Each rule is its own function and its own test (`tests/shortcut_validation.rs`):
///
/// 1. [`at_most_three_keys`] — a chord is at most three keys.
/// 2. [`a_modifier_is_required`] — fn, ⌃, ⌥ or ⌘ is required. ⇧ alone does
///    not count: ⇧+letter is how a capital letter is typed, so a global ⇧A
///    breaks typing exactly like a bare A would.
/// 3. [`left_and_right_variants_are_one_key`] — left ⌘ + right ⌘ is not two
///    keys; it is one modifier pressed twice and is refused.
/// 4. [`overlap_with_ptt`] — a chord that contains, or is contained by, the
///    push-to-talk chord WARNS (it does not refuse): holding fn⌃ also holds fn.
///
/// Where they run: `hotkey_table::chord_for_action` applies rules 1–3 to every
/// Settings rebind of a Carbon action (Cancel is exempt from rule 2 — it is
/// registered only while a take runs, see [`CANCEL`]), and the hotkey table
/// raises rule 4 on the Dictate (hold) row when the configured hold keys
/// overlap. The dictate toggle's shipped default ⌃⌥⌘Space is four keys and is
/// NOT run through rule 1: it was picked to sit beside, not on, macOS's
/// ⌃⌘Space / ⌘⌥Space (see [`DICTATE_TOGGLE`]), and its rebind keeps its own
/// path (`hotkeys::chord_from_setting`).
pub mod validate {
    use tauri_plugin_global_shortcut::Code;

    /// Which physical copy of a modifier. `Either` is the settings-level
    /// "⌘", which matches both.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Side {
        Left,
        Right,
        Either,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Modifier {
        Fn,
        Control,
        Option,
        Shift,
        Command,
    }

    impl Modifier {
        pub fn glyph(self) -> &'static str {
            match self {
                Modifier::Fn => "fn",
                Modifier::Control => "⌃",
                Modifier::Option => "⌥",
                Modifier::Shift => "⇧",
                Modifier::Command => "⌘",
            }
        }
    }

    /// One physical key in a chord.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Key {
        Mod(Modifier, Side),
        Key(Code),
    }

    /// Shorthand constructors for the common shapes.
    pub fn m(modifier: Modifier) -> Key {
        Key::Mod(modifier, Side::Either)
    }
    pub fn left(modifier: Modifier) -> Key {
        Key::Mod(modifier, Side::Left)
    }
    pub fn right(modifier: Modifier) -> Key {
        Key::Mod(modifier, Side::Right)
    }
    pub fn key(code: Code) -> Key {
        Key::Key(code)
    }

    /// The most keys a chord may hold.
    pub const MAX_KEYS: usize = 3;

    /// Why a chord is refused.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Violation {
        TooManyKeys { label: String, count: usize },
        NoModifier { label: String },
        LeftRightDuplicate { label: String, modifier: Modifier },
        Empty,
    }

    impl std::fmt::Display for Violation {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Violation::TooManyKeys { label, count } => write!(
                    f,
                    "{label} is {count} keys — a shortcut is at most {MAX_KEYS}"
                ),
                Violation::NoModifier { label } => write!(
                    f,
                    "{label} needs fn, ⌃, ⌥ or ⌘ — without one it would swallow typing in every app"
                ),
                Violation::LeftRightDuplicate { label, modifier } => write!(
                    f,
                    "{label} holds left and right {} — they are one key, not two",
                    modifier.glyph()
                ),
                Violation::Empty => write!(f, "no keys pressed"),
            }
        }
    }

    /// How a chord overlaps the push-to-talk chord.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum OverlapKind {
        /// The same keys.
        Same,
        /// The candidate holds every PTT key plus more (fn⌃ over fn).
        ContainsPtt,
        /// The PTT chord holds every candidate key plus more (fn under fn⌃).
        InsidePtt,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Overlap {
        pub kind: OverlapKind,
        pub candidate: String,
        pub ptt: String,
    }

    impl Overlap {
        /// The warning sentence a row shows.
        pub fn warning(&self) -> String {
            match self.kind {
                OverlapKind::Same => format!(
                    "{} is the push-to-talk key itself — both would fire on one press",
                    self.candidate
                ),
                OverlapKind::ContainsPtt => format!(
                    "{} overlaps {}: holding {} also holds {}, so the {} hold arms first",
                    self.candidate, self.ptt, self.candidate, self.ptt, self.ptt
                ),
                OverlapKind::InsidePtt => format!(
                    "{} overlaps {}: every {} hold starts as a {} hold",
                    self.candidate, self.ptt, self.ptt, self.candidate
                ),
            }
        }
    }

    /// The glyph label of a key list: modifiers in ⌃⌥⇧⌘ order after fn, then
    /// the key. Left/right copies are shown once per copy so a duplicate is
    /// visible in its own error message.
    pub fn label(keys: &[Key]) -> String {
        let order = [
            Modifier::Fn,
            Modifier::Control,
            Modifier::Option,
            Modifier::Shift,
            Modifier::Command,
        ];
        let mut out = String::new();
        for want in order {
            for k in keys {
                if let Key::Mod(m, _) = k {
                    if *m == want {
                        out.push_str(m.glyph());
                    }
                }
            }
        }
        for k in keys {
            if let Key::Key(code) = k {
                let name = format!("{code:?}");
                let name = name
                    .strip_prefix("Key")
                    .or_else(|| name.strip_prefix("Digit"))
                    .unwrap_or(&name)
                    .to_string();
                out.push_str(&name);
            }
        }
        out
    }

    /// Rule 1 — at most [`MAX_KEYS`] keys.
    pub fn at_most_three_keys(keys: &[Key]) -> Result<(), Violation> {
        if keys.is_empty() {
            return Err(Violation::Empty);
        }
        if keys.len() > MAX_KEYS {
            return Err(Violation::TooManyKeys {
                label: label(keys),
                count: keys.len(),
            });
        }
        Ok(())
    }

    /// Rule 2 — fn, ⌃, ⌥ or ⌘ is required (⇧ alone is not a modifier for
    /// this purpose: ⇧+letter types a capital).
    pub fn a_modifier_is_required(keys: &[Key]) -> Result<(), Violation> {
        let has = keys.iter().any(|k| {
            matches!(
                k,
                Key::Mod(
                    Modifier::Fn | Modifier::Control | Modifier::Option | Modifier::Command,
                    _
                )
            )
        });
        if has {
            Ok(())
        } else {
            Err(Violation::NoModifier { label: label(keys) })
        }
    }

    /// Rule 3 — left and right copies of one modifier are one key: a chord
    /// that holds the same modifier twice is refused.
    pub fn left_and_right_variants_are_one_key(keys: &[Key]) -> Result<(), Violation> {
        for (i, a) in keys.iter().enumerate() {
            if let Key::Mod(ma, _) = a {
                if keys[i + 1..]
                    .iter()
                    .any(|b| matches!(b, Key::Mod(mb, _) if mb == ma))
                {
                    return Err(Violation::LeftRightDuplicate {
                        label: label(keys),
                        modifier: *ma,
                    });
                }
            }
        }
        Ok(())
    }

    /// A key with its side erased — what "the same key" means for overlap.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Norm {
        Mod(Modifier),
        Key(Code),
    }

    fn norm(keys: &[Key]) -> Vec<Norm> {
        let mut out: Vec<Norm> = Vec::new();
        for k in keys {
            let n = match k {
                Key::Mod(m, _) => Norm::Mod(*m),
                Key::Key(c) => Norm::Key(*c),
            };
            if !out.contains(&n) {
                out.push(n);
            }
        }
        out
    }

    /// Rule 4 — does `candidate` overlap the push-to-talk chord `ptt`? Same
    /// keys, or one a strict superset of the other, ignoring left/right.
    /// `None` when they share no containment (⌃⌘M vs fn).
    pub fn overlap_with_ptt(candidate: &[Key], ptt: &[Key]) -> Option<Overlap> {
        let (c, p) = (norm(candidate), norm(ptt));
        if c.is_empty() || p.is_empty() {
            return None;
        }
        let c_in_p = c.iter().all(|k| p.contains(k));
        let p_in_c = p.iter().all(|k| c.contains(k));
        let kind = match (c_in_p, p_in_c) {
            (true, true) => OverlapKind::Same,
            (false, true) => OverlapKind::ContainsPtt,
            (true, false) => OverlapKind::InsidePtt,
            (false, false) => return None,
        };
        Some(Overlap {
            kind,
            candidate: label(candidate),
            ptt: label(ptt),
        })
    }

    /// The hold chords a `ptt_binding` setting arms (the fn tap): "fn" → fn,
    /// "fn_control" → fn⌃, "both" / "fn_or_fn_control" → fn and fn⌃. Unknown
    /// values read as the shipped default, fn⌃.
    pub fn ptt_chords(ptt_binding: &str) -> Vec<Vec<Key>> {
        let fn_only = vec![m(Modifier::Fn)];
        let fn_ctrl = vec![m(Modifier::Fn), m(Modifier::Control)];
        match ptt_binding {
            "fn" => vec![fn_only],
            "both" | "fn_or_fn_control" => vec![fn_only, fn_ctrl],
            _ => vec![fn_ctrl],
        }
    }

    /// The overlap warning for the hold keys a `ptt_binding` arms against
    /// EACH OTHER — "both" arms fn and fn⌃, which is exactly the pair rule 4
    /// exists for. `None` for a single hold key.
    pub fn ptt_self_overlap(ptt_binding: &str) -> Option<String> {
        let chords = ptt_chords(ptt_binding);
        for (i, a) in chords.iter().enumerate() {
            for b in &chords[i + 1..] {
                if let Some(o) = overlap_with_ptt(b, a) {
                    return Some(o.warning());
                }
            }
        }
        None
    }

    /// The key list of a Carbon chord (settings-level modifiers, no sides).
    pub fn keys_of_chord(ctrl: bool, alt: bool, shift: bool, cmd: bool, code: Code) -> Vec<Key> {
        let mut keys = Vec::new();
        if ctrl {
            keys.push(m(Modifier::Control));
        }
        if alt {
            keys.push(m(Modifier::Option));
        }
        if shift {
            keys.push(m(Modifier::Shift));
        }
        if cmd {
            keys.push(m(Modifier::Command));
        }
        keys.push(key(code));
        keys
    }

    /// Rules 1–3 for a Carbon rebind, first refusal wins. `allow_bare` skips
    /// rule 2 (the Cancel key, live only during a take).
    pub fn refuse(keys: &[Key], allow_bare: bool) -> Result<(), Violation> {
        at_most_three_keys(keys)?;
        if !allow_bare {
            a_modifier_is_required(keys)?;
        }
        left_and_right_variants_are_one_key(keys)?;
        Ok(())
    }
}

/// WF-18 — the Keyboard Shortcuts editor that cannot conflict, as pure
/// functions the Settings screen, `save_settings` and the launch registration
/// all call (and `tests/shortcuts_editor.rs` drives without an app).
///
/// Wispr lets one action carry up to four shortcuts, warns when a new one
/// overlaps another action, and has Reset to default. Yap's version:
///
/// * **Several bindings per action.** The four always-global actions
///   (Record meeting, Hands-free toggle, Copy last, Open Scratchpad) keep their
///   primary chord where it always lived (`meeting_chord`, …) and up to
///   [`MAX_BINDINGS_PER_ACTION`]` - 1` more in `AppSettings::extra_chords`.
///   The dictate toggle and Cancel keep exactly one: the dictate toggle has its
///   own hold/press path and Cancel is registered only while a take runs, so a
///   second key there would be a second lifecycle, not a second chord.
/// * **Cannot conflict.** [`check_add`] refuses a reserved macOS combo
///   ([`reserved_combo`] over `hotkey_table::RESERVED_CHORDS`: ⌘C, ⌘V, ⌘Tab …),
///   anything Wispr's rules refuse, a fixed Yap chord, a duplicate inside the
///   action, and a fifth binding; a chord another action already holds comes
///   back as [`Verdict::Overlaps`] with the warning sentence the row shows.
///   `save_settings` re-runs the same checks, so a hand-edited settings file
///   cannot double-book a chord either.
/// * **Never zero.** [`remove_at`] refuses to delete an action's last
///   binding; [`reset_to_default`] is the way back to the shipped chord.
pub mod editor {
    use serde::{Deserialize, Serialize};
    use tauri_plugin_global_shortcut::Shortcut;

    use crate::hotkey_table::{self, BindingAction};
    use crate::hotkeys::ChordSetting;

    /// The most shortcuts one action may carry (Wispr's limit).
    pub const MAX_BINDINGS_PER_ACTION: usize = 4;

    /// The sentence a refused delete of the last binding shows.
    pub const LAST_BINDING_REFUSAL: &str =
        "every action keeps at least one shortcut — add another before removing this one";

    /// One extra (non-primary) binding, as stored in `AppSettings::extra_chords`.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ExtraBinding {
        pub action: BindingAction,
        pub chord: ChordSetting,
    }

    /// What the editor says about a candidate chord.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Verdict {
        /// It may be added.
        Ok,
        /// Another action already holds it: the row shows this warning and the
        /// add is not applied (no two actions on one chord).
        Overlaps(String),
        /// It may never be a Yap shortcut (reserved macOS combo, Wispr rule,
        /// fixed Yap chord, duplicate, or the action is full).
        Refused(String),
    }

    /// Can `action` carry more than one binding?
    pub fn supports_extra(action: BindingAction) -> bool {
        BindingAction::ALWAYS_GLOBAL_REBINDABLE.contains(&action)
    }

    /// The editable actions, in the order the Shortcuts screen lists them.
    pub const EDITABLE: [BindingAction; 6] = [
        BindingAction::DictateToggle,
        BindingAction::Cancel,
        BindingAction::RecordMeeting,
        BindingAction::HandsFreeToggle,
        BindingAction::CopyLast,
        BindingAction::OpenScratchpad,
    ];

    /// The shipped chord of `action` as a setting (`None` for the hold keys).
    pub fn default_chord(action: BindingAction) -> Option<ChordSetting> {
        action.default_binding().map(|b| ChordSetting {
            code: format!("{:?}", b.code),
            ctrl: b.ctrl,
            alt: b.alt,
            cmd: b.cmd,
            shift: b.shift,
        })
    }

    /// The macOS function a chord belongs to, when it is a reserved combo
    /// (⌘C → "Copy", ⌘Tab → "the app switcher").
    pub fn reserved_combo(c: &ChordSetting) -> Option<&'static str> {
        hotkey_table::RESERVED_CHORDS
            .iter()
            .find(|(ctrl, alt, shift, cmd, code, _)| {
                (*ctrl, *alt, *shift, *cmd) == (c.ctrl, c.alt, c.shift, c.cmd) && *code == c.code
            })
            .map(|(.., what)| *what)
    }

    fn label_of(c: &ChordSetting) -> String {
        hotkey_table::format_chord(c).unwrap_or_else(|_| c.code.clone())
    }

    /// Every binding `action` holds: the primary (or its default) first, then
    /// its extras in stored order. Never empty for an editable action.
    pub fn bindings_of(
        action: BindingAction,
        primary: Option<&ChordSetting>,
        extras: &[ExtraBinding],
    ) -> Vec<ChordSetting> {
        let mut out: Vec<ChordSetting> = Vec::new();
        if let Some(p) = primary.cloned().or_else(|| default_chord(action)) {
            out.push(p);
        }
        if supports_extra(action) {
            for e in extras.iter().filter(|e| e.action == action) {
                if !out.contains(&e.chord) {
                    out.push(e.chord.clone());
                }
            }
        }
        out
    }

    /// The other action already holding `candidate`, if one does. `table` is
    /// every editable action with all of its bindings ([`bindings_of`]).
    pub fn overlapping_action(
        action: BindingAction,
        candidate: &ChordSetting,
        table: &[(BindingAction, Vec<ChordSetting>)],
    ) -> Option<BindingAction> {
        table
            .iter()
            .find(|(other, chords)| *other != action && chords.contains(candidate))
            .map(|(other, _)| *other)
    }

    /// The warning a row shows when `candidate` is already `other`'s.
    pub fn overlap_warning(candidate: &ChordSetting, other: BindingAction) -> String {
        format!(
            "{} is already {} — remove it there first, or pick another chord",
            label_of(candidate),
            other.title()
        )
    }

    /// Judge adding `candidate` to `action`, whose bindings are `current`.
    pub fn check_add(
        action: BindingAction,
        candidate: &ChordSetting,
        current: &[ChordSetting],
        table: &[(BindingAction, Vec<ChordSetting>)],
    ) -> Verdict {
        if !current.is_empty() && !supports_extra(action) {
            return Verdict::Refused(format!(
                "{} has one shortcut — change it instead of adding another",
                action.title()
            ));
        }
        if current.len() >= MAX_BINDINGS_PER_ACTION {
            return Verdict::Refused(format!(
                "{} already has {MAX_BINDINGS_PER_ACTION} shortcuts — remove one first",
                action.title()
            ));
        }
        if current.contains(candidate) {
            return Verdict::Refused(format!(
                "{} is already one of {}'s shortcuts",
                label_of(candidate),
                action.title()
            ));
        }
        // Reserved macOS combos, Wispr's rules and the fixed Yap chords — the
        // same gate a primary rebind goes through.
        if let Err(why) = hotkey_table::chord_for_action(action, candidate) {
            return Verdict::Refused(why);
        }
        match overlapping_action(action, candidate, table) {
            Some(other) => Verdict::Overlaps(overlap_warning(candidate, other)),
            None => Verdict::Ok,
        }
    }

    /// Remove binding `index` from `current`. Refuses the last one, so every
    /// action keeps at least one binding.
    pub fn remove_at(current: &[ChordSetting], index: usize) -> Result<Vec<ChordSetting>, String> {
        if index >= current.len() {
            return Err(format!("no shortcut at position {}", index + 1));
        }
        if current.len() <= 1 {
            return Err(LAST_BINDING_REFUSAL.to_string());
        }
        let mut out = current.to_vec();
        out.remove(index);
        Ok(out)
    }

    /// Reset to default: exactly the shipped chord.
    pub fn reset_to_default(action: BindingAction) -> Vec<ChordSetting> {
        default_chord(action).into_iter().collect()
    }

    /// Store a binding list back as (primary setting, extras). The primary is
    /// `None` when it is the shipped default, matching how every primary has
    /// always been stored.
    pub fn split(
        action: BindingAction,
        list: &[ChordSetting],
    ) -> (Option<ChordSetting>, Vec<ExtraBinding>) {
        let primary = list
            .first()
            .filter(|c| Some((*c).clone()) != default_chord(action))
            .cloned();
        let extras = if supports_extra(action) {
            list.iter()
                .skip(1)
                .map(|c| ExtraBinding {
                    action,
                    chord: c.clone(),
                })
                .collect()
        } else {
            Vec::new()
        };
        (primary, extras)
    }

    /// The live hands-free line under the hold rows, naming the hold key the
    /// `ptt_binding` setting arms: "Double-tap fn for hands-free".
    pub fn hands_free_line(ptt_binding: &str) -> String {
        let key = match ptt_binding {
            "fn" => "fn",
            "both" | "fn_or_fn_control" => "fn or fn⌃",
            _ => "fn⌃",
        };
        format!("Double-tap {key} for hands-free")
    }

    /// Validate every stored extra for a save: each must belong to an action
    /// that takes extras, pass the rebind gate, not repeat inside its action,
    /// and keep the action at or under [`MAX_BINDINGS_PER_ACTION`]. Returns the
    /// resolved chords, for `hotkey_table::check_distinct` alongside the
    /// primaries (which catches an extra that another action holds).
    pub fn validate_extras(
        primaries: &[(BindingAction, Option<&ChordSetting>)],
        extras: &[ExtraBinding],
    ) -> Result<Vec<(BindingAction, Shortcut, String)>, String> {
        let mut out = Vec::new();
        for (i, e) in extras.iter().enumerate() {
            if !supports_extra(e.action) {
                return Err(format!(
                    "{} has one shortcut — change it instead of adding another",
                    e.action.title()
                ));
            }
            let primary = primaries
                .iter()
                .find(|(a, _)| *a == e.action)
                .and_then(|(_, p)| *p);
            let before: Vec<ChordSetting> = {
                let mut v = bindings_of(e.action, primary, &[]);
                for prev in &extras[..i] {
                    if prev.action == e.action {
                        v.push(prev.chord.clone());
                    }
                }
                v
            };
            if before.contains(&e.chord) {
                return Err(format!(
                    "{} is already one of {}'s shortcuts",
                    label_of(&e.chord),
                    e.action.title()
                ));
            }
            if before.len() >= MAX_BINDINGS_PER_ACTION {
                return Err(format!(
                    "{} already has {MAX_BINDINGS_PER_ACTION} shortcuts — remove one first",
                    e.action.title()
                ));
            }
            let (sc, label) = hotkey_table::chord_for_action(e.action, &e.chord)?;
            out.push((e.action, sc, label));
        }
        Ok(out)
    }

    /// The extras Yap registers at launch: [`validate_extras`] one at a time,
    /// so a stored extra that breaks a rule (or that a newer reserved list
    /// refuses) is dropped with a log line instead of taking the rest down.
    pub fn resolve_extras_lenient(
        primaries: &[(BindingAction, Option<&ChordSetting>)],
        extras: &[ExtraBinding],
    ) -> Vec<(BindingAction, Shortcut, String)> {
        let mut kept: Vec<ExtraBinding> = Vec::new();
        let mut out = Vec::new();
        for e in extras {
            let mut trial = kept.clone();
            trial.push(e.clone());
            match validate_extras(primaries, &trial) {
                Ok(mut v) => {
                    if let Some(last) = v.pop() {
                        out.push(last);
                    }
                    kept.push(e.clone());
                }
                Err(why) => {
                    log::warn!("shortcuts extra_dropped action={:?} reason={why}", e.action)
                }
            }
        }
        out
    }
}
