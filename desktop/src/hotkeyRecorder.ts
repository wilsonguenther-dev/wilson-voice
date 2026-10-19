/**
 * focus-2026-10-19 — the hotkey recorder's decision, pure so vitest covers it.
 * One keydown → cancel / hint / a captured chord (Carbon actions) or a hold key
 * (fn combos, which only the fn tap can express).
 */
import type { ChordSetting } from "./appTypes";

const MODIFIER_CODES = new Set([
  "ControlLeft", "ControlRight", "AltLeft", "AltRight", "MetaLeft", "MetaRight",
  "ShiftLeft", "ShiftRight", "CapsLock", "Fn",
]);

export interface KeyLike {
  code: string;
  key?: string;
  ctrlKey: boolean;
  altKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  getModifierState?: (k: string) => boolean;
}

export type RecorderStep =
  | { kind: "cancel" }
  | { kind: "hint"; text: string }
  | { kind: "chord"; chord: ChordSetting; label: string }
  | { kind: "hold"; binding: "fn" | "fn_control" };

/** Glyph label, identical to Rust `hotkeys::chord_label` (⌃⌥⇧⌘ then key). */
export function formatChord(c: ChordSetting): string {
  const mods = `${c.ctrl ? "⌃" : ""}${c.alt ? "⌥" : ""}${c.shift ? "⇧" : ""}${c.cmd ? "⌘" : ""}`;
  const key = c.code.replace(/^Key/, "").replace(/^Digit/, "");
  return mods + key;
}

/**
 * fix-249 BLOCKING 2 — macOS / system chords a rebind may never take. Mirror of
 * Rust `hotkey_table::RESERVED_CHORDS`: [ctrl, alt, shift, cmd, code, what].
 */
export const RESERVED_CHORDS: ReadonlyArray<readonly [boolean, boolean, boolean, boolean, string, string]> = [
  [false, false, false, true, "KeyQ", "Quit"],
  [false, false, false, true, "KeyW", "Close Window"],
  [false, false, false, true, "KeyC", "Copy"],
  [false, false, false, true, "KeyV", "Paste"],
  [false, false, false, true, "KeyX", "Cut"],
  [false, false, false, true, "KeyZ", "Undo"],
  [false, false, false, true, "KeyA", "Select All"],
  [false, false, false, true, "KeyS", "Save"],
  [false, false, false, true, "KeyH", "Hide"],
  [false, false, false, true, "KeyM", "Minimize"],
  [false, false, false, true, "Tab", "the app switcher"],
  [false, false, false, true, "Space", "Spotlight"],
  [false, false, false, true, "Backquote", "next window"],
  [false, false, true, true, "KeyQ", "Log Out"],
  [false, false, true, true, "Digit3", "screenshot"],
  [false, false, true, true, "Digit4", "screenshot selection"],
  [false, false, true, true, "Digit5", "the screenshot toolbar"],
  [true, false, false, true, "KeyQ", "Lock Screen"],
  [true, false, false, true, "KeyF", "full screen"],
  [true, false, false, true, "Space", "the emoji picker"],
  [false, true, false, true, "Escape", "Force Quit"],
  [false, true, false, true, "KeyD", "show / hide the Dock"],
  [false, true, false, true, "Space", "Finder search"],
  [true, false, false, false, "Space", "input-source switching"],
  [true, true, false, false, "Space", "input-source switching"],
  [true, false, false, false, "ArrowUp", "Mission Control"],
  [true, false, false, false, "ArrowDown", "App Exposé"],
  [true, false, false, false, "ArrowLeft", "the previous Space"],
  [true, false, false, false, "ArrowRight", "the next Space"],
];

function bareCancelKeyOk(code: string): boolean {
  if (code === "Escape") return true;
  const m = /^F(\d{1,2})$/.exec(code);
  return m !== null && Number(m[1]) >= 1 && Number(m[1]) <= 20;
}

/** Why `c` may not be rebound to, or null. Same rules and words as Rust `reserved_chord_reason`. */
export function reservedChordReason(c: ChordSetting, allowBare: boolean): string | null {
  const label = formatChord(c);
  const hit = RESERVED_CHORDS.find(([ctrl, alt, shift, cmd, code]) =>
    ctrl === c.ctrl && alt === c.alt && shift === c.shift && cmd === c.cmd && code === c.code);
  if (hit) return `${label} is macOS's ${hit[5]} — pick another chord`;
  if (!(c.ctrl || c.alt || c.cmd)) {
    if (allowBare && !c.shift && bareCancelKeyOk(c.code)) return null;
    return allowBare
      ? `${label} would stop typing that key in every app during a take — use Esc, an F-key, or add ⌃, ⌥ or ⌘`
      : `${label} needs ⌃, ⌥ or ⌘ — Yap holds it in every app`;
  }
  if (c.cmd && !c.ctrl && !c.alt) return `${label} is an app menu shortcut in every app — add ⌃ or ⌥`;
  return null;
}

function fnHeld(e: KeyLike): boolean {
  return e.key === "Fn" || e.code === "Fn" || Boolean(e.getModifierState?.("Fn"));
}

/**
 * `mode`: "chord" for Carbon actions (dictate toggle, meeting, cancel);
 * "hold" for the fn hold key. `allowBare`: the Cancel key may be a bare key
 * (it is registered only while a take runs).
 */
export function recorderStep(e: KeyLike, mode: "chord" | "hold", allowBare = false): RecorderStep {
  const anyMod = e.ctrlKey || e.altKey || e.metaKey;
  if (e.code === "Escape" && !anyMod && !(mode === "chord" && allowBare && e.shiftKey)) {
    // Escape cancels the recorder — except a Cancel recorder, where Esc is a
    // valid answer: Shift-free Esc there is captured below instead.
    if (!(allowBare && mode === "chord")) return { kind: "cancel" };
  }
  if (mode === "hold") {
    if (fnHeld(e)) return { kind: "hold", binding: e.ctrlKey ? "fn_control" : "fn" };
    return { kind: "hint", text: "Press fn (or fn + Control). Other keys are set under Dictate (toggle)." };
  }
  if (MODIFIER_CODES.has(e.code)) return { kind: "hint", text: "Keep holding the modifiers and press a key." };
  const chord: ChordSetting = { code: e.code, ctrl: e.ctrlKey, alt: e.altKey, cmd: e.metaKey, shift: e.shiftKey };
  const why = reservedChordReason(chord, allowBare);
  if (why) return { kind: "hint", text: why };
  return { kind: "chord", chord, label: formatChord(chord) };
}

/** One row of the backend's hotkey table (`hotkey_bindings`). */
export interface BindingRow {
  action: "dictate_hold" | "hands_free" | "dictate_toggle" | "cancel" | "record_meeting";
  title: string;
  key: string;
  status: { state: string; detail?: string };
  chip: string;
  liveKey: string | null;
  fallback: boolean;
}

export interface HotkeyBindingsReport {
  rows: BindingRow[];
  liveDictateKey: string | null;
  dump: string;
}

/** Chip tone for a row status. */
export function chipTone(state: string): "ok" | "warn" | "bad" {
  if (state === "works" || state === "ready" || state === "during_take_only") return "ok";
  if (state === "paused" || state === "not_started") return "warn";
  return "bad";
}
