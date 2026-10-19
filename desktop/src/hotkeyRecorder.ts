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
  if (!anyMod && !allowBare) return { kind: "hint", text: "Add ⌃, ⌥ or ⌘ to the key." };
  const chord: ChordSetting = { code: e.code, ctrl: e.ctrlKey, alt: e.altKey, cmd: e.metaKey, shift: e.shiftKey };
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
