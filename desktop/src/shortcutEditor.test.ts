import { describe, expect, it } from "vitest";
import type { AppSettings, ChordSetting } from "./appTypes";
import { RESERVED_CHORDS } from "./hotkeyRecorder";
import {
  ACTIONS, addBinding, bindingsOf, checkAdd, handsFreeLine, LAST_BINDING_REFUSAL,
  MAX_BINDINGS_PER_ACTION, removeAt, resetAllToDefault, resetToDefault,
} from "./shortcutEditor";

const c = (code: string, m: Partial<Omit<ChordSetting, "code">> = {}): ChordSetting =>
  ({ code, ctrl: !!m.ctrl, alt: !!m.alt, shift: !!m.shift, cmd: !!m.cmd });
const base = {} as AppSettings;

describe("WF-18 shortcuts editor — reserved combos", () => {
  it("refuses every reserved macOS combo (⌘C, ⌘V, ⌘Tab …) as a new binding", () => {
    for (const [ctrl, alt, shift, cmd, code, what] of RESERVED_CHORDS) {
      const v = checkAdd(base, "record_meeting", { code, ctrl, alt, shift, cmd });
      expect(v.kind, `${code} ${what}`).toBe("refused");
    }
    const copy = checkAdd(base, "copy_last", c("KeyC", { cmd: true }));
    expect(copy).toEqual({ kind: "refused", text: "⌘C is macOS's Copy — pick another chord" });
    expect(checkAdd(base, "copy_last", c("Tab", { cmd: true })).kind).toBe("refused");
    expect(checkAdd(base, "copy_last", c("KeyV", { cmd: true })).kind).toBe("refused");
  });
  it("refuses a fixed Yap chord and a four-key chord", () => {
    expect(checkAdd(base, "copy_last", c("KeyV", { ctrl: true, cmd: true })).kind).toBe("refused");
    expect(checkAdd(base, "copy_last", c("KeyK", { ctrl: true, alt: true, cmd: true })).kind).toBe("refused");
  });
});

describe("WF-18 shortcuts editor — overlap check", () => {
  it("warns when a new binding is another action's chord, and does not apply it", () => {
    const r = addBinding(base, "record_meeting", c("KeyH", { ctrl: true, cmd: true }));
    expect(r.next).toBeNull();
    expect(r.verdict).toEqual({
      kind: "overlaps",
      other: "hands_free_toggle",
      text: "⌃⌘H is already Hands-free (toggle) — remove it there first, or pick another chord",
    });
  });
  it("warns on another action's EXTRA binding too", () => {
    const s = { extraChords: [{ action: "copy_last", chord: c("KeyK", { ctrl: true, alt: true }) }] } as AppSettings;
    const v = checkAdd(s, "open_scratchpad", c("KeyK", { ctrl: true, alt: true }));
    expect(v.kind).toBe("overlaps");
    expect(v.kind === "overlaps" && v.other).toBe("copy_last");
  });
  it("lets a free chord through", () => {
    const r = addBinding(base, "record_meeting", c("KeyR", { ctrl: true, alt: true }));
    expect(r.verdict.kind).toBe("ok");
    expect(r.next?.extraChords).toEqual([{ action: "record_meeting", chord: c("KeyR", { ctrl: true, alt: true }) }]);
  });
});

describe("WF-18 shortcuts editor — several bindings, never zero", () => {
  it("takes up to four bindings per always-global action", () => {
    let s = base;
    for (const k of ["KeyR", "KeyT", "KeyY"]) {
      const r = addBinding(s, "copy_last", c(k, { ctrl: true, alt: true }));
      expect(r.verdict.kind, k).toBe("ok");
      s = r.next!;
    }
    expect(bindingsOf(s, "copy_last")).toHaveLength(MAX_BINDINGS_PER_ACTION);
    expect(checkAdd(s, "copy_last", c("KeyU", { ctrl: true, alt: true })).kind).toBe("refused");
  });
  it("the dictate toggle and Cancel keep exactly one", () => {
    expect(checkAdd(base, "dictate_toggle", c("KeyR", { ctrl: true, alt: true })).kind).toBe("refused");
    expect(checkAdd(base, "cancel", c("KeyR", { ctrl: true, alt: true })).kind).toBe("refused");
  });
  it("never deletes the last binding", () => {
    for (const a of ACTIONS) {
      expect(bindingsOf(base, a.action)).toHaveLength(1);
      expect(removeAt(base, a.action, 0)).toEqual({ next: null, error: LAST_BINDING_REFUSAL });
    }
    const two = addBinding(base, "record_meeting", c("KeyR", { ctrl: true, alt: true })).next!;
    const one = removeAt(two, "record_meeting", 0).next!;
    expect(bindingsOf(one, "record_meeting")).toEqual([c("KeyR", { ctrl: true, alt: true })]);
    expect(removeAt(one, "record_meeting", 0).error).toBe(LAST_BINDING_REFUSAL);
  });
  it("Reset to default puts back the shipped chord and drops extras", () => {
    const s = {
      meetingChord: c("KeyK", { ctrl: true, alt: true }),
      extraChords: [{ action: "record_meeting", chord: c("KeyR", { ctrl: true, alt: true }) }],
    } as AppSettings;
    const r = resetToDefault(s, "record_meeting");
    expect(r.meetingChord).toBeNull();
    expect(bindingsOf(r, "record_meeting")).toEqual([c("KeyM", { ctrl: true, cmd: true })]);
    const all = resetAllToDefault({ ...s, cancelChord: c("F13") });
    expect(all.cancelChord).toBeNull();
    expect(all.extraChords).toEqual([]);
  });
});

describe("WF-18 shortcuts editor — hands-free line", () => {
  it("names the live hold key", () => {
    expect(handsFreeLine("fn")).toBe("Double-tap fn for hands-free");
    expect(handsFreeLine("fn_control")).toBe("Double-tap fn⌃ for hands-free");
    expect(handsFreeLine(undefined)).toBe("Double-tap fn⌃ for hands-free");
  });
});
