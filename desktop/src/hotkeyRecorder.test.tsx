import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { chipTone, formatChord, recorderStep } from "./hotkeyRecorder";
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
