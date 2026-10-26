/**
 * focus-2026-10-19 — the hotkey recorder's decision, pure so vitest covers it.
 * One keydown → cancel / hint / a captured chord (Carbon actions) or a hold key
 * (fn combos, which only the fn tap can express).
 */
import type { ChordSetting } from "./appTypes";
import reservedChords from "./reservedChords.json";

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
 * fix-249 BLOCKING 2 — macOS / system chords a rebind may never take:
 * [ctrl, alt, shift, cmd, code, what]. The list lives in reservedChords.json,
 * which Rust test `hotkey_table::reserved_chords_parity` compares to
 * `RESERVED_CHORDS` (drift fails cargo test); both sides also run the shared
 * case table reservedChordCases.json.
 */
export const RESERVED_CHORDS: ReadonlyArray<readonly [boolean, boolean, boolean, boolean, string, string]> =
  reservedChords.map((r) => [r.ctrl, r.alt, r.shift, r.cmd, r.code, r.what] as const);

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

/**
 * advisory 3 (#249) — the recorder's copy. While listening, the button that
 * stops it says how to cancel: Esc for every recorder except the Cancel-key
 * one, where Esc is a valid answer, so there the button itself is the way out.
 */
export function recorderPrompt(mode: "chord" | "hold", allowBare = false): { listening: string; hint: string } {
  if (mode === "hold") {
    return { listening: "Cancel · Esc to cancel", hint: "Press fn or fn + Control. Esc to cancel." };
  }
  if (allowBare) {
    return {
      listening: "Cancel · click to keep the current key",
      hint: "Press the new cancel key — Esc or F1–F20 on its own, or a chord with ⌃, ⌥ or ⌘. Esc here sets Esc, so click Cancel to keep the current key.",
    };
  }
  return { listening: "Cancel · Esc to cancel", hint: "Press the new chord. Esc to cancel." };
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
  /** Why the stored chord is not the one Yap holds (launch re-validation), or null. */
  note: string | null;
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
