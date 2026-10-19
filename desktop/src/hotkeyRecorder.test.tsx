import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { chipTone, formatChord, recorderStep, RESERVED_CHORDS, reservedChordReason } from "./hotkeyRecorder";
import HotkeyRecorder from "./HotkeyRow";

const key = (code: string, mods: Partial<{ ctrl: boolean; alt: boolean; cmd: boolean; shift: boolean }> = {}, extra: Record<string, unknown> = {}) => ({
  code,
  ctrlKey: Boolean(mods.ctrl),
  altKey: Boolean(mods.alt),
  metaKey: Boolean(mods.cmd),
  shiftKey: Boolean(mods.shift),
  ...extra,
});

describe("hotkey recorder", () => {
  it("captures a chord and formats it like Rust chord_label", () => {
    const s = recorderStep(key("Space", { ctrl: true, alt: true, cmd: true }), "chord");
    expect(s).toEqual({ kind: "chord", chord: { code: "Space", ctrl: true, alt: true, cmd: true, shift: false }, label: "⌃⌥⌘Space" });
    expect(formatChord({ code: "KeyM", ctrl: true, cmd: true, alt: false, shift: false })).toBe("⌃⌘M");
    expect(formatChord({ code: "Digit1", ctrl: false, cmd: true, alt: false, shift: true })).toBe("⇧⌘1");
  });

  it("refuses a bare key for system-wide chords but allows it for Cancel", () => {
    expect(recorderStep(key("KeyM"), "chord").kind).toBe("hint");
    expect(recorderStep(key("F6"), "chord", true)).toMatchObject({ kind: "chord", label: "F6" });
    expect(recorderStep(key("Escape"), "chord", true)).toMatchObject({ kind: "chord", label: "Escape" });
  });

  it("refuses every macOS reserved / system chord and says why (fix-249 BLOCKING 2)", () => {
    for (const [ctrl, alt, shift, cmd, code, what] of RESERVED_CHORDS) {
      for (const allowBare of [false, true]) {
        const s = recorderStep(key(code, { ctrl, alt, shift, cmd }), "chord", allowBare);
        expect(s.kind, `${code} ${what}`).toBe("hint");
        if (s.kind === "hint") expect(s.text).toContain(what);
      }
    }
    for (const [code, mods] of [
      ["KeyQ", { cmd: true }], ["KeyW", { cmd: true }], ["KeyC", { cmd: true }], ["KeyV", { cmd: true }],
      ["KeyX", { cmd: true }], ["KeyZ", { cmd: true }], ["KeyA", { cmd: true }], ["KeyS", { cmd: true }],
      ["Tab", { cmd: true }], ["Space", { cmd: true }], ["KeyH", { cmd: true }], ["KeyM", { cmd: true }],
      ["KeyQ", { ctrl: true, cmd: true }], ["Escape", { alt: true, cmd: true }], ["Backquote", { cmd: true }],
      ["KeyN", { cmd: true }], ["KeyN", { cmd: true, shift: true }], ["KeyR", {}], ["KeyR", { shift: true }],
    ] as const) {
      expect(recorderStep(key(code, mods), "chord").kind, code).toBe("hint");
    }
    const q = recorderStep(key("KeyQ", { cmd: true }), "chord");
    expect(q).toEqual({ kind: "hint", text: "⌘Q is macOS's Quit — pick another chord" });
    // Cancel: bare Esc / F-keys only.
    expect(recorderStep(key("KeyR"), "chord", true).kind).toBe("hint");
    expect(recorderStep(key("F13"), "chord", true).kind).toBe("chord");
    expect(reservedChordReason({ code: "F13", ctrl: false, alt: false, shift: false, cmd: false }, false)).toContain("needs ⌃, ⌥ or ⌘");
    // Ordinary chords pass.
    expect(recorderStep(key("KeyR", { ctrl: true, cmd: true }), "chord").kind).toBe("chord");
    expect(recorderStep(key("KeyR", { ctrl: true, alt: true, cmd: true }), "chord").kind).toBe("chord");
  });

  it("Esc cancels a normal recorder; modifier-only presses ask for a key", () => {
    expect(recorderStep(key("Escape"), "chord")).toEqual({ kind: "cancel" });
    expect(recorderStep(key("MetaLeft", { cmd: true }), "chord").kind).toBe("hint");
  });

  it("captures fn combos for the hold key", () => {
    expect(recorderStep(key("Fn", {}, { key: "Fn" }), "hold")).toEqual({ kind: "hold", binding: "fn" });
    expect(recorderStep(key("ControlLeft", { ctrl: true }, { getModifierState: (k: string) => k === "Fn" }), "hold")).toEqual({ kind: "hold", binding: "fn_control" });
    expect(recorderStep(key("KeyA"), "hold").kind).toBe("hint");
  });

  it("renders key, status chip, fallback key and Reset", () => {
    const html = renderToStaticMarkup(
      <HotkeyRecorder
        title="Dictate (hold)"
        keyLabel="fn⌃"
        defaultLabel="fn⌃"
        isDefault={false}
        mode="hold"
        row={{ action: "dictate_hold", title: "Dictate (hold)", key: "fn⌃", status: { state: "paused" }, chip: "Paused by macOS — re-arming", liveKey: "⌃⌥⌘Space", fallback: true }}
        onCapture={() => {}}
        onReset={() => {}}
      />,
    );
    expect(html).toContain("fn⌃");
    expect(html).toContain("Paused by macOS");
    expect(html).toContain("use ⌃⌥⌘Space now");
    expect(html).toContain("Reset to fn⌃");
    expect(html).toContain("Record");
    expect(chipTone("works")).toBe("ok");
    expect(chipTone("stale_grant")).toBe("bad");
  });
});
