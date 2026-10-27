/** Y5-K — the registry: one id -> one creature, and an unknown id is classic. */
import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(null)) }));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn(), listen: vi.fn(() => Promise.resolve(() => {})) }));

import { FALLBACK_CHARACTER, characters, registerCharacter, resolveCharacter } from "./registry";
import { classic } from "./classic";
import { yappy } from "./yappy";

describe("character registry", () => {
  it("ships classic and yappy, in that order", () => {
    expect(characters().slice(0, 2)).toEqual([classic, yappy]);
    expect(FALLBACK_CHARACTER).toBe("classic");
  });

  it("resolves pill-style setting once, and an unknown, empty or missing id falls back to classic", () => {
    expect(resolveCharacter("yappy")).toBe(yappy);
    expect(resolveCharacter("classic")).toBe(classic);
    for (const id of ["unicorn", "", null, undefined, "Yappy"]) expect(resolveCharacter(id).id).toBe("classic");
  });

  it("re-registering the same creature is a no-op; a different one under a taken id throws", () => {
    const before = characters().length;
    expect(registerCharacter(yappy)).toBe(yappy);
    expect(characters().length).toBe(before);
    expect(() => registerCharacter({ ...yappy })).toThrow(/already registered/);
    expect(() => registerCharacter({ id: " ", label: "x", render: () => null })).toThrow(/needs an id/);
  });

  it("only Yappy declares a whole-window stage; classic paints in the shell's body slot", () => {
    expect(typeof yappy.stage).toBe("object");
    expect(classic.stage).toBeUndefined();
  });
});
