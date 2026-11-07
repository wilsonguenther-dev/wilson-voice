import { describe, expect, it } from "vitest";
import {
  bindingOf, buttonLabel, captureMouse, makerButtonOfDom, MOUSE_BUTTON_MAX, MOUSE_PRIMARY_REFUSED,
  mouseBindingLine, parseMouseBinding, toStored, withMode,
} from "./mouseBinding";

describe("WF-19 mouse push-to-talk binding", () => {
  it("left and right click can never be bound", () => {
    for (const stored of [
      "left", "right", "primary", "secondary", "button1", "button2", "LEFT", " right ",
      "left:hands_free", "right:hold", "button1:hands_free", "button2:toggle",
    ]) {
      expect(parseMouseBinding(stored)).toEqual({ ok: false, why: MOUSE_PRIMARY_REFUSED });
    }
    for (const mode of ["hold", "hands_free"] as const) {
      expect(bindingOf(1, mode)).toEqual({ ok: false, why: MOUSE_PRIMARY_REFUSED });
      expect(bindingOf(2, mode)).toEqual({ ok: false, why: MOUSE_PRIMARY_REFUSED });
      // DOM 0 = left, DOM 2 = right: the recorder refuses both.
      expect(captureMouse(0, mode)).toEqual({ ok: false, why: MOUSE_PRIMARY_REFUSED });
      expect(captureMouse(2, mode)).toEqual({ ok: false, why: MOUSE_PRIMARY_REFUSED });
    }
    // Every stored value that parses is a non-primary button.
    for (let n = 3; n <= MOUSE_BUTTON_MAX; n++) {
      const p = parseMouseBinding(`button${n}`);
      expect(p.ok && p.binding && p.binding.button >= 3).toBe(true);
    }
  });

  it("the refusal line matches the Rust one", () => {
    expect(MOUSE_PRIMARY_REFUSED).toBe(
      "Left and right click can't be shortcuts — pick the middle button or a side button",
    );
  });

  it("captures middle and side buttons", () => {
    expect(captureMouse(1, "hold")).toEqual({ ok: true, binding: { button: 3, mode: "hold" } });
    expect(captureMouse(3, "hold")).toEqual({ ok: true, binding: { button: 4, mode: "hold" } });
    expect(captureMouse(4, "hands_free")).toEqual({ ok: true, binding: { button: 5, mode: "hands_free" } });
    expect(makerButtonOfDom(0)).toBe(1);
    expect(makerButtonOfDom(2)).toBe(2);
  });

  it("round-trips the stored form", () => {
    for (const s of ["middle", "button4", "button5:hands_free", "middle:hands_free"]) {
      const p = parseMouseBinding(s);
      expect(p.ok).toBe(true);
      if (p.ok) expect(toStored(p.binding)).toBe(s);
    }
    expect(parseMouseBinding("button3")).toEqual({ ok: true, binding: { button: 3, mode: "hold" } });
    expect(parseMouseBinding("button4:ptt")).toEqual({ ok: true, binding: { button: 4, mode: "hold" } });
    expect(parseMouseBinding("button4:push_to_talk")).toEqual({ ok: true, binding: { button: 4, mode: "hold" } });
    expect(toStored(null)).toBe("");
  });

  it("off unbinds; garbage is refused with its own reason", () => {
    for (const off of ["", "off", "none", "  OFF ", null, undefined]) {
      expect(parseMouseBinding(off)).toEqual({ ok: true, binding: null });
    }
    for (const bad of ["button0", "button33", "wheel", "button", "middle:sideways", "button4:"]) {
      const p = parseMouseBinding(bad);
      expect(p.ok).toBe(false);
      if (!p.ok) expect(p.why).not.toBe(MOUSE_PRIMARY_REFUSED);
    }
    expect(bindingOf(0, "hold").ok).toBe(false);
    expect(bindingOf(3.5, "hold").ok).toBe(false);
  });

  it("labels and mode switch", () => {
    expect(buttonLabel(3)).toBe("middle button");
    expect(buttonLabel(4)).toBe("button 4");
    expect(mouseBindingLine("")).toBe("Off");
    expect(mouseBindingLine("left")).toBe("Off — the saved mouse button isn't valid");
    expect(mouseBindingLine("button4")).toBe("Hold the button 4 to talk, double-click for hands-free");
    expect(mouseBindingLine("middle:hands_free")).toBe("Click the middle button to start, click again to stop");
    expect(withMode("button4", "hands_free")).toBe("button4:hands_free");
    expect(withMode("button4:hands_free", "hold")).toBe("button4");
    expect(withMode("", "hold")).toBe("");
    expect(withMode("right", "hold")).toBe("");
  });
});
